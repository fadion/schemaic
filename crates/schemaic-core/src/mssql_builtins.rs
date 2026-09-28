//! The SQL Server builtin function catalog.
//!
//! [`MSSQL_FUNCTIONS`] is the fourth entry in `intel::builtin_catalog`, and the
//! one that could not be generated. PostgreSQL's comes out of `pg_catalog`;
//! T-SQL's intrinsics — `LEN`, `DATEADD`, `ISNULL` — are in no catalog view at
//! all (`sys.all_objects` holds the `sys.fn_*` system functions, not these), so
//! this list is written from Microsoft's *Functions (Transact-SQL)* reference,
//! category by category, as the SQLite one is from SQLite's.
//!
//! **What checks it is the parser.** A name T-SQL does not know fails to compile
//! with Msg 195, *is not a recognized built-in function name*; a builtin called
//! with the wrong arguments fails with a different message, and nothing runs.
//! So `live::mssql::every_catalogued_builtin_is_one_the_server_knows` calls
//! each entry with no arguments and asserts the answer is anything but 195 —
//! the over-listing direction, which is the one a hand-written list gets wrong
//! by memory. The other direction, a builtin this list lacks, has no oracle, and
//! costs a squiggle only where the missing name is a near miss of one held.
//!
//! **SQL Server 2022's set.** 2025 adds `REGEXP_*`, `EDIT_DISTANCE`, `UNISTR`,
//! `PRODUCT` and `CURRENT_DATE`, which the tier's server does not have; they
//! belong here once a leg can check them. Rowset functions (`OPENJSON`,
//! `STRING_SPLIT`, `GENERATE_SERIES`, `OPENROWSET`) are listed too — they are
//! called, in `FROM` — and the oracle calls them there.
//!
//! Upper-case names, as MySQL's [`crate::intel::FUNCTIONS`] are; the index
//! lower-cases every catalog for its lookups.

use crate::intel::{SqlFunction, f};

/// SQL Server's builtins, grouped by Microsoft's categories.
pub const MSSQL_FUNCTIONS: &[SqlFunction] = &[
    // ── Aggregate ────────────────────────────────────────────────────────────
    f(
        "AVG",
        "AVG([ALL | DISTINCT] expression)",
        "Average of the values",
    ),
    f(
        "CHECKSUM_AGG",
        "CHECKSUM_AGG([ALL | DISTINCT] expression)",
        "Checksum of the group's values",
    ),
    f(
        "COUNT",
        "COUNT({* | [ALL | DISTINCT] expression})",
        "Number of rows (int)",
    ),
    f(
        "COUNT_BIG",
        "COUNT_BIG({* | [ALL | DISTINCT] expression})",
        "Number of rows (bigint)",
    ),
    f(
        "GROUPING",
        "GROUPING(column)",
        "1 when the column is aggregated in a ROLLUP/CUBE row",
    ),
    f(
        "GROUPING_ID",
        "GROUPING_ID(column [, …n])",
        "Bit mask of the GROUPING of each column",
    ),
    f("MAX", "MAX([ALL | DISTINCT] expression)", "Largest value"),
    f("MIN", "MIN([ALL | DISTINCT] expression)", "Smallest value"),
    f(
        "STDEV",
        "STDEV([ALL | DISTINCT] expression)",
        "Sample standard deviation",
    ),
    f(
        "STDEVP",
        "STDEVP([ALL | DISTINCT] expression)",
        "Population standard deviation",
    ),
    f(
        "STRING_AGG",
        "STRING_AGG(expression, separator)",
        "Values joined by the separator",
    ),
    f(
        "SUM",
        "SUM([ALL | DISTINCT] expression)",
        "Sum of the values",
    ),
    f("VAR", "VAR([ALL | DISTINCT] expression)", "Sample variance"),
    f(
        "VARP",
        "VARP([ALL | DISTINCT] expression)",
        "Population variance",
    ),
    f(
        "APPROX_COUNT_DISTINCT",
        "APPROX_COUNT_DISTINCT(expression)",
        "Approximate number of distinct values",
    ),
    f(
        "APPROX_PERCENTILE_CONT",
        "APPROX_PERCENTILE_CONT(numeric_literal)",
        "Approximate interpolated percentile (WITHIN GROUP)",
    ),
    f(
        "APPROX_PERCENTILE_DISC",
        "APPROX_PERCENTILE_DISC(numeric_literal)",
        "Approximate discrete percentile (WITHIN GROUP)",
    ),
    // ── Ranking and analytic ─────────────────────────────────────────────────
    f(
        "ROW_NUMBER",
        "ROW_NUMBER() OVER (…)",
        "Sequential number of the row in its partition",
    ),
    f("RANK", "RANK() OVER (…)", "Rank with gaps after ties"),
    f("DENSE_RANK", "DENSE_RANK() OVER (…)", "Rank without gaps"),
    f(
        "NTILE",
        "NTILE(integer_expression) OVER (…)",
        "Bucket number, the partition split into n groups",
    ),
    f(
        "CUME_DIST",
        "CUME_DIST() OVER (…)",
        "Cumulative distribution of the row",
    ),
    f(
        "PERCENT_RANK",
        "PERCENT_RANK() OVER (…)",
        "Relative rank, 0 to 1",
    ),
    f(
        "FIRST_VALUE",
        "FIRST_VALUE(expression) OVER (…)",
        "Value at the frame's first row",
    ),
    f(
        "LAST_VALUE",
        "LAST_VALUE(expression) OVER (…)",
        "Value at the frame's last row",
    ),
    f(
        "LAG",
        "LAG(expression [, offset [, default]]) OVER (…)",
        "Value from a preceding row",
    ),
    f(
        "LEAD",
        "LEAD(expression [, offset [, default]]) OVER (…)",
        "Value from a following row",
    ),
    f(
        "PERCENTILE_CONT",
        "PERCENTILE_CONT(numeric_literal) WITHIN GROUP (ORDER BY …) OVER (…)",
        "Interpolated percentile",
    ),
    f(
        "PERCENTILE_DISC",
        "PERCENTILE_DISC(numeric_literal) WITHIN GROUP (ORDER BY …) OVER (…)",
        "Discrete percentile",
    ),
    // ── Conversion ───────────────────────────────────────────────────────────
    f(
        "CAST",
        "CAST(expression AS data_type[(length)])",
        "Value converted to the type",
    ),
    f(
        "CONVERT",
        "CONVERT(data_type[(length)], expression [, style])",
        "Value converted to the type, with a style",
    ),
    f(
        "PARSE",
        "PARSE(string AS data_type [USING culture])",
        "String parsed as a date or number",
    ),
    f(
        "TRY_CAST",
        "TRY_CAST(expression AS data_type[(length)])",
        "CAST, or NULL where it fails",
    ),
    f(
        "TRY_CONVERT",
        "TRY_CONVERT(data_type[(length)], expression [, style])",
        "CONVERT, or NULL where it fails",
    ),
    f(
        "TRY_PARSE",
        "TRY_PARSE(string AS data_type [USING culture])",
        "PARSE, or NULL where it fails",
    ),
    // ── Date and time ────────────────────────────────────────────────────────
    f(
        "CURRENT_TIMESTAMP",
        "CURRENT_TIMESTAMP",
        "Current date and time (datetime), no parentheses",
    ),
    f(
        "CURRENT_TIMEZONE",
        "CURRENT_TIMEZONE()",
        "The server's time zone",
    ),
    f(
        "CURRENT_TIMEZONE_ID",
        "CURRENT_TIMEZONE_ID()",
        "The server's time zone id",
    ),
    f(
        "DATE_BUCKET",
        "DATE_BUCKET(datepart, number, date [, origin])",
        "Start of the bucket the date falls in",
    ),
    f(
        "DATEADD",
        "DATEADD(datepart, number, date)",
        "Date with an interval added",
    ),
    f(
        "DATEDIFF",
        "DATEDIFF(datepart, startdate, enddate)",
        "Boundaries crossed between two dates (int)",
    ),
    f(
        "DATEDIFF_BIG",
        "DATEDIFF_BIG(datepart, startdate, enddate)",
        "Boundaries crossed between two dates (bigint)",
    ),
    f(
        "DATEFROMPARTS",
        "DATEFROMPARTS(year, month, day)",
        "Date from its parts",
    ),
    f(
        "DATENAME",
        "DATENAME(datepart, date)",
        "Name of the date part (text)",
    ),
    f(
        "DATEPART",
        "DATEPART(datepart, date)",
        "The date part (int)",
    ),
    f(
        "DATETIME2FROMPARTS",
        "DATETIME2FROMPARTS(year, month, day, hour, minute, seconds, fractions, precision)",
        "datetime2 from its parts",
    ),
    f(
        "DATETIMEFROMPARTS",
        "DATETIMEFROMPARTS(year, month, day, hour, minute, seconds, milliseconds)",
        "datetime from its parts",
    ),
    f(
        "DATETIMEOFFSETFROMPARTS",
        "DATETIMEOFFSETFROMPARTS(year, month, day, hour, minute, seconds, fractions, hour_offset, minute_offset, precision)",
        "datetimeoffset from its parts",
    ),
    f(
        "DATETRUNC",
        "DATETRUNC(datepart, date)",
        "Date truncated to the part",
    ),
    f("DAY", "DAY(date)", "Day of the month"),
    f(
        "EOMONTH",
        "EOMONTH(start_date [, month_to_add])",
        "Last day of the month",
    ),
    f("GETDATE", "GETDATE()", "Current date and time (datetime)"),
    f(
        "GETUTCDATE",
        "GETUTCDATE()",
        "Current UTC date and time (datetime)",
    ),
    f(
        "ISDATE",
        "ISDATE(expression)",
        "1 when the value converts to a date",
    ),
    f("MONTH", "MONTH(date)", "Month of the year"),
    f(
        "SMALLDATETIMEFROMPARTS",
        "SMALLDATETIMEFROMPARTS(year, month, day, hour, minute)",
        "smalldatetime from its parts",
    ),
    f(
        "SWITCHOFFSET",
        "SWITCHOFFSET(datetimeoffset, time_zone)",
        "The same instant at another offset",
    ),
    f(
        "SYSDATETIME",
        "SYSDATETIME()",
        "Current date and time (datetime2)",
    ),
    f(
        "SYSDATETIMEOFFSET",
        "SYSDATETIMEOFFSET()",
        "Current date and time with its offset",
    ),
    f(
        "SYSUTCDATETIME",
        "SYSUTCDATETIME()",
        "Current UTC date and time (datetime2)",
    ),
    f(
        "TIMEFROMPARTS",
        "TIMEFROMPARTS(hour, minute, seconds, fractions, precision)",
        "time from its parts",
    ),
    f(
        "TODATETIMEOFFSET",
        "TODATETIMEOFFSET(datetime, time_zone)",
        "Date and time with an offset attached",
    ),
    f("YEAR", "YEAR(date)", "The year"),
    // ── Logical ──────────────────────────────────────────────────────────────
    f(
        "CHOOSE",
        "CHOOSE(index, val_1, val_2 [, …n])",
        "The value at the 1-based index",
    ),
    f(
        "COALESCE",
        "COALESCE(expression [, …n])",
        "First value that is not NULL",
    ),
    f(
        "GREATEST",
        "GREATEST(expression1 [, …n])",
        "Largest of the values",
    ),
    f(
        "IIF",
        "IIF(boolean_expression, true_value, false_value)",
        "One of two values, by a condition",
    ),
    f(
        "LEAST",
        "LEAST(expression1 [, …n])",
        "Smallest of the values",
    ),
    f(
        "NULLIF",
        "NULLIF(expression, expression)",
        "NULL when the two are equal, else the first",
    ),
    // ── Mathematical ─────────────────────────────────────────────────────────
    f("ABS", "ABS(numeric_expression)", "Absolute value"),
    f("ACOS", "ACOS(float_expression)", "Arccosine, in radians"),
    f("ASIN", "ASIN(float_expression)", "Arcsine, in radians"),
    f("ATAN", "ATAN(float_expression)", "Arctangent, in radians"),
    f(
        "ATN2",
        "ATN2(float_expression, float_expression)",
        "Angle of the point (y, x), in radians",
    ),
    f(
        "CEILING",
        "CEILING(numeric_expression)",
        "Smallest integer not less than the value",
    ),
    f("COS", "COS(float_expression)", "Cosine"),
    f("COT", "COT(float_expression)", "Cotangent"),
    f(
        "DEGREES",
        "DEGREES(numeric_expression)",
        "Radians converted to degrees",
    ),
    f("EXP", "EXP(float_expression)", "e raised to the power"),
    f(
        "FLOOR",
        "FLOOR(numeric_expression)",
        "Largest integer not greater than the value",
    ),
    f(
        "LOG",
        "LOG(float_expression [, base])",
        "Natural logarithm, or to the base",
    ),
    f("LOG10", "LOG10(float_expression)", "Base-10 logarithm"),
    f("PI", "PI()", "The constant pi"),
    f(
        "POWER",
        "POWER(float_expression, y)",
        "The value raised to the power",
    ),
    f(
        "RADIANS",
        "RADIANS(numeric_expression)",
        "Degrees converted to radians",
    ),
    f("RAND", "RAND([seed])", "Random float from 0 to 1"),
    f(
        "ROUND",
        "ROUND(numeric_expression, length [, function])",
        "Value rounded, or truncated, to the length",
    ),
    f("SIGN", "SIGN(numeric_expression)", "-1, 0 or 1"),
    f("SIN", "SIN(float_expression)", "Sine"),
    f("SQRT", "SQRT(float_expression)", "Square root"),
    f("SQUARE", "SQUARE(float_expression)", "The value squared"),
    f("TAN", "TAN(float_expression)", "Tangent"),
    // ── Bit manipulation ─────────────────────────────────────────────────────
    f("BIT_COUNT", "BIT_COUNT(expression)", "Number of bits set"),
    f(
        "GET_BIT",
        "GET_BIT(expression, bit_offset)",
        "The bit at the offset",
    ),
    f(
        "LEFT_SHIFT",
        "LEFT_SHIFT(expression, shift_amount)",
        "Bits shifted left",
    ),
    f(
        "RIGHT_SHIFT",
        "RIGHT_SHIFT(expression, shift_amount)",
        "Bits shifted right",
    ),
    f(
        "SET_BIT",
        "SET_BIT(expression, bit_offset [, bit_value])",
        "The value with a bit set or cleared",
    ),
    // ── String ───────────────────────────────────────────────────────────────
    f(
        "ASCII",
        "ASCII(character_expression)",
        "Code of the first character",
    ),
    f("CHAR", "CHAR(integer_expression)", "Character for the code"),
    f(
        "CHARINDEX",
        "CHARINDEX(expressionToFind, expressionToSearch [, start_location])",
        "1-based position of the text, or 0",
    ),
    f(
        "CONCAT",
        "CONCAT(argument1, argument2 [, …n])",
        "Values joined, NULL as empty",
    ),
    f(
        "CONCAT_WS",
        "CONCAT_WS(separator, argument1, argument2 [, …n])",
        "Values joined by the separator, NULLs skipped",
    ),
    f(
        "DIFFERENCE",
        "DIFFERENCE(character_expression, character_expression)",
        "How alike two SOUNDEX codes are, 0 to 4",
    ),
    f(
        "FORMAT",
        "FORMAT(value, format [, culture])",
        "Value formatted by a .NET format string",
    ),
    f(
        "LEFT",
        "LEFT(character_expression, integer_expression)",
        "Leftmost characters",
    ),
    f(
        "LEN",
        "LEN(string_expression)",
        "Length in characters, trailing spaces not counted",
    ),
    f("LOWER", "LOWER(character_expression)", "Lower-cased text"),
    f(
        "LTRIM",
        "LTRIM(character_expression [, characters])",
        "Leading spaces, or characters, removed",
    ),
    f(
        "NCHAR",
        "NCHAR(integer_expression)",
        "Unicode character for the code",
    ),
    f(
        "PATINDEX",
        "PATINDEX('%pattern%', expression)",
        "1-based position of the LIKE pattern, or 0",
    ),
    f(
        "QUOTENAME",
        "QUOTENAME('character_string' [, 'quote_character'])",
        "Text quoted as an identifier",
    ),
    f(
        "REPLACE",
        "REPLACE(string_expression, string_pattern, string_replacement)",
        "Every occurrence replaced",
    ),
    f(
        "REPLICATE",
        "REPLICATE(string_expression, integer_expression)",
        "Text repeated n times",
    ),
    f("REVERSE", "REVERSE(string_expression)", "Text reversed"),
    f(
        "RIGHT",
        "RIGHT(character_expression, integer_expression)",
        "Rightmost characters",
    ),
    f(
        "RTRIM",
        "RTRIM(character_expression [, characters])",
        "Trailing spaces, or characters, removed",
    ),
    f(
        "SOUNDEX",
        "SOUNDEX(character_expression)",
        "Four-character code for how the text sounds",
    ),
    f("SPACE", "SPACE(integer_expression)", "n spaces"),
    f(
        "STR",
        "STR(float_expression [, length [, decimal]])",
        "Number as right-aligned text",
    ),
    f(
        "STRING_ESCAPE",
        "STRING_ESCAPE(text, type)",
        "Text escaped for JSON",
    ),
    f(
        "STRING_SPLIT",
        "STRING_SPLIT(string, separator [, enable_ordinal])",
        "Rows of the text split at the separator (in FROM)",
    ),
    f(
        "STUFF",
        "STUFF(character_expression, start, length, replace_with_expression)",
        "Text with a stretch replaced",
    ),
    f(
        "SUBSTRING",
        "SUBSTRING(expression, start, length)",
        "Part of the text",
    ),
    f(
        "TRANSLATE",
        "TRANSLATE(inputString, characters, translations)",
        "Characters mapped one for one",
    ),
    f(
        "TRIM",
        "TRIM([[LEADING | TRAILING | BOTH] [characters] FROM] string)",
        "Spaces, or characters, removed from the ends",
    ),
    f(
        "UNICODE",
        "UNICODE(ncharacter_expression)",
        "Code point of the first character",
    ),
    f("UPPER", "UPPER(character_expression)", "Upper-cased text"),
    // ── JSON ─────────────────────────────────────────────────────────────────
    f(
        "ISJSON",
        "ISJSON(expression [, json_type_constraint])",
        "1 when the text is valid JSON",
    ),
    f(
        "JSON_ARRAY",
        "JSON_ARRAY([value [, …n]] [NULL ON NULL | ABSENT ON NULL])",
        "JSON array of the values",
    ),
    f(
        "JSON_MODIFY",
        "JSON_MODIFY(expression, path, newValue)",
        "JSON with a value at the path set",
    ),
    f(
        "JSON_OBJECT",
        "JSON_OBJECT([key: value [, …n]] [NULL ON NULL | ABSENT ON NULL])",
        "JSON object of the pairs",
    ),
    f(
        "JSON_PATH_EXISTS",
        "JSON_PATH_EXISTS(value_expression, sql_json_path)",
        "1 when the path exists",
    ),
    f(
        "JSON_QUERY",
        "JSON_QUERY(expression [, path])",
        "Object or array at the path",
    ),
    f(
        "JSON_VALUE",
        "JSON_VALUE(expression, path)",
        "Scalar at the path, as text",
    ),
    f(
        "OPENJSON",
        "OPENJSON(jsonExpression [, path]) [WITH (…)]",
        "Rows of a JSON array or object (in FROM)",
    ),
    // ── System ───────────────────────────────────────────────────────────────
    f(
        "BINARY_CHECKSUM",
        "BINARY_CHECKSUM(* | expression [, …n])",
        "Checksum over the binary values",
    ),
    f(
        "CHECKSUM",
        "CHECKSUM(* | expression [, …n])",
        "Hash value over the values",
    ),
    f(
        "COMPRESS",
        "COMPRESS(expression)",
        "Value compressed with GZIP",
    ),
    f(
        "CONNECTIONPROPERTY",
        "CONNECTIONPROPERTY(property)",
        "A property of this connection",
    ),
    f(
        "CONTEXT_INFO",
        "CONTEXT_INFO()",
        "The session's context_info",
    ),
    f(
        "CURRENT_REQUEST_ID",
        "CURRENT_REQUEST_ID()",
        "This request's id",
    ),
    f(
        "CURRENT_TRANSACTION_ID",
        "CURRENT_TRANSACTION_ID()",
        "This transaction's id",
    ),
    f(
        "DECOMPRESS",
        "DECOMPRESS(expression)",
        "GZIP-compressed value expanded",
    ),
    f(
        "ERROR_LINE",
        "ERROR_LINE()",
        "Line of the error a CATCH block caught",
    ),
    f(
        "ERROR_MESSAGE",
        "ERROR_MESSAGE()",
        "Message of the error a CATCH block caught",
    ),
    f(
        "ERROR_NUMBER",
        "ERROR_NUMBER()",
        "Number of the error a CATCH block caught",
    ),
    f(
        "ERROR_PROCEDURE",
        "ERROR_PROCEDURE()",
        "Routine where the caught error happened",
    ),
    f(
        "ERROR_SEVERITY",
        "ERROR_SEVERITY()",
        "Severity of the error a CATCH block caught",
    ),
    f(
        "ERROR_STATE",
        "ERROR_STATE()",
        "State of the error a CATCH block caught",
    ),
    f(
        "FORMATMESSAGE",
        "FORMATMESSAGE({msg_number | 'msg_string'} [, param_value [, …n]])",
        "Message text with its parameters filled in",
    ),
    f(
        "GETANSINULL",
        "GETANSINULL(['database'])",
        "The database's default nullability",
    ),
    f(
        "HOST_ID",
        "HOST_ID()",
        "The client workstation's process id",
    ),
    f("HOST_NAME", "HOST_NAME()", "The client workstation's name"),
    f(
        "ISNULL",
        "ISNULL(check_expression, replacement_value)",
        "The replacement when the value is NULL",
    ),
    f(
        "ISNUMERIC",
        "ISNUMERIC(expression)",
        "1 when the value converts to a number",
    ),
    f(
        "MIN_ACTIVE_ROWVERSION",
        "MIN_ACTIVE_ROWVERSION()",
        "Lowest active rowversion in the database",
    ),
    f("NEWID", "NEWID()", "A new uniqueidentifier"),
    f(
        "NEWSEQUENTIALID",
        "NEWSEQUENTIALID()",
        "An increasing uniqueidentifier (column defaults only)",
    ),
    f(
        "ROWCOUNT_BIG",
        "ROWCOUNT_BIG()",
        "Rows the last statement affected (bigint)",
    ),
    f(
        "SESSION_CONTEXT",
        "SESSION_CONTEXT(N'key')",
        "The session's value for the key",
    ),
    f(
        "XACT_STATE",
        "XACT_STATE()",
        "1 committable, -1 doomed, 0 no transaction",
    ),
    f(
        "GET_FILESTREAM_TRANSACTION_CONTEXT",
        "GET_FILESTREAM_TRANSACTION_CONTEXT()",
        "The transaction's FILESTREAM context",
    ),
    f(
        "TRIGGER_NESTLEVEL",
        "TRIGGER_NESTLEVEL([object_id] [, 'trigger_type', 'trigger_event_category'])",
        "How deeply triggers are nested",
    ),
    f(
        "UPDATE",
        "UPDATE(column)",
        "In a trigger: whether the statement touched the column",
    ),
    f(
        "COLUMNS_UPDATED",
        "COLUMNS_UPDATED()",
        "In a trigger: bit mask of the columns touched",
    ),
    f(
        "EVENTDATA",
        "EVENTDATA()",
        "In a DDL trigger: the event as XML",
    ),
    // ── Metadata ─────────────────────────────────────────────────────────────
    f("APP_NAME", "APP_NAME()", "The client application's name"),
    f(
        "APPLOCK_MODE",
        "APPLOCK_MODE('database_principal', 'resource_name', 'lock_owner')",
        "Mode of an application lock held",
    ),
    f(
        "APPLOCK_TEST",
        "APPLOCK_TEST('database_principal', 'resource_name', 'lock_mode', 'lock_owner')",
        "1 when the application lock could be taken",
    ),
    f(
        "ASSEMBLYPROPERTY",
        "ASSEMBLYPROPERTY('assembly_name', 'property_name')",
        "A property of a CLR assembly",
    ),
    f(
        "COL_LENGTH",
        "COL_LENGTH('table', 'column')",
        "Column's defined length in bytes",
    ),
    f(
        "COL_NAME",
        "COL_NAME(table_id, column_id)",
        "Column name from its ids",
    ),
    f(
        "COLUMNPROPERTY",
        "COLUMNPROPERTY(id, column, property)",
        "A property of a column",
    ),
    f(
        "DATABASEPROPERTYEX",
        "DATABASEPROPERTYEX(database, property)",
        "A property of a database",
    ),
    f(
        "DATALENGTH",
        "DATALENGTH(expression)",
        "Length of the value in bytes",
    ),
    f("DB_ID", "DB_ID(['database_name'])", "Database id"),
    f("DB_NAME", "DB_NAME([database_id])", "Database name"),
    f(
        "FILE_ID",
        "FILE_ID(file_name)",
        "File id (deprecated for FILE_IDEX)",
    ),
    f(
        "FILE_IDEX",
        "FILE_IDEX(file_name)",
        "File id from its logical name",
    ),
    f("FILE_NAME", "FILE_NAME(file_id)", "Logical file name"),
    f(
        "FILEGROUP_ID",
        "FILEGROUP_ID('filegroup_name')",
        "Filegroup id",
    ),
    f(
        "FILEGROUP_NAME",
        "FILEGROUP_NAME(filegroup_id)",
        "Filegroup name",
    ),
    f(
        "FILEGROUPPROPERTY",
        "FILEGROUPPROPERTY(filegroup_name, property)",
        "A property of a filegroup",
    ),
    f(
        "FILEPROPERTY",
        "FILEPROPERTY(file_name, property)",
        "A property of a file",
    ),
    f(
        "FULLTEXTCATALOGPROPERTY",
        "FULLTEXTCATALOGPROPERTY('catalog_name', 'property')",
        "A property of a full-text catalogue",
    ),
    f(
        "FULLTEXTSERVICEPROPERTY",
        "FULLTEXTSERVICEPROPERTY('property')",
        "A property of the full-text service",
    ),
    f(
        "IDENT_CURRENT",
        "IDENT_CURRENT('table_or_view')",
        "Last identity value made for the table",
    ),
    f(
        "IDENT_INCR",
        "IDENT_INCR('table_or_view')",
        "The table's identity increment",
    ),
    f(
        "IDENT_SEED",
        "IDENT_SEED('table_or_view')",
        "The table's identity seed",
    ),
    f(
        "IDENTITY",
        "IDENTITY(data_type [, seed, increment])",
        "An identity column in SELECT … INTO",
    ),
    f(
        "INDEX_COL",
        "INDEX_COL('table_or_view', index_id, key_id)",
        "Name of an index's key column",
    ),
    f(
        "INDEXKEY_PROPERTY",
        "INDEXKEY_PROPERTY(object_id, index_id, key_id, property)",
        "A property of an index key column",
    ),
    f(
        "INDEXPROPERTY",
        "INDEXPROPERTY(object_id, index_or_statistics_name, property)",
        "A property of an index",
    ),
    f(
        "OBJECT_DEFINITION",
        "OBJECT_DEFINITION(object_id)",
        "The object's source text",
    ),
    f(
        "OBJECT_ID",
        "OBJECT_ID('object_name' [, 'object_type'])",
        "Object id",
    ),
    f(
        "OBJECT_NAME",
        "OBJECT_NAME(object_id [, database_id])",
        "Object name",
    ),
    f(
        "OBJECT_SCHEMA_NAME",
        "OBJECT_SCHEMA_NAME(object_id [, database_id])",
        "The object's schema name",
    ),
    f(
        "OBJECTPROPERTY",
        "OBJECTPROPERTY(id, property)",
        "A property of an object",
    ),
    f(
        "OBJECTPROPERTYEX",
        "OBJECTPROPERTYEX(id, property)",
        "A property of an object, as sql_variant",
    ),
    f(
        "ORIGINAL_DB_NAME",
        "ORIGINAL_DB_NAME()",
        "The database the connection string named",
    ),
    f(
        "PARSENAME",
        "PARSENAME('object_name', object_piece)",
        "One part of a dotted name",
    ),
    f("SCHEMA_ID", "SCHEMA_ID(['schema_name'])", "Schema id"),
    f("SCHEMA_NAME", "SCHEMA_NAME([schema_id])", "Schema name"),
    f(
        "SCOPE_IDENTITY",
        "SCOPE_IDENTITY()",
        "Last identity value made in this scope",
    ),
    f(
        "SERVERPROPERTY",
        "SERVERPROPERTY('propertyname')",
        "A property of the server",
    ),
    f(
        "STATS_DATE",
        "STATS_DATE(object_id, stats_id)",
        "When statistics were last updated",
    ),
    f("TYPE_ID", "TYPE_ID('type_name')", "Type id"),
    f("TYPE_NAME", "TYPE_NAME(type_id)", "Type name"),
    f(
        "TYPEPROPERTY",
        "TYPEPROPERTY(type, property)",
        "A property of a type",
    ),
    // ── Security ─────────────────────────────────────────────────────────────
    f(
        "CERTENCODED",
        "CERTENCODED(cert_id)",
        "A certificate's public part, encoded",
    ),
    f(
        "CERTPRIVATEKEY",
        "CERTPRIVATEKEY(cert_ID, 'encryption_password' [, 'decryption_password'])",
        "A certificate's private key, encoded",
    ),
    f(
        "CURRENT_USER",
        "CURRENT_USER",
        "The current database user, no parentheses",
    ),
    f(
        "HAS_DBACCESS",
        "HAS_DBACCESS('database_name')",
        "1 when this login can enter the database",
    ),
    f(
        "HAS_PERMS_BY_NAME",
        "HAS_PERMS_BY_NAME(securable, securable_class, permission [, sub-securable [, sub-securable_class]])",
        "1 when this login holds the permission",
    ),
    f(
        "IS_MEMBER",
        "IS_MEMBER({'group' | 'role'})",
        "1 when the user is a member",
    ),
    f(
        "IS_ROLEMEMBER",
        "IS_ROLEMEMBER('role' [, 'database_principal'])",
        "1 when the principal holds the database role",
    ),
    f(
        "IS_SRVROLEMEMBER",
        "IS_SRVROLEMEMBER('role' [, 'login'])",
        "1 when the login holds the server role",
    ),
    f(
        "LOGINPROPERTY",
        "LOGINPROPERTY('login_name', 'property_name')",
        "A property of a SQL login",
    ),
    f(
        "ORIGINAL_LOGIN",
        "ORIGINAL_LOGIN()",
        "The login that connected, before any impersonation",
    ),
    f(
        "PERMISSIONS",
        "PERMISSIONS([objectid [, 'column']])",
        "Bit mask of this user's permissions (deprecated)",
    ),
    f(
        "PWDCOMPARE",
        "PWDCOMPARE('clear_text_password', password_hash [, version])",
        "1 when the password matches the hash",
    ),
    f(
        "PWDENCRYPT",
        "PWDENCRYPT('password')",
        "The password's hash",
    ),
    f(
        "SESSION_USER",
        "SESSION_USER",
        "The session's database user, no parentheses",
    ),
    f(
        "SESSIONPROPERTY",
        "SESSIONPROPERTY(option)",
        "A SET option of this session",
    ),
    f(
        "SUSER_ID",
        "SUSER_ID(['login'])",
        "Login id (deprecated for SUSER_SID)",
    ),
    f(
        "SUSER_NAME",
        "SUSER_NAME([server_user_id])",
        "Login name from its id",
    ),
    f(
        "SUSER_SID",
        "SUSER_SID(['login' [, param2]])",
        "Login's security id",
    ),
    f(
        "SUSER_SNAME",
        "SUSER_SNAME([server_user_sid])",
        "Login name from its security id",
    ),
    f(
        "SYSTEM_USER",
        "SYSTEM_USER",
        "The current login, no parentheses",
    ),
    f("USER", "USER", "The current database user, no parentheses"),
    f(
        "USER_ID",
        "USER_ID(['user'])",
        "Database user id (deprecated for DATABASE_PRINCIPAL_ID)",
    ),
    f(
        "USER_NAME",
        "USER_NAME([id])",
        "Database user name from its id",
    ),
    f(
        "DATABASE_PRINCIPAL_ID",
        "DATABASE_PRINCIPAL_ID(['principal_name'])",
        "Database principal id",
    ),
    // ── Cryptographic ────────────────────────────────────────────────────────
    f(
        "ASYMKEY_ID",
        "ASYMKEY_ID('Asym_Key_Name')",
        "Asymmetric key id",
    ),
    f(
        "ASYMKEYPROPERTY",
        "ASYMKEYPROPERTY(Key_ID, 'algorithm_desc' | 'string_sid' | 'sid')",
        "A property of an asymmetric key",
    ),
    f("CERT_ID", "CERT_ID('cert_name')", "Certificate id"),
    f(
        "CERTPROPERTY",
        "CERTPROPERTY(Cert_ID, 'property')",
        "A property of a certificate",
    ),
    f(
        "DECRYPTBYASYMKEY",
        "DECRYPTBYASYMKEY(Asym_Key_ID, ciphertext [, 'Asym_Key_Password'])",
        "Data decrypted with an asymmetric key",
    ),
    f(
        "DECRYPTBYCERT",
        "DECRYPTBYCERT(certificate_ID, ciphertext [, pwd])",
        "Data decrypted with a certificate",
    ),
    f(
        "DECRYPTBYKEY",
        "DECRYPTBYKEY(ciphertext [, add_authenticator, authenticator])",
        "Data decrypted with the open symmetric key",
    ),
    f(
        "DECRYPTBYKEYAUTOASYMKEY",
        "DECRYPTBYKEYAUTOASYMKEY(akey_ID, akey_password, ciphertext [, add_authenticator, authenticator])",
        "Data decrypted with a symmetric key an asymmetric key protects",
    ),
    f(
        "DECRYPTBYKEYAUTOCERT",
        "DECRYPTBYKEYAUTOCERT(cert_ID, cert_password, ciphertext [, add_authenticator, authenticator])",
        "Data decrypted with a symmetric key a certificate protects",
    ),
    f(
        "DECRYPTBYPASSPHRASE",
        "DECRYPTBYPASSPHRASE(passphrase, ciphertext [, add_authenticator, authenticator])",
        "Data decrypted with a passphrase",
    ),
    f(
        "ENCRYPTBYASYMKEY",
        "ENCRYPTBYASYMKEY(Asym_Key_ID, cleartext)",
        "Data encrypted with an asymmetric key",
    ),
    f(
        "ENCRYPTBYCERT",
        "ENCRYPTBYCERT(certificate_ID, cleartext)",
        "Data encrypted with a certificate",
    ),
    f(
        "ENCRYPTBYKEY",
        "ENCRYPTBYKEY(key_GUID, cleartext [, add_authenticator, authenticator])",
        "Data encrypted with a symmetric key",
    ),
    f(
        "ENCRYPTBYPASSPHRASE",
        "ENCRYPTBYPASSPHRASE(passphrase, cleartext [, add_authenticator, authenticator])",
        "Data encrypted with a passphrase",
    ),
    f(
        "HASHBYTES",
        "HASHBYTES('algorithm', input)",
        "Hash of the input (SHA2_256, SHA2_512, …)",
    ),
    f(
        "IS_OBJECTSIGNED",
        "IS_OBJECTSIGNED('OBJECT', @object_name, @certOrAsymKey, @thumbprint)",
        "1 when the object is signed",
    ),
    f("KEY_GUID", "KEY_GUID('Key_Name')", "Symmetric key's GUID"),
    f("KEY_ID", "KEY_ID('Key_Name')", "Symmetric key id"),
    f(
        "KEY_NAME",
        "KEY_NAME(ciphertext | key_guid)",
        "Symmetric key name",
    ),
    f(
        "SIGNBYASYMKEY",
        "SIGNBYASYMKEY(Asym_Key_ID, @plaintext [, 'password'])",
        "Signature made with an asymmetric key",
    ),
    f(
        "SIGNBYCERT",
        "SIGNBYCERT(certificate_ID, @cleartext [, 'password'])",
        "Signature made with a certificate",
    ),
    f(
        "SYMKEYPROPERTY",
        "SYMKEYPROPERTY(Key_ID, 'algorithm_desc' | 'string_sid' | 'sid')",
        "A property of a symmetric key",
    ),
    f(
        "VERIFYSIGNEDBYASYMKEY",
        "VERIFYSIGNEDBYASYMKEY(Asym_Key_ID, clear_text, signature)",
        "1 when the asymmetric-key signature matches",
    ),
    f(
        "VERIFYSIGNEDBYCERT",
        "VERIFYSIGNEDBYCERT(Cert_ID, signed_data, signature)",
        "1 when the certificate signature matches",
    ),
    // ── Text and image ───────────────────────────────────────────────────────
    f(
        "TEXTPTR",
        "TEXTPTR(column)",
        "Pointer to a text, ntext or image value (deprecated)",
    ),
    f(
        "TEXTVALID",
        "TEXTVALID('table.column', text_ptr)",
        "1 when a text pointer is valid (deprecated)",
    ),
    // ── Rowset ───────────────────────────────────────────────────────────────
    f(
        "CONTAINSTABLE",
        "CONTAINSTABLE(table, {column | *}, 'contains_search_condition')",
        "Full-text matches, ranked (in FROM)",
    ),
    f(
        "FREETEXTTABLE",
        "FREETEXTTABLE(table, {column | *}, 'freetext_string')",
        "Full-text meaning matches, ranked (in FROM)",
    ),
    f(
        "GENERATE_SERIES",
        "GENERATE_SERIES(start, stop [, step])",
        "Rows of a number series (in FROM)",
    ),
    f(
        "OPENDATASOURCE",
        "OPENDATASOURCE(provider_name, init_string)",
        "An ad hoc remote data source (in FROM)",
    ),
    f(
        "OPENQUERY",
        "OPENQUERY(linked_server, 'query')",
        "A query run on a linked server (in FROM)",
    ),
    f(
        "OPENROWSET",
        "OPENROWSET(…)",
        "Rows from a remote source or a file (in FROM)",
    ),
    f(
        "OPENXML",
        "OPENXML(idoc, rowpattern [, flags]) [WITH (…)]",
        "Rows of an XML document (in FROM)",
    ),
    f(
        "PREDICT",
        "PREDICT(MODEL = model, DATA = source AS alias)",
        "Scores from a stored model (in FROM)",
    ),
];
