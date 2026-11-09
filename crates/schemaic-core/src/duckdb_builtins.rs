//! The DuckDB builtin function catalogue and reserved words, read off the
//! engine this build links.
//!
//! [`DUCKDB_FUNCTIONS`] is the fifth entry in `intel::builtin_catalog`. Like
//! PostgreSQL's it is **generated, not remembered** — DuckDB's function set is
//! large and every release adds to it — and unlike PostgreSQL's the generator
//! needs no server: the engine is compiled into `schemaic-db` (the workspace
//! pins its version exactly), so the catalogue is the linked engine's and
//! nothing else's. The query, one row per name, the overload with the fewest
//! parameters:
//!
//! ```sql
//! SELECT function_name,
//!        any_value(function_name || '(' || array_to_string(parameters, ', ') || ')'
//!                  ORDER BY len(parameters), parameters),
//!        coalesce(any_value(description ORDER BY description IS NULL, len(parameters)), '')
//!   FROM duckdb_functions()
//!  WHERE internal AND schema_name IN ('main', 'pg_catalog')
//!    AND function_type IN ('scalar', 'aggregate', 'table', 'macro', 'table_macro')
//!    AND regexp_full_match(function_name, '[a-z][a-z0-9_]*')
//!  GROUP BY function_name ORDER BY function_name;
//! ```
//!
//! **Asked under the app's own configuration** — extension auto-load off, as
//! `db::duckdb` opens every file. A default DuckDB loads an extension the
//! machine has installed (ICU is common) the first time a query needs it, and
//! a catalogue generated that way held 291 functions no Schemaic session can
//! call.
//!
//! The query is followed by the two rules PostgreSQL's catalogue follows: a **signature** longer
//! than 72 bytes is written `name(…)`, and an empty **summary** is the
//! literal ``No description in `duckdb_functions()` ``, a description's first
//! letter upper-cased otherwise.
//!
//! Four rows are added to what the query returns: `current_time`,
//! `current_timestamp`, `localtime` and `localtimestamp`, which DuckDB's parser
//! reads as keywords and rewrites into a call, so the function list never
//! names them — and they are what `intel`'s bare-called builtins offer.
//!
//! [`DUCKDB_RESERVED`] is `duckdb_keywords()`'s `reserved` category,
//! upper-cased: the words DuckDB refuses as a bare name.
//!
//! `db::duckdb`'s `the_builtin_catalogue_is_the_linked_engines` rebuilds both
//! from the engine and compares, so a version bump that changes either fails a
//! test rather than leaving the editor squiggling a function the engine has.

use crate::intel::{SqlFunction, f};

/// DuckDB's builtins, lower-case, in name order.
pub const DUCKDB_FUNCTIONS: &[SqlFunction] = &[
    f("abs", "abs(x)", "Absolute value"),
    f("acos", "acos(x)", "Computes the arccosine of x"),
    f(
        "acosh",
        "acosh(x)",
        "Computes the inverse hyperbolic cos of x",
    ),
    f("add", "add()", "No description in `duckdb_functions()`"),
    f(
        "age",
        "age(col0)",
        "Subtract arguments, resulting in the time difference between the two timestamps",
    ),
    f(
        "aggregate",
        "aggregate(list, function_name)",
        "Executes the aggregate function `function_name` on the elements of `list`.",
    ),
    f("ago", "ago(i)", "No description in `duckdb_functions()`"),
    f(
        "alias",
        "alias(expr)",
        "Returns the name of a given expression",
    ),
    f(
        "any_value",
        "any_value(arg)",
        "Returns the first non-NULL value from arg. This function is affected by ordering.",
    ),
    f(
        "apply",
        "apply(list, lambda)",
        "Returns a list that is the result of applying the `lambda` function to each element of the input `list`. The return type is defined by the return type of the `lambda` function.",
    ),
    f(
        "approx_count_distinct",
        "approx_count_distinct(any)",
        "Computes the approximate count of distinct elements using HyperLogLog.",
    ),
    f(
        "approx_quantile",
        "approx_quantile(x, pos)",
        "Computes the approximate quantile using T-Digest.",
    ),
    f(
        "approx_top_k",
        "approx_top_k(val, k)",
        "Finds the k approximately most occurring values in the data set",
    ),
    f(
        "arbitrary",
        "arbitrary(arg)",
        "Returns the first value (NULL or non-NULL) from arg. This function is affected by ordering.",
    ),
    f(
        "arg_max",
        "arg_max(arg, val)",
        "Finds the row with the maximum val. Calculates the non-NULL arg expression at that row.",
    ),
    f(
        "arg_max_null",
        "arg_max_null(arg, val)",
        "Finds the row with the maximum val. Calculates the arg expression at that row.",
    ),
    f(
        "arg_max_nulls_last",
        "arg_max_nulls_last(arg, val)",
        "Finds the rows with N maximum vals, including nulls. Calculates the arg expression at that row.",
    ),
    f(
        "arg_min",
        "arg_min(arg, val)",
        "Finds the row with the minimum val. Calculates the non-NULL arg expression at that row.",
    ),
    f(
        "arg_min_null",
        "arg_min_null(arg, val)",
        "Finds the row with the minimum val. Calculates the arg expression at that row.",
    ),
    f(
        "arg_min_nulls_last",
        "arg_min_nulls_last(arg, val)",
        "Finds the rows with N minimum vals, including nulls. Calculates the arg expression at that row.",
    ),
    f(
        "argmax",
        "argmax(arg, val)",
        "Finds the row with the maximum val. Calculates the non-NULL arg expression at that row.",
    ),
    f(
        "argmin",
        "argmin(arg, val)",
        "Finds the row with the minimum val. Calculates the non-NULL arg expression at that row.",
    ),
    f(
        "array_agg",
        "array_agg(arg)",
        "Returns a LIST containing all the values of a column.",
    ),
    f(
        "array_aggr",
        "array_aggr(list, function_name)",
        "Executes the aggregate function `function_name` on the elements of `list`.",
    ),
    f(
        "array_aggregate",
        "array_aggregate(list, function_name)",
        "Executes the aggregate function `function_name` on the elements of `list`.",
    ),
    f(
        "array_append",
        "array_append(arr, el)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "array_apply",
        "array_apply(list, lambda)",
        "Returns a list that is the result of applying the `lambda` function to each element of the input `list`. The return type is defined by the return type of the `lambda` function.",
    ),
    f(
        "array_cat",
        "array_cat()",
        "Concatenates lists. `NULL` inputs are skipped. See also operator `||`.",
    ),
    f(
        "array_concat",
        "array_concat()",
        "Concatenates lists. `NULL` inputs are skipped. See also operator `||`.",
    ),
    f(
        "array_contains",
        "array_contains(list, element)",
        "Returns true if the list contains the element.",
    ),
    f(
        "array_cosine_distance",
        "array_cosine_distance(array1, array2)",
        "Computes the cosine distance between two arrays of the same size. The array elements can not be `NULL`. The arrays can have any size as long as the size is the same for both arguments.",
    ),
    f(
        "array_cosine_similarity",
        "array_cosine_similarity(array1, array2)",
        "Computes the cosine similarity between two arrays of the same size. The array elements can not be `NULL`. The arrays can have any size as long as the size is the same for both arguments.",
    ),
    f(
        "array_cross_product",
        "array_cross_product(array1, array2)",
        "Computes the cross product of two arrays of size 3. The array elements can not be `NULL`.",
    ),
    f(
        "array_distance",
        "array_distance(array1, array2)",
        "Computes the distance between two arrays of the same size. The array elements can not be `NULL`. The arrays can have any size as long as the size is the same for both arguments.",
    ),
    f(
        "array_distinct",
        "array_distinct(list)",
        "Removes all duplicates and `NULL` values from a list. Does not preserve the original order.",
    ),
    f(
        "array_dot_product",
        "array_dot_product(array1, array2)",
        "Computes the inner product between two arrays of the same size. The array elements can not be `NULL`. The arrays can have any size as long as the size is the same for both arguments.",
    ),
    f(
        "array_extract",
        "array_extract(list, index)",
        "Extracts a single character from a `string` using a (1-based) `index`.",
    ),
    f(
        "array_filter",
        "array_filter(list, lambda)",
        "Constructs a list from those elements of the input `list` for which the `lambda` function returns `true`. DuckDB must be able to cast the `lambda` function's return type to `BOOL`. The return type of `list_filter` is the same as the input list's.",
    ),
    f(
        "array_grade_up",
        "array_grade_up(list)",
        "Works like list_sort, but the results are the indexes that correspond to the position in the original list instead of the actual values.",
    ),
    f(
        "array_has",
        "array_has(list, element)",
        "Returns true if the list contains the element.",
    ),
    f(
        "array_has_all",
        "array_has_all(list1, list2)",
        "Returns true if all elements of list2 are in list1. NULLs are ignored.",
    ),
    f(
        "array_has_any",
        "array_has_any(list1, list2)",
        "Returns true if the lists have any element in common. NULLs are ignored.",
    ),
    f(
        "array_indexof",
        "array_indexof(list, element)",
        "Returns the index of the `element` if the `list` contains the `element`. If the `element` is not found, it returns `NULL`.",
    ),
    f(
        "array_inner_product",
        "array_inner_product(array1, array2)",
        "Computes the inner product between two arrays of the same size. The array elements can not be `NULL`. The arrays can have any size as long as the size is the same for both arguments.",
    ),
    f(
        "array_intersect",
        "array_intersect(list1, list2)",
        "Returns a list containing the distinct elements that are present in both `list1` and `list2`.",
    ),
    f(
        "array_length",
        "array_length(list)",
        "Returns the length of the `list`.",
    ),
    f(
        "array_negative_dot_product",
        "array_negative_dot_product(array1, array2)",
        "Computes the negative inner product between two arrays of the same size. The array elements can not be `NULL`. The arrays can have any size as long as the size is the same for both arguments.",
    ),
    f(
        "array_negative_inner_product",
        "array_negative_inner_product(array1, array2)",
        "Computes the negative inner product between two arrays of the same size. The array elements can not be `NULL`. The arrays can have any size as long as the size is the same for both arguments.",
    ),
    f(
        "array_pop_back",
        "array_pop_back(arr)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "array_pop_front",
        "array_pop_front(arr)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "array_position",
        "array_position(list, element)",
        "Returns the index of the `element` if the `list` contains the `element`. If the `element` is not found, it returns `NULL`.",
    ),
    f(
        "array_prepend",
        "array_prepend(el, arr)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "array_push_back",
        "array_push_back(arr, e)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "array_push_front",
        "array_push_front(arr, e)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "array_reduce",
        "array_reduce(list, lambda)",
        "Reduces all elements of the input `list` into a single scalar value by executing the `lambda` function on a running result and the next list element. The `lambda` function has an optional `initial_value` argument.",
    ),
    f(
        "array_resize",
        "array_resize(list, size)",
        "Resizes the `list` to contain `size` elements. Initializes new elements with `value` or `NULL` if `value` is not set.",
    ),
    f(
        "array_reverse",
        "array_reverse(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "array_reverse_sort",
        "array_reverse_sort(list)",
        "Sorts the elements of the list in reverse order.",
    ),
    f(
        "array_select",
        "array_select(value_list, index_list)",
        "Returns a list based on the elements selected by the `index_list`.",
    ),
    f(
        "array_slice",
        "array_slice(list, begin, end)",
        "Extracts a sublist or substring using slice conventions. Negative values are accepted.",
    ),
    f(
        "array_sort",
        "array_sort(list)",
        "Sorts the elements of the list.",
    ),
    f(
        "array_to_json",
        "array_to_json()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "array_to_string",
        "array_to_string(arr, sep)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "array_to_string_comma_default",
        "array_to_string_comma_default(arr, sep)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "array_transform",
        "array_transform(list, lambda)",
        "Returns a list that is the result of applying the `lambda` function to each element of the input `list`. The return type is defined by the return type of the `lambda` function.",
    ),
    f(
        "array_unique",
        "array_unique(list)",
        "Counts the unique elements of a `list`.",
    ),
    f(
        "array_value",
        "array_value()",
        "Creates an `ARRAY` containing the argument values.",
    ),
    f(
        "array_where",
        "array_where(value_list, mask_list)",
        "Returns a list with the `BOOLEAN`s in `mask_list` applied as a mask to the `value_list`.",
    ),
    f(
        "array_zip",
        "array_zip()",
        "Zips n `LIST`s to a new `LIST` whose length will be that of the longest list. Its elements are structs of n elements from each list `list_1`, …, `list_n`, missing elements are replaced with `NULL`. If `truncate` is set, all lists are truncated to the smallest list length.",
    ),
    f(
        "arrow_scan",
        "arrow_scan()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "arrow_scan_dumb",
        "arrow_scan_dumb()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "ascii",
        "ascii(string)",
        "Returns an integer that represents the Unicode code point of the first character of the `string`.",
    ),
    f("asin", "asin(x)", "Computes the arcsine of x"),
    f(
        "asinh",
        "asinh(x)",
        "Computes the inverse hyperbolic sin of x",
    ),
    f(
        "assert_true",
        "assert_true(condition)",
        "No description in `duckdb_functions()`",
    ),
    f("atan", "atan(x)", "Computes the arctangent of x"),
    f("atan2", "atan2(y, x)", "Computes the arctangent (y, x)"),
    f(
        "atanh",
        "atanh(x)",
        "Computes the inverse hyperbolic tan of x",
    ),
    f(
        "avg",
        "avg(x)",
        "Calculates the average value for all tuples in x.",
    ),
    f(
        "bar",
        "bar(x, min, max)",
        "Draws a band whose width is proportional to (`x - min`) and equal to `width` characters when `x` = `max`. `width` defaults to 80.",
    ),
    f(
        "base64",
        "base64(blob)",
        "Converts a `blob` to a base64 encoded string.",
    ),
    f(
        "bin",
        "bin(string)",
        "Converts the `string` to binary representation.",
    ),
    f("binom", "binom(n, k)", "Returns binom(n,k)"),
    f(
        "bit_and",
        "bit_and(arg)",
        "Returns the bitwise AND of all bits in a given expression.",
    ),
    f(
        "bit_count",
        "bit_count(x)",
        "Returns the number of bits that are set",
    ),
    f(
        "bit_length",
        "bit_length(bit)",
        "Number of bits in a `string`.",
    ),
    f(
        "bit_or",
        "bit_or(arg)",
        "Returns the bitwise OR of all bits in a given expression.",
    ),
    f(
        "bit_position",
        "bit_position(substring, bitstring)",
        "Returns first starting index of the specified substring within bits, or zero if it is not present. The first (leftmost) bit is indexed 1",
    ),
    f(
        "bit_xor",
        "bit_xor(arg)",
        "Returns the bitwise XOR of all bits in a given expression.",
    ),
    f(
        "bitstring",
        "bitstring(bitstring, length)",
        "Pads the bitstring until the specified length",
    ),
    f(
        "bitstring_agg",
        "bitstring_agg(arg)",
        "Returns a bitstring with bits set for each distinct value.",
    ),
    f(
        "bitstring_byte_comparable",
        "bitstring_byte_comparable(bitstring)",
        "Converts a BIT to a binary comparable sort key",
    ),
    f(
        "bool_and",
        "bool_and(arg)",
        "Returns TRUE if every input value is TRUE, otherwise FALSE.",
    ),
    f(
        "bool_or",
        "bool_or(arg)",
        "Returns TRUE if any input value is TRUE, otherwise FALSE.",
    ),
    f(
        "can_cast_implicitly",
        "can_cast_implicitly(source_type, target_type)",
        "Whether or not we can implicitly cast from the source type to the other type",
    ),
    f(
        "cardinality",
        "cardinality(map)",
        "Returns the size of the map (or the number of entries in the map)",
    ),
    f(
        "cast_to_type",
        "cast_to_type(param, type)",
        "Casts the first argument to the type of the second argument",
    ),
    f("cbrt", "cbrt(x)", "Returns the cube root of x"),
    f("ceil", "ceil(x)", "Rounds the number up"),
    f("ceiling", "ceiling(x)", "Rounds the number up"),
    f(
        "century",
        "century(col0)",
        "Extract the century component from a date or timestamp",
    ),
    f(
        "char_length",
        "char_length(bit)",
        "Number of characters in `string`.",
    ),
    f(
        "character_length",
        "character_length(bit)",
        "Number of characters in `string`.",
    ),
    f(
        "checkpoint",
        "checkpoint()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "chr",
        "chr(code_point)",
        "Returns a character which is corresponding the ASCII code value or Unicode code point.",
    ),
    f(
        "col_description",
        "col_description(table_oid, column_number)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "combine",
        "combine(state1, state2)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "combine_aggr",
        "combine_aggr(arg)",
        "Combines exported aggregate states",
    ),
    f(
        "concat",
        "concat(value)",
        "Concatenates multiple strings or lists. `NULL` inputs are skipped. See also operator `||`.",
    ),
    f(
        "concat_ws",
        "concat_ws(separator, string)",
        "Concatenates many strings, separated by `separator`. `NULL` inputs are skipped.",
    ),
    f(
        "constant_or_null",
        "constant_or_null(arg1, arg2)",
        "If arg2 is NULL, return NULL. Otherwise, return arg1.",
    ),
    f(
        "contains",
        "contains(list, element)",
        "Returns `true` if `search_string` is found within `string`.",
    ),
    f(
        "corr",
        "corr(y, x)",
        "Returns the correlation coefficient for non-NULL pairs in a group.",
    ),
    f("cos", "cos(x)", "Computes the cos of x"),
    f("cosh", "cosh(x)", "Computes the hyperbolic cos of x"),
    f("cot", "cot(x)", "Computes the cotangent of x"),
    f(
        "count",
        "count()",
        "Returns the number of non-NULL values in arg.",
    ),
    f(
        "count_if",
        "count_if(arg)",
        "Counts the total number of TRUE values for a boolean column",
    ),
    f(
        "count_star",
        "count_star()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "countif",
        "countif(arg)",
        "Counts the total number of TRUE values for a boolean column",
    ),
    f(
        "covar_pop",
        "covar_pop(y, x)",
        "Returns the population covariance of input values.",
    ),
    f(
        "covar_samp",
        "covar_samp(y, x)",
        "Returns the sample covariance for non-NULL pairs in a group.",
    ),
    f(
        "create_external_resource",
        "create_external_resource(…)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "create_sort_key",
        "create_sort_key(key1)",
        "Constructs a binary-comparable sort key based on a set of input parameters and sort qualifiers",
    ),
    f(
        "current_catalog",
        "current_catalog()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "current_connection_id",
        "current_connection_id()",
        "Get the current connection_id",
    ),
    f(
        "current_database",
        "current_database()",
        "Returns the name of the currently active database",
    ),
    f(
        "current_query",
        "current_query()",
        "Returns the current query as a string",
    ),
    f(
        "current_query_id",
        "current_query_id()",
        "Get the current query_id",
    ),
    f(
        "current_role",
        "current_role()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "current_schema",
        "current_schema()",
        "Returns the name of the currently active schema. Default is main",
    ),
    f(
        "current_schemas",
        "current_schemas(include_implicit)",
        "Returns list of schemas. Pass a parameter of True to include implicit schemas",
    ),
    f(
        "current_setting",
        "current_setting(setting_name)",
        "Returns the current value of the configuration setting",
    ),
    f(
        "current_time",
        "current_time",
        "The current time, as of the start of the transaction",
    ),
    f(
        "current_timestamp",
        "current_timestamp",
        "The current date and time, as of the start of the transaction",
    ),
    f(
        "current_transaction_id",
        "current_transaction_id()",
        "Get the current global transaction_id",
    ),
    f(
        "current_user",
        "current_user()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "currval",
        "currval(sequence_name)",
        "Return the current value of the sequence. Note that nextval must be called at least once prior to calling currval.",
    ),
    f(
        "damerau_levenshtein",
        "damerau_levenshtein(s1, s2)",
        "Extension of Levenshtein distance to also include transposition of adjacent characters as an allowed edit operation. In other words, the minimum number of edit operations (insertions, deletions, substitutions or transpositions) required to change one string to another. Characters of different cases (e.g., `a` and `A`) are considered different.",
    ),
    f(
        "date_add",
        "date_add(date, interval)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "date_diff",
        "date_diff(col0, col1, col2)",
        "The number of partition boundaries between the timestamps",
    ),
    f(
        "date_part",
        "date_part(col0, col1)",
        "Get subfield (equivalent to extract)",
    ),
    f(
        "date_sub",
        "date_sub(col0, col1, col2)",
        "The number of complete partitions between the timestamps",
    ),
    f(
        "date_trunc",
        "date_trunc(col0, col1)",
        "Truncate to specified precision",
    ),
    f(
        "datediff",
        "datediff(col0, col1, col2)",
        "The number of partition boundaries between the timestamps",
    ),
    f(
        "datepart",
        "datepart(col0, col1)",
        "Get subfield (equivalent to extract)",
    ),
    f(
        "datesub",
        "datesub(col0, col1, col2)",
        "The number of complete partitions between the timestamps",
    ),
    f(
        "datetrunc",
        "datetrunc(col0, col1)",
        "Truncate to specified precision",
    ),
    f(
        "day",
        "day(col0)",
        "Extract the day component from a date or timestamp",
    ),
    f(
        "dayname",
        "dayname(col0)",
        "The (English) name of the weekday",
    ),
    f(
        "dayofmonth",
        "dayofmonth(col0)",
        "Extract the dayofmonth component from a date or timestamp",
    ),
    f(
        "dayofweek",
        "dayofweek(col0)",
        "Extract the dayofweek component from a date or timestamp",
    ),
    f(
        "dayofyear",
        "dayofyear(col0)",
        "Extract the dayofyear component from a date or timestamp",
    ),
    f(
        "days_in_month",
        "days_in_month(date)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "decade",
        "decade(col0)",
        "Extract the decade component from a date or timestamp",
    ),
    f(
        "decimal_average",
        "decimal_average(arg)",
        "Computes the average of a DECIMAL column using exact integer arithmetic with banker's rounding. Returns a DECIMAL with the same width and scale as the input.",
    ),
    f(
        "decimal_division",
        "decimal_division(x, y)",
        "Divides two DECIMAL values using exact integer arithmetic and round-half-to-even, returning a DECIMAL result with scale determined by SQL Server semantics (result_scale = max(6, s1 + p2 + 1)).",
    ),
    f(
        "decode",
        "decode(blob)",
        "Converts `blob` to `VARCHAR`. Invalid UTF-8 is handled based on the error behavior argument. Can be 'strict' (default, fail), 'replace' to replace invalid characters with '?', or 'ignore' to skip invalid characters.",
    ),
    f("degrees", "degrees(x)", "Converts radians to degrees"),
    f(
        "deregister_external_resource",
        "deregister_external_resource(name)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "destroy_external_resource",
        "destroy_external_resource(deleter_function, payload)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "disable_logging",
        "disable_logging()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "disable_profiling",
        "disable_profiling()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "divide",
        "divide(left, right)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_approx_database_count",
        "duckdb_approx_database_count()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_available_metrics",
        "duckdb_available_metrics()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_columns",
        "duckdb_columns()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_connection_count",
        "duckdb_connection_count()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_constraints",
        "duckdb_constraints()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_coordinate_systems",
        "duckdb_coordinate_systems()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_databases",
        "duckdb_databases()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_dependencies",
        "duckdb_dependencies()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_dialects",
        "duckdb_dialects()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_eviction_queues",
        "duckdb_eviction_queues()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_extension_repositories",
        "duckdb_extension_repositories()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_extensions",
        "duckdb_extensions()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_external_file_cache",
        "duckdb_external_file_cache()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_external_resource_types",
        "duckdb_external_resource_types()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_external_resources",
        "duckdb_external_resources(discover)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_functions",
        "duckdb_functions()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_grammar_extensions",
        "duckdb_grammar_extensions()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_indexes",
        "duckdb_indexes()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_keywords",
        "duckdb_keywords()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_log_contexts",
        "duckdb_log_contexts()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_logs",
        "duckdb_logs(denormalized_table)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_logs_parsed",
        "duckdb_logs_parsed(log_type)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_memory",
        "duckdb_memory()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_optimizers",
        "duckdb_optimizers()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_prepared_statements",
        "duckdb_prepared_statements()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_profiling_settings",
        "duckdb_profiling_settings()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_schemas",
        "duckdb_schemas()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_secret_type_parameters",
        "duckdb_secret_type_parameters()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_secret_types",
        "duckdb_secret_types()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_secrets",
        "duckdb_secrets(redact)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_sequences",
        "duckdb_sequences()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_settings",
        "duckdb_settings(debug, deprecated, include_all)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_table_sample",
        "duckdb_table_sample(table_name)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_tables",
        "duckdb_tables()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_temporary_files",
        "duckdb_temporary_files()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_triggers",
        "duckdb_triggers()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_types",
        "duckdb_types()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_variables",
        "duckdb_variables()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "duckdb_views",
        "duckdb_views()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "editdist3",
        "editdist3(s1, s2)",
        "The minimum number of single-character edits (insertions, deletions or substitutions) required to change one string to the other. Characters of different cases (e.g., `a` and `A`) are considered different.",
    ),
    f(
        "element_at",
        "element_at(map, key)",
        "Returns a list containing the value for a given key or an empty list if the key is not contained in the map. The type of the key provided in the second parameter must match the type of the map’s keys else an error is returned",
    ),
    f(
        "enable_logging",
        "enable_logging(…)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "enable_profiling",
        "enable_profiling(metrics, format, coverage, save_location, mode)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "encode",
        "encode(string)",
        "Converts the `string` to `BLOB`. Converts UTF-8 characters into literal encoding.",
    ),
    f(
        "ends_with",
        "ends_with(string, search_string)",
        "Returns `true` if `string` ends with `search_string`.",
    ),
    f(
        "entropy",
        "entropy(x)",
        "Returns the log-2 entropy of count input-values.",
    ),
    f(
        "enum_code",
        "enum_code(enum)",
        "Returns the numeric value backing the given enum value",
    ),
    f(
        "enum_first",
        "enum_first(enum)",
        "Returns the first value of the input enum type",
    ),
    f(
        "enum_last",
        "enum_last(enum)",
        "Returns the last value of the input enum type",
    ),
    f(
        "enum_range",
        "enum_range(enum)",
        "Returns all values of the input enum type as an array",
    ),
    f(
        "enum_range_boundary",
        "enum_range_boundary(start, end)",
        "Returns the range between the two given enum values as an array. The values must be of the same enum type. When the first parameter is NULL, the result starts with the first value of the enum type. When the second parameter is NULL, the result ends with the last value of the enum type",
    ),
    f(
        "epoch",
        "epoch(col0)",
        "Extract the epoch component from a temporal type",
    ),
    f(
        "epoch_ms",
        "epoch_ms(temporal)",
        "Extract the epoch component in milliseconds from a temporal type",
    ),
    f(
        "epoch_ns",
        "epoch_ns(temporal)",
        "Extract the epoch component in nanoseconds from a temporal type",
    ),
    f(
        "epoch_us",
        "epoch_us(temporal)",
        "Extract the epoch component in microseconds from a temporal type",
    ),
    f(
        "equi_width_bins",
        "equi_width_bins(min, max, bin_count, nice_rounding)",
        "Generates bin_count equi-width bins between the min and max. If enabled nice_rounding makes the numbers more readable/less jagged",
    ),
    f(
        "era",
        "era(col0)",
        "Extract the era component from a date or timestamp",
    ),
    f("error", "error(message)", "Throws the given error message"),
    f(
        "even",
        "even(x)",
        "Rounds x to next even number by rounding away from zero",
    ),
    f("exp", "exp(x)", "Computes e to the power of x"),
    f(
        "factorial",
        "factorial(x)",
        "Factorial of x. Computes the product of the current integer and all integers below it",
    ),
    f(
        "favg",
        "favg(x)",
        "Calculates the average using a more accurate floating point summation (Kahan Sum)",
    ),
    f(
        "fdiv",
        "fdiv(x, y)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "filter",
        "filter(list, lambda)",
        "Constructs a list from those elements of the input `list` for which the `lambda` function returns `true`. DuckDB must be able to cast the `lambda` function's return type to `BOOL`. The return type of `list_filter` is the same as the input list's.",
    ),
    f(
        "finalize",
        "finalize(state)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "first",
        "first(arg)",
        "Returns the first value (NULL or non-NULL) from arg. This function is affected by ordering.",
    ),
    f(
        "flatten",
        "flatten(nested_list)",
        "Flattens a nested list by one level.",
    ),
    f("floor", "floor(x)", "Rounds the number down"),
    f(
        "fmod",
        "fmod(x, y)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "force_checkpoint",
        "force_checkpoint()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "format",
        "format(format)",
        "Formats a string using the fmt syntax.",
    ),
    f(
        "format_bytes",
        "format_bytes(integer)",
        "Converts `integer` to a human-readable representation using units based on powers of 2 (KiB, MiB, GiB, etc.).",
    ),
    f(
        "format_pg_type",
        "format_pg_type(logical_type, type_name)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "format_type",
        "format_type(type_oid, typemod)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "from_base64",
        "from_base64(string)",
        "Converts a base64 encoded `string` to a character string (`BLOB`).",
    ),
    f(
        "from_binary",
        "from_binary(value)",
        "Converts a `value` from binary representation to a blob.",
    ),
    f(
        "from_hex",
        "from_hex(value)",
        "Converts a `value` from hexadecimal representation to a blob.",
    ),
    f(
        "from_json",
        "from_json(json, structure)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "from_json_strict",
        "from_json_strict(json, structure)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "fsum",
        "fsum(arg)",
        "Calculates the sum using a more accurate floating point summation (Kahan Sum).",
    ),
    f(
        "gamma",
        "gamma(x)",
        "Interpolation of (x-1) factorial (so decimal inputs are allowed)",
    ),
    f(
        "gcd",
        "gcd(x, y)",
        "Computes the greatest common divisor of x and y",
    ),
    f(
        "gen_random_uuid",
        "gen_random_uuid()",
        "Returns a random UUID v4 similar to this: eeccb8c5-9943-b2bb-bb5e-222f4e14b687",
    ),
    f(
        "generate_series",
        "generate_series(stop)",
        "Creates a list of values between `start` and `stop` - the stop parameter is inclusive.",
    ),
    f(
        "generate_subscripts",
        "generate_subscripts(arr, dim)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "geomean",
        "geomean(x)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "geometric_mean",
        "geometric_mean(x)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "get_bit",
        "get_bit(bitstring, index)",
        "Extracts the nth bit from bitstring; the first (leftmost) bit is indexed 0",
    ),
    f(
        "get_block_size",
        "get_block_size(db_name)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "get_current_timestamp",
        "get_current_timestamp()",
        "Returns the current timestamp",
    ),
    f(
        "get_type",
        "get_type(expression)",
        "Returns the type of the result of the expression",
    ),
    f(
        "getvariable",
        "getvariable(variable_name)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "glob",
        "glob(pattern)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "grade_up",
        "grade_up(list)",
        "Works like list_sort, but the results are the indexes that correspond to the position in the original list instead of the actual values.",
    ),
    f(
        "greatest",
        "greatest(arg1)",
        "Returns the largest value. For strings lexicographical ordering is used. Note that lowercase characters are considered “larger” than uppercase characters and collations are not supported.",
    ),
    f(
        "greatest_common_divisor",
        "greatest_common_divisor(x, y)",
        "Computes the greatest common divisor of x and y",
    ),
    f(
        "group_concat",
        "group_concat(str)",
        "Concatenates the column string values with an optional separator.",
    ),
    f(
        "hamming",
        "hamming(s1, s2)",
        "The Hamming distance between to strings, i.e., the number of positions with different characters for two strings of equal length. Strings must be of equal length. Characters of different cases (e.g., `a` and `A`) are considered different.",
    ),
    f(
        "has_any_column_privilege",
        "has_any_column_privilege(table, privilege)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "has_column_privilege",
        "has_column_privilege(table, column, privilege)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "has_database_privilege",
        "has_database_privilege(database, privilege)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "has_foreign_data_wrapper_privilege",
        "has_foreign_data_wrapper_privilege(fdw, privilege)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "has_function_privilege",
        "has_function_privilege(function, privilege)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "has_language_privilege",
        "has_language_privilege(language, privilege)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "has_schema_privilege",
        "has_schema_privilege(schema, privilege)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "has_sequence_privilege",
        "has_sequence_privilege(sequence, privilege)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "has_server_privilege",
        "has_server_privilege(server, privilege)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "has_table_privilege",
        "has_table_privilege(table, privilege)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "has_tablespace_privilege",
        "has_tablespace_privilege(tablespace, privilege)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "hash",
        "hash(value)",
        "Returns a `UBIGINT` with the hash of the `value`. Note that this is not a cryptographic hash.",
    ),
    f(
        "hex",
        "hex(blob)",
        "Converts the `string` to hexadecimal representation.",
    ),
    f(
        "histogram",
        "histogram(arg)",
        "Returns a LIST of STRUCTs with the fields bucket and count.",
    ),
    f(
        "histogram_exact",
        "histogram_exact(arg, bins)",
        "Returns a LIST of STRUCTs with the fields bucket and count matching the buckets exactly.",
    ),
    f(
        "histogram_values",
        "histogram_values(source, col_name, bin_count, technique)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "hive_partition_component",
        "hive_partition_component(name, value)",
        "Returns the hive partition directory for a partition column and its value, e.g. name=value. The name and the value are escaped like url_encode does, a NULL value becomes __HIVE_DEFAULT_PARTITION__.",
    ),
    f(
        "hour",
        "hour(col0)",
        "Extract the hour component from a date or timestamp",
    ),
    f(
        "if",
        "if(a, b, c)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "ilike_escape",
        "ilike_escape(string, like_specifier, escape_character)",
        "Returns `true` if the `string` matches the `like_specifier` (see Pattern Matching) using case-insensitive matching. `escape_character` is used to search for wildcard characters in the `string`.",
    ),
    f(
        "in_search_path",
        "in_search_path(database_name, schema_name)",
        "Returns whether or not the database/schema are in the search path",
    ),
    f(
        "index_key",
        "index_key(path, name)",
        "Generate index keys for a named index on a table (specified by a path struct with required 'table' and optional 'catalog'/'schema' fields) from key column arguments. Currently only supports ART indexes.",
    ),
    f(
        "inet_client_addr",
        "inet_client_addr()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "inet_client_port",
        "inet_client_port()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "inet_server_addr",
        "inet_server_addr()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "inet_server_port",
        "inet_server_port()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "instr",
        "instr(string, search_string)",
        "Returns location of first occurrence of `search_string` in `string`, counting from 1. Returns 0 if no match found.",
    ),
    f(
        "invoke",
        "invoke(lambda, arg1)",
        "Invokes a lambda function with the given arguments",
    ),
    f(
        "is_histogram_other_bin",
        "is_histogram_other_bin(val)",
        "Whether or not the provided value is the histogram \"other\" bin (used for values not belonging to any provided bin)",
    ),
    f(
        "isfinite",
        "isfinite(x)",
        "Returns true if the floating point value is finite, false otherwise",
    ),
    f(
        "isinf",
        "isinf(x)",
        "Returns true if the floating point value is infinite, false otherwise",
    ),
    f(
        "isnan",
        "isnan(x)",
        "Returns true if the floating point value is not a number, false otherwise",
    ),
    f(
        "isodow",
        "isodow(col0)",
        "Extract the isodow component from a date or timestamp",
    ),
    f(
        "isoyear",
        "isoyear(col0)",
        "Extract the isoyear component from a date or timestamp",
    ),
    f(
        "jaccard",
        "jaccard(s1, s2)",
        "The Jaccard similarity between two strings. Characters of different cases (e.g., `a` and `A`) are considered different. Returns a number between 0 and 1.",
    ),
    f(
        "jaro_similarity",
        "jaro_similarity(s1, s2)",
        "The Jaro similarity between two strings. Characters of different cases (e.g., `a` and `A`) are considered different. Returns a number between 0 and 1. For similarity < `score_cutoff`, 0 is returned instead. `score_cutoff` defaults to 0.",
    ),
    f(
        "jaro_winkler_similarity",
        "jaro_winkler_similarity(s1, s2)",
        "The Jaro-Winkler similarity between two strings. Characters of different cases (e.g., `a` and `A`) are considered different. Returns a number between 0 and 1. For similarity < `score_cutoff`, 0 is returned instead. `score_cutoff` defaults to 0.",
    ),
    f("json", "json(x)", "No description in `duckdb_functions()`"),
    f(
        "json_array",
        "json_array()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_array_length",
        "json_array_length(json)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_contains",
        "json_contains(haystack, needle)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_copy_strftime_if_date",
        "json_copy_strftime_if_date(x, format)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_copy_strftime_if_timestamp",
        "json_copy_strftime_if_timestamp(x, format)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_deep_merge",
        "json_deep_merge(json1, json2)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_deserialize_sql",
        "json_deserialize_sql(json)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_each",
        "json_each(json)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_execute_serialized_sql",
        "json_execute_serialized_sql(serialized_sql)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_exists",
        "json_exists(json, path)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_extract",
        "json_extract(json, index)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_extract_path",
        "json_extract_path(json, index)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_extract_path_text",
        "json_extract_path_text(json, index)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_extract_string",
        "json_extract_string(json, index)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_group_array",
        "json_group_array(x)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_group_object",
        "json_group_object(n, v)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_group_structure",
        "json_group_structure(x)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_insert",
        "json_insert(json, path, value)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_keys",
        "json_keys(json)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_merge_patch",
        "json_merge_patch(json1, json2)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_merge_patch_diff",
        "json_merge_patch_diff(old, new)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_normalize",
        "json_normalize(json)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_object",
        "json_object()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_pretty",
        "json_pretty(json)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_quote",
        "json_quote()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_remove",
        "json_remove(json, path)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_replace",
        "json_replace(json, path, value)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_serialize_plan",
        "json_serialize_plan(…)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_serialize_sql",
        "json_serialize_sql(sql, skip_null, skip_empty, skip_default, format)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_set",
        "json_set(json, path, value)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_strip_nulls",
        "json_strip_nulls(json)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_structure",
        "json_structure(json)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_transform",
        "json_transform(json, structure)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_transform_strict",
        "json_transform_strict(json, structure)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_tree",
        "json_tree(json)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_type",
        "json_type(json)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_valid",
        "json_valid(json)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "json_value",
        "json_value(json, index)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "julian",
        "julian(col0)",
        "Extract the Julian Day number from a date or timestamp",
    ),
    f(
        "kahan_sum",
        "kahan_sum(arg)",
        "Calculates the sum using a more accurate floating point summation (Kahan Sum).",
    ),
    f(
        "kurtosis",
        "kurtosis(x)",
        "Returns the excess kurtosis (Fisher’s definition) of all input values, with a bias correction according to the sample size",
    ),
    f(
        "kurtosis_pop",
        "kurtosis_pop(x)",
        "Returns the excess kurtosis (Fisher’s definition) of all input values, without bias correction",
    ),
    f(
        "last",
        "last(arg)",
        "Returns the last value of a column. This function is affected by ordering.",
    ),
    f(
        "last_day",
        "last_day(col0)",
        "Returns the last day of the month",
    ),
    f("lcase", "lcase(string)", "Converts `string` to lower case."),
    f(
        "lcm",
        "lcm(x, y)",
        "Computes the least common multiple of x and y",
    ),
    f(
        "least",
        "least(arg1)",
        "Returns the smallest value. For strings lexicographical ordering is used. Note that uppercase characters are considered “smaller” than lowercase characters, and collations are not supported.",
    ),
    f(
        "least_common_multiple",
        "least_common_multiple(x, y)",
        "Computes the least common multiple of x and y",
    ),
    f(
        "left",
        "left(string, count)",
        "Extracts the left-most count characters.",
    ),
    f(
        "left_grapheme",
        "left_grapheme(string, count)",
        "Extracts the left-most count grapheme clusters.",
    ),
    f("len", "len(bit)", "Number of characters in `string`."),
    f("length", "length(bit)", "Number of characters in `string`."),
    f(
        "length_grapheme",
        "length_grapheme(string)",
        "Number of grapheme clusters in `string`.",
    ),
    f(
        "levenshtein",
        "levenshtein(s1, s2)",
        "The minimum number of single-character edits (insertions, deletions or substitutions) required to change one string to the other. Characters of different cases (e.g., `a` and `A`) are considered different.",
    ),
    f(
        "lgamma",
        "lgamma(x)",
        "Computes the log of the gamma function",
    ),
    f(
        "like_escape",
        "like_escape(string, like_specifier, escape_character)",
        "Returns `true` if the `string` matches the `like_specifier` (see Pattern Matching) using case-sensitive matching. `escape_character` is used to search for wildcard characters in the `string`.",
    ),
    f(
        "list",
        "list(arg)",
        "Returns a LIST containing all the values of a column.",
    ),
    f(
        "list_aggr",
        "list_aggr(list, function_name)",
        "Executes the aggregate function `function_name` on the elements of `list`.",
    ),
    f(
        "list_aggregate",
        "list_aggregate(list, function_name)",
        "Executes the aggregate function `function_name` on the elements of `list`.",
    ),
    f(
        "list_any_value",
        "list_any_value(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_append",
        "list_append(l, e)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_apply",
        "list_apply(list, lambda)",
        "Returns a list that is the result of applying the `lambda` function to each element of the input `list`. The return type is defined by the return type of the `lambda` function.",
    ),
    f(
        "list_approx_count_distinct",
        "list_approx_count_distinct(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_avg",
        "list_avg(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_bit_and",
        "list_bit_and(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_bit_or",
        "list_bit_or(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_bit_xor",
        "list_bit_xor(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_bool_and",
        "list_bool_and(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_bool_or",
        "list_bool_or(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_cat",
        "list_cat()",
        "Concatenates lists. `NULL` inputs are skipped. See also operator `||`.",
    ),
    f(
        "list_concat",
        "list_concat()",
        "Concatenates lists. `NULL` inputs are skipped. See also operator `||`.",
    ),
    f(
        "list_contains",
        "list_contains(list, element)",
        "Returns true if the list contains the element.",
    ),
    f(
        "list_cosine_distance",
        "list_cosine_distance(list1, list2)",
        "Computes the cosine distance between two same-sized lists.",
    ),
    f(
        "list_cosine_similarity",
        "list_cosine_similarity(list1, list2)",
        "Computes the cosine similarity between two same-sized lists.",
    ),
    f(
        "list_count",
        "list_count(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_distance",
        "list_distance(list1, list2)",
        "Calculates the Euclidean distance between two points with coordinates given in two inputs lists of equal length.",
    ),
    f(
        "list_distinct",
        "list_distinct(list)",
        "Removes all duplicates and `NULL` values from a list. Does not preserve the original order.",
    ),
    f(
        "list_dot_product",
        "list_dot_product(list1, list2)",
        "Computes the inner product between two same-sized lists.",
    ),
    f(
        "list_element",
        "list_element(list, index)",
        "Extract the `index`th (1-based) value from the list.",
    ),
    f(
        "list_entropy",
        "list_entropy(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_extract",
        "list_extract(list, index)",
        "Extract the `index`th (1-based) value from the list.",
    ),
    f(
        "list_filter",
        "list_filter(list, lambda)",
        "Constructs a list from those elements of the input `list` for which the `lambda` function returns `true`. DuckDB must be able to cast the `lambda` function's return type to `BOOL`. The return type of `list_filter` is the same as the input list's.",
    ),
    f(
        "list_first",
        "list_first(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_grade_up",
        "list_grade_up(list)",
        "Works like list_sort, but the results are the indexes that correspond to the position in the original list instead of the actual values.",
    ),
    f(
        "list_has",
        "list_has(list, element)",
        "Returns true if the list contains the element.",
    ),
    f(
        "list_has_all",
        "list_has_all(list1, list2)",
        "Returns true if all elements of list2 are in list1. NULLs are ignored.",
    ),
    f(
        "list_has_any",
        "list_has_any(list1, list2)",
        "Returns true if the lists have any element in common. NULLs are ignored.",
    ),
    f(
        "list_histogram",
        "list_histogram(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_indexof",
        "list_indexof(list, element)",
        "Returns the index of the `element` if the `list` contains the `element`. If the `element` is not found, it returns `NULL`.",
    ),
    f(
        "list_inner_product",
        "list_inner_product(list1, list2)",
        "Computes the inner product between two same-sized lists.",
    ),
    f(
        "list_intersect",
        "list_intersect(list1, list2)",
        "Returns a list containing the distinct elements that are present in both `list1` and `list2`.",
    ),
    f(
        "list_kurtosis",
        "list_kurtosis(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_kurtosis_pop",
        "list_kurtosis_pop(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_last",
        "list_last(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_mad",
        "list_mad(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_max",
        "list_max(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_median",
        "list_median(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_min",
        "list_min(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_mode",
        "list_mode(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_negative_dot_product",
        "list_negative_dot_product(list1, list2)",
        "Computes the negative inner product between two same-sized lists.",
    ),
    f(
        "list_negative_inner_product",
        "list_negative_inner_product(list1, list2)",
        "Computes the negative inner product between two same-sized lists.",
    ),
    f(
        "list_pack",
        "list_pack()",
        "Creates a LIST containing the argument values.",
    ),
    f(
        "list_position",
        "list_position(list, element)",
        "Returns the index of the `element` if the `list` contains the `element`. If the `element` is not found, it returns `NULL`.",
    ),
    f(
        "list_prepend",
        "list_prepend(e, l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_product",
        "list_product(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_reduce",
        "list_reduce(list, lambda)",
        "Reduces all elements of the input `list` into a single scalar value by executing the `lambda` function on a running result and the next list element. The `lambda` function has an optional `initial_value` argument.",
    ),
    f(
        "list_resize",
        "list_resize(list, size)",
        "Resizes the `list` to contain `size` elements. Initializes new elements with `value` or `NULL` if `value` is not set.",
    ),
    f(
        "list_reverse",
        "list_reverse(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_reverse_sort",
        "list_reverse_sort(list)",
        "Sorts the elements of the list in reverse order.",
    ),
    f(
        "list_select",
        "list_select(value_list, index_list)",
        "Returns a list based on the elements selected by the `index_list`.",
    ),
    f(
        "list_sem",
        "list_sem(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_skewness",
        "list_skewness(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_slice",
        "list_slice(list, begin, end)",
        "Extracts a sublist or substring using slice conventions. Negative values are accepted.",
    ),
    f(
        "list_sort",
        "list_sort(list)",
        "Sorts the elements of the list.",
    ),
    f(
        "list_stddev_pop",
        "list_stddev_pop(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_stddev_samp",
        "list_stddev_samp(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_string_agg",
        "list_string_agg(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_sum",
        "list_sum(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_transform",
        "list_transform(list, lambda)",
        "Returns a list that is the result of applying the `lambda` function to each element of the input `list`. The return type is defined by the return type of the `lambda` function.",
    ),
    f(
        "list_unique",
        "list_unique(list)",
        "Counts the unique elements of a `list`.",
    ),
    f(
        "list_value",
        "list_value()",
        "Creates a LIST containing the argument values.",
    ),
    f(
        "list_var_pop",
        "list_var_pop(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_var_samp",
        "list_var_samp(l)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "list_where",
        "list_where(value_list, mask_list)",
        "Returns a list with the `BOOLEAN`s in `mask_list` applied as a mask to the `value_list`.",
    ),
    f(
        "list_zip",
        "list_zip()",
        "Zips n `LIST`s to a new `LIST` whose length will be that of the longest list. Its elements are structs of n elements from each list `list_1`, …, `list_n`, missing elements are replaced with `NULL`. If `truncate` is set, all lists are truncated to the smallest list length.",
    ),
    f(
        "listagg",
        "listagg(str)",
        "Concatenates the column string values with an optional separator.",
    ),
    f("ln", "ln(x)", "Computes the natural logarithm of x"),
    f(
        "localtime",
        "localtime",
        "The current local time, as of the start of the transaction",
    ),
    f(
        "localtimestamp",
        "localtimestamp",
        "The current local date and time, as of the start of the transaction",
    ),
    f(
        "log",
        "log(x)",
        "Computes the logarithm of x to base b. b may be omitted, in which case the default 10",
    ),
    f("log10", "log10(x)", "Computes the 10-log of x"),
    f("log2", "log2(x)", "Computes the 2-log of x"),
    f("lower", "lower(string)", "Converts `string` to lower case."),
    f(
        "lpad",
        "lpad(string, count, character)",
        "Pads the `string` with the `character` on the left until it has `count` characters. Truncates the `string` on the right if it has more than `count` characters.",
    ),
    f(
        "ltrim",
        "ltrim(string)",
        "Removes any occurrences of any of the `characters` from the left side of the `string`. `characters` defaults to `space`.",
    ),
    f(
        "lttb",
        "lttb(x, y, n)",
        "Downsamples the (x, y) points to (at most) n representative points using the Largest Triangle Three Buckets algorithm. Returns a LIST of STRUCT(x, y); the points must be ordered by x (use lttb(x, y, n ORDER BY x)).",
    ),
    f(
        "mad",
        "mad(x)",
        "Returns the median absolute deviation for the values within x. NULL values are ignored. Temporal types return a positive INTERVAL.",
    ),
    f(
        "make_date",
        "make_date(date-struct)",
        "The date for the given struct.",
    ),
    f(
        "make_time",
        "make_time(hour, minute, seconds)",
        "The time for the given parts",
    ),
    f(
        "make_timestamp",
        "make_timestamp(micros)",
        "The timestamp for the given parts",
    ),
    f(
        "make_timestamp_ms",
        "make_timestamp_ms(nanos)",
        "The timestamp for the given microseconds since the epoch",
    ),
    f(
        "make_timestamp_ns",
        "make_timestamp_ns(nanos)",
        "The timestamp for the given nanoseconds since epoch",
    ),
    f(
        "make_type",
        "make_type(name)",
        "Construct a type from its name and optional parameters",
    ),
    f(
        "map",
        "map()",
        "Creates a map from a set of keys and values",
    ),
    f(
        "map_concat",
        "map_concat()",
        "Returns a map created from merging the input maps, on key collision the value is taken from the last map with that key",
    ),
    f(
        "map_contains",
        "map_contains(map, key)",
        "Checks if a map contains a given key.",
    ),
    f(
        "map_contains_entry",
        "map_contains_entry(map, key, value)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "map_contains_value",
        "map_contains_value(map, value)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "map_entries",
        "map_entries(map)",
        "Returns the map entries as a list of keys/values",
    ),
    f(
        "map_extract",
        "map_extract(map, key)",
        "Returns a list containing the value for a given key or an empty list if the key is not contained in the map. The type of the key provided in the second parameter must match the type of the map’s keys else an error is returned",
    ),
    f(
        "map_extract_value",
        "map_extract_value(map, key)",
        "Returns the value for a given key or NULL if the key is not contained in the map. The type of the key provided in the second parameter must match the type of the map’s keys else an error is returned",
    ),
    f(
        "map_from_entries",
        "map_from_entries(map)",
        "Returns a map created from the entries of the array",
    ),
    f(
        "map_keys",
        "map_keys(map)",
        "Returns the keys of a map as a list",
    ),
    f(
        "map_to_pg_oid",
        "map_to_pg_oid(type_name)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "map_values",
        "map_values(map)",
        "Returns the values of a map as a list",
    ),
    f(
        "max",
        "max(arg)",
        "Returns the maximum value present in arg.",
    ),
    f(
        "max_by",
        "max_by(arg, val)",
        "Finds the row with the maximum val. Calculates the non-NULL arg expression at that row.",
    ),
    f(
        "md5",
        "md5(blob)",
        "Returns the MD5 hash of the `string` as a `VARCHAR`.",
    ),
    f(
        "md5_number",
        "md5_number(blob)",
        "Returns the MD5 hash of the `string` as a `HUGEINT`.",
    ),
    f(
        "md5_number_lower",
        "md5_number_lower(param)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "md5_number_upper",
        "md5_number_upper(param)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "mean",
        "mean(x)",
        "Calculates the average value for all tuples in x.",
    ),
    f(
        "median",
        "median(x)",
        "Returns the middle value of the set. NULL values are ignored. For even value counts, interpolate-able types (numeric, date/time) return the average of the two middle values. Non-interpolate-able types (everything else) return the lower of the two middle values.",
    ),
    f(
        "microsecond",
        "microsecond(col0)",
        "Extract the microsecond component from a date or timestamp",
    ),
    f(
        "millennium",
        "millennium(col0)",
        "Extract the millennium component from a date or timestamp",
    ),
    f(
        "millisecond",
        "millisecond(col0)",
        "Extract the millisecond component from a date or timestamp",
    ),
    f(
        "min",
        "min(arg)",
        "Returns the minimum value present in arg.",
    ),
    f(
        "min_by",
        "min_by(arg, val)",
        "Finds the row with the minimum val. Calculates the non-NULL arg expression at that row.",
    ),
    f(
        "minute",
        "minute(col0)",
        "Extract the minute component from a date or timestamp",
    ),
    f(
        "mismatches",
        "mismatches(s1, s2)",
        "The Hamming distance between to strings, i.e., the number of positions with different characters for two strings of equal length. Strings must be of equal length. Characters of different cases (e.g., `a` and `A`) are considered different.",
    ),
    f(
        "mod",
        "mod(left, right)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "mode",
        "mode(x)",
        "Returns the most frequent value for the values within x. NULL values are ignored.",
    ),
    f(
        "month",
        "month(col0)",
        "Extract the month component from a date or timestamp",
    ),
    f(
        "monthname",
        "monthname(col0)",
        "The (English) name of the month",
    ),
    f(
        "multiply",
        "multiply(left, right)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "nanosecond",
        "nanosecond(tsns)",
        "Extract the nanosecond component from a date or timestamp",
    ),
    f(
        "nextafter",
        "nextafter(x, y)",
        "Returns the next floating point value after x in the direction of y",
    ),
    f(
        "nextval",
        "nextval(sequence_name)",
        "Return the following value of the sequence.",
    ),
    f(
        "nfc_normalize",
        "nfc_normalize(string)",
        "Converts `string` to Unicode NFC normalized string. Useful for comparisons and ordering if text data is mixed between NFC normalized and not.",
    ),
    f(
        "normalized_interval",
        "normalized_interval(interval)",
        "Normalizes an INTERVAL to an equivalent interval",
    ),
    f(
        "not_ilike_escape",
        "not_ilike_escape(string, like_specifier, escape_character)",
        "Returns `false` if the `string` matches the `like_specifier` (see Pattern Matching) using case-insensitive matching. `escape_character` is used to search for wildcard characters in the `string`.",
    ),
    f(
        "not_like_escape",
        "not_like_escape(string, like_specifier, escape_character)",
        "Returns `false` if the `string` matches the `like_specifier` (see Pattern Matching) using case-sensitive matching. `escape_character` is used to search for wildcard characters in the `string`.",
    ),
    f("now", "now()", "Returns the current timestamp"),
    f(
        "nullif",
        "nullif(a, b)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "obj_description",
        "obj_description(object_oid, catalog_name)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "octet_length",
        "octet_length(bitstring)",
        "Number of bytes in `blob`.",
    ),
    f(
        "ord",
        "ord(string)",
        "Returns an `INTEGER` representing the `unicode` codepoint of the first character in the `string`.",
    ),
    f(
        "overlay",
        "overlay(string, replacement, start)",
        "Replaces a substring of `string` starting at character `start` with `replacement`. If `count` is not specified, it defaults to the length of `replacement`. Note that a `start` value of `1` refers to the first character of the `string`.",
    ),
    f(
        "parquet_bloom_probe",
        "parquet_bloom_probe(path, column_name, value)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "parquet_file_metadata",
        "parquet_file_metadata(path)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "parquet_full_metadata",
        "parquet_full_metadata(path)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "parquet_kv_metadata",
        "parquet_kv_metadata(path)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "parquet_metadata",
        "parquet_metadata(path)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "parquet_scan",
        "parquet_scan(…)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "parquet_schema",
        "parquet_schema(path)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "parse_dirname",
        "parse_dirname(path)",
        "Returns the top-level directory name from the given `path`. `separator` options: `system`, `both_slash` (default), `forward_slash`, `backslash`.",
    ),
    f(
        "parse_dirpath",
        "parse_dirpath(path)",
        "Returns the head of the `path` (the pathname until the last slash) similarly to Python's `os.path.dirname`. `separator` options: `system`, `both_slash` (default), `forward_slash`, `backslash`.",
    ),
    f(
        "parse_duckdb_log_message",
        "parse_duckdb_log_message(type, message)",
        "Parse the message into the expected logical type",
    ),
    f(
        "parse_filename",
        "parse_filename(string)",
        "Returns the last component of the `path` similarly to Python's `os.path.basename` function. If `trim_extension` is `true`, the file extension will be removed (defaults to `false`). `separator` options: `system`, `both_slash` (default), `forward_slash`, `backslash`.",
    ),
    f(
        "parse_formatted_bytes",
        "parse_formatted_bytes(string)",
        "Parses a human-readable representation of a size in bytes into an integer.",
    ),
    f(
        "parse_path",
        "parse_path(path)",
        "Returns a list of the components (directories and filename) in the `path` similarly to Python's `pathlib.parts` function. `separator` options: `system`, `both_slash` (default), `forward_slash`, `backslash`.",
    ),
    f(
        "path_join",
        "path_join(path)",
        "Joins path components using the platform separator, normalizing duplicate separators and dot segments. Absolute RHS components must be compatible (same scheme/parent) or raise an error.",
    ),
    f(
        "pg_collation_is_visible",
        "pg_collation_is_visible(collation_oid)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pg_conf_load_time",
        "pg_conf_load_time()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pg_conversion_is_visible",
        "pg_conversion_is_visible(conversion_oid)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pg_function_is_visible",
        "pg_function_is_visible(function_oid)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pg_get_constraintdef",
        "pg_get_constraintdef(constraint_oid)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pg_get_expr",
        "pg_get_expr(pg_node_tree, relation_oid)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pg_get_viewdef",
        "pg_get_viewdef(oid)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pg_has_role",
        "pg_has_role(role, privilege)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pg_is_other_temp_schema",
        "pg_is_other_temp_schema(schema_id)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pg_my_temp_schema",
        "pg_my_temp_schema()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pg_opclass_is_visible",
        "pg_opclass_is_visible(opclass_oid)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pg_operator_is_visible",
        "pg_operator_is_visible(operator_oid)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pg_opfamily_is_visible",
        "pg_opfamily_is_visible(opclass_oid)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pg_postmaster_start_time",
        "pg_postmaster_start_time()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pg_size_pretty",
        "pg_size_pretty(bytes)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pg_sleep",
        "pg_sleep(seconds)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pg_table_is_visible",
        "pg_table_is_visible(table_oid)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pg_ts_config_is_visible",
        "pg_ts_config_is_visible(config_oid)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pg_ts_dict_is_visible",
        "pg_ts_dict_is_visible(dict_oid)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pg_ts_parser_is_visible",
        "pg_ts_parser_is_visible(parser_oid)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pg_ts_template_is_visible",
        "pg_ts_template_is_visible(template_oid)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pg_type_is_visible",
        "pg_type_is_visible(type_oid)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pg_typeof",
        "pg_typeof(expression)",
        "No description in `duckdb_functions()`",
    ),
    f("pi", "pi()", "Returns the value of pi"),
    f(
        "position",
        "position(string, search_string)",
        "Returns location of first occurrence of `search_string` in `string`, counting from 1. Returns 0 if no match found.",
    ),
    f("pow", "pow(x, y)", "Computes x to the power of y"),
    f("power", "power(x, y)", "Computes x to the power of y"),
    f(
        "pragma_collations",
        "pragma_collations()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pragma_database_size",
        "pragma_database_size()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pragma_metadata_info",
        "pragma_metadata_info()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pragma_platform",
        "pragma_platform()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pragma_show",
        "pragma_show(table_name)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pragma_storage_info",
        "pragma_storage_info(…)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pragma_table_info",
        "pragma_table_info(table_name)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pragma_user_agent",
        "pragma_user_agent()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "pragma_version",
        "pragma_version()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "prefix",
        "prefix(string, search_string)",
        "Returns `true` if `string` starts with `search_string`.",
    ),
    f(
        "printf",
        "printf(format)",
        "Formats a `string` using printf syntax.",
    ),
    f(
        "product",
        "product(arg)",
        "Calculates the product of all tuples in arg.",
    ),
    f(
        "quantile",
        "quantile(x)",
        "Returns the exact quantile number between 0 and 1 . If pos is a LIST of FLOATs, then the result is a LIST of the corresponding exact quantiles.",
    ),
    f(
        "quantile_cont",
        "quantile_cont(x, quantile)",
        "Returns the interpolated quantile number between 0 and 1 . If pos is a LIST of FLOATs, then the result is a LIST of the corresponding interpolated quantiles.",
    ),
    f(
        "quantile_disc",
        "quantile_disc(x)",
        "Returns the exact quantile number between 0 and 1 . If pos is a LIST of FLOATs, then the result is a LIST of the corresponding exact quantiles.",
    ),
    f(
        "quarter",
        "quarter(col0)",
        "Extract the quarter component from a date or timestamp",
    ),
    f(
        "query",
        "query(query)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "query_table",
        "query_table(table_name)",
        "No description in `duckdb_functions()`",
    ),
    f("radians", "radians(x)", "Converts degrees to radians"),
    f(
        "random",
        "random()",
        "Returns a random number between 0 and 1",
    ),
    f(
        "range",
        "range(stop)",
        "Creates a list of values between `start` and `stop` - the stop parameter is exclusive.",
    ),
    f(
        "read_blob",
        "read_blob(path, allow_empty)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "read_csv",
        "read_csv(…)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "read_csv_auto",
        "read_csv_auto(…)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "read_duckdb",
        "read_duckdb(…)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "read_json",
        "read_json(…)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "read_json_auto",
        "read_json_auto(…)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "read_json_objects",
        "read_json_objects(…)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "read_json_objects_auto",
        "read_json_objects_auto(…)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "read_ndjson",
        "read_ndjson(…)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "read_ndjson_auto",
        "read_ndjson_auto(…)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "read_ndjson_objects",
        "read_ndjson_objects(…)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "read_parquet",
        "read_parquet(…)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "read_single_csv_file",
        "read_single_csv_file(…)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "read_single_json_file",
        "read_single_json_file(…)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "read_single_parquet_file",
        "read_single_parquet_file(…)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "read_text",
        "read_text(path, allow_empty)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "reduce",
        "reduce(list, lambda)",
        "Reduces all elements of the input `list` into a single scalar value by executing the `lambda` function on a running result and the next list element. The `lambda` function has an optional `initial_value` argument.",
    ),
    f(
        "regexp_escape",
        "regexp_escape(string)",
        "Escapes special patterns to turn `string` into a regular expression similarly to Python's `re.escape` function.",
    ),
    f(
        "regexp_extract",
        "regexp_extract(string, regex)",
        "If `string` contains the `regex` pattern, returns the capturing group specified by optional parameter `group`; otherwise, returns the empty string. The `group` must be a constant value. If no `group` is given, it defaults to 0. A set of optional regex `options` can be set.",
    ),
    f(
        "regexp_extract_all",
        "regexp_extract_all(string, regex)",
        "Finds non-overlapping occurrences of the `regex` in the `string` and returns the corresponding values of the capturing `group`. A set of optional regex `options` can be set.",
    ),
    f(
        "regexp_full_match",
        "regexp_full_match(string, regex)",
        "Returns `true` if the entire `string` matches the `regex`. A set of optional regex `options` can be set.",
    ),
    f(
        "regexp_matches",
        "regexp_matches(string, regex)",
        "Returns `true` if `string` contains the `regex`, `false` otherwise. A set of optional regex `options` can be set.",
    ),
    f(
        "regexp_replace",
        "regexp_replace(string, regex, replacement)",
        "If `string` contains the `regex`, replaces the matching part with `replacement`. A set of optional regex `options` can be set.",
    ),
    f(
        "regexp_split_to_array",
        "regexp_split_to_array(string, regex)",
        "Splits the `string` along the `regex`. A set of optional regex `options` can be set.",
    ),
    f(
        "regexp_split_to_table",
        "regexp_split_to_table(text, pattern)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "register_external_resource",
        "register_external_resource(…)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "register_external_resource_type",
        "register_external_resource_type(…)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "regr_avgx",
        "regr_avgx(y, x)",
        "Returns the average of the independent variable for non-NULL pairs in a group, where x is the independent variable and y is the dependent variable.",
    ),
    f(
        "regr_avgy",
        "regr_avgy(y, x)",
        "Returns the average of the dependent variable for non-NULL pairs in a group, where x is the independent variable and y is the dependent variable.",
    ),
    f(
        "regr_count",
        "regr_count(y, x)",
        "Returns the number of non-NULL number pairs in a group.",
    ),
    f(
        "regr_intercept",
        "regr_intercept(y, x)",
        "Returns the intercept of the univariate linear regression line for non-NULL pairs in a group.",
    ),
    f(
        "regr_r2",
        "regr_r2(y, x)",
        "Returns the coefficient of determination for non-NULL pairs in a group.",
    ),
    f(
        "regr_slope",
        "regr_slope(y, x)",
        "Returns the slope of the linear regression line for non-NULL pairs in a group.",
    ),
    f(
        "regr_sxx",
        "regr_sxx(y, x)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "regr_sxy",
        "regr_sxy(y, x)",
        "Returns the population covariance of input values",
    ),
    f(
        "regr_syy",
        "regr_syy(y, x)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "remap_struct",
        "remap_struct(input, target_type, mapping, defaults)",
        "Map a struct to another struct type, potentially re-ordering, renaming and casting members and filling in defaults for missing values",
    ),
    f(
        "repeat",
        "repeat(blob, count)",
        "Repeats the `string` `count` number of times.",
    ),
    f(
        "repeat_row",
        "repeat_row(num_rows)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "replace",
        "replace(string, source, target)",
        "Replaces any occurrences of the `source` with `target` in `string`.",
    ),
    f(
        "replace_type",
        "replace_type(param, type1, type2)",
        "Casts all fields of type1 to type2",
    ),
    f(
        "reservoir_quantile",
        "reservoir_quantile(x, quantile, sample_size)",
        "Gives the approximate quantile using reservoir sampling, the sample size is optional and uses 8192 as a default size.",
    ),
    f("reverse", "reverse(string)", "Reverses the `string`."),
    f(
        "right",
        "right(string, count)",
        "Extract the right-most `count` characters.",
    ),
    f(
        "right_grapheme",
        "right_grapheme(string, count)",
        "Extracts the right-most `count` grapheme clusters.",
    ),
    f("round", "round(x)", "Rounds x to s decimal places"),
    f(
        "round_even",
        "round_even(x, precision)",
        "Rounds x to s decimal places, rounding halfway cases to the nearest even digit",
    ),
    f(
        "roundbankers",
        "roundbankers(x, precision)",
        "Rounds x to s decimal places, rounding halfway cases to the nearest even digit",
    ),
    f(
        "row",
        "row()",
        "Create an unnamed STRUCT (tuple) containing the argument values.",
    ),
    f(
        "row_to_json",
        "row_to_json()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "rpad",
        "rpad(string, count, character)",
        "Pads the `string` with the `character` on the right until it has `count` characters. Truncates the `string` on the right if it has more than `count` characters.",
    ),
    f(
        "rtrim",
        "rtrim(string)",
        "Removes any occurrences of any of the `characters` from the right side of the `string`. `characters` defaults to `space`.",
    ),
    f(
        "second",
        "second(col0)",
        "Extract the second component from a date or timestamp",
    ),
    f("sem", "sem(x)", "Returns the standard error of the mean"),
    f(
        "seq_scan",
        "seq_scan()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "session_user",
        "session_user()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "set_bit",
        "set_bit(bitstring, index, new_value)",
        "Sets the nth bit in bitstring to newvalue; the first (leftmost) bit is indexed 0. Returns a new bitstring",
    ),
    f(
        "setseed",
        "setseed(seed)",
        "Sets the seed to be used for the random function",
    ),
    f(
        "setval",
        "setval(sequence_name, value)",
        "Set the value of the sequence to the specified value. If is_called is true, the sequence is called. If is_called is false, the sequence is not called.",
    ),
    f(
        "sha1",
        "sha1(blob)",
        "Returns a `VARCHAR` with the SHA-1 hash of the `value`.",
    ),
    f(
        "sha256",
        "sha256(blob)",
        "Returns a `VARCHAR` with the SHA-256 hash of the `value`",
    ),
    f(
        "shobj_description",
        "shobj_description(object_oid, catalog_name)",
        "No description in `duckdb_functions()`",
    ),
    f("sign", "sign(x)", "Returns the sign of x as -1, 0 or 1"),
    f(
        "signbit",
        "signbit(x)",
        "Returns whether the signbit is set or not",
    ),
    f("sin", "sin(x)", "Computes the sin of x"),
    f("sinh", "sinh(x)", "Computes the hyperbolic sin of x"),
    f(
        "skewness",
        "skewness(x)",
        "Returns the skewness of all input values.",
    ),
    f(
        "sleep_ms",
        "sleep_ms(milliseconds)",
        "Sleeps for the specified number of milliseconds and returns NULL",
    ),
    f(
        "sniff_csv",
        "sniff_csv(…)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "split",
        "split(string, separator)",
        "Splits the `string` along the `separator`.",
    ),
    f(
        "split_part",
        "split_part(string, delimiter, position)",
        "No description in `duckdb_functions()`",
    ),
    f("sqrt", "sqrt(x)", "Returns the square root of x"),
    f(
        "st_asbinary",
        "st_asbinary(geom)",
        "Returns the Well-Known Binary (WKB) representation of the geometry",
    ),
    f(
        "st_asgeojson",
        "st_asgeojson(geom)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "st_astext",
        "st_astext(geom)",
        "Returns the Well-Known Text (WKT) representation of the geometry",
    ),
    f(
        "st_aswkb",
        "st_aswkb(geom)",
        "Returns the Well-Known Binary (WKB) representation of the geometry",
    ),
    f(
        "st_aswkt",
        "st_aswkt(geom)",
        "Returns the Well-Known Text (WKT) representation of the geometry",
    ),
    f(
        "st_crs",
        "st_crs(geom)",
        "Returns the Coordinate Reference System (CRS) identifier of the geometry",
    ),
    f(
        "st_geomfromgeojson",
        "st_geomfromgeojson(geojson)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "st_geomfromwkb",
        "st_geomfromwkb(wkb)",
        "Creates a geometry from Well-Known Binary (WKB) representation",
    ),
    f(
        "st_intersects_extent",
        "st_intersects_extent(geom1, geom2)",
        "Returns true if the geometries bounding boxes intersect",
    ),
    f(
        "st_setcrs",
        "st_setcrs(geom, crs)",
        "Sets the Coordinate Reference System (CRS) identifier of the geometry",
    ),
    f(
        "starts_with",
        "starts_with(string, search_string)",
        "Returns `true` if `string` begins with `search_string`.",
    ),
    f(
        "stats",
        "stats(expression)",
        "Returns a string with statistics about the expression. Expression can be a column, constant, or SQL expression",
    ),
    f(
        "stddev",
        "stddev(x)",
        "Returns the sample standard deviation",
    ),
    f(
        "stddev_pop",
        "stddev_pop(x)",
        "Returns the population standard deviation.",
    ),
    f(
        "stddev_samp",
        "stddev_samp(x)",
        "Returns the sample standard deviation",
    ),
    f(
        "str_split",
        "str_split(string, separator)",
        "Splits the `string` along the `separator`.",
    ),
    f(
        "str_split_regex",
        "str_split_regex(string, regex)",
        "Splits the `string` along the `regex`. A set of optional regex `options` can be set.",
    ),
    f(
        "strftime",
        "strftime(data, format)",
        "Converts a `date` to a string according to the format string.",
    ),
    f(
        "string_agg",
        "string_agg(str)",
        "Concatenates the column string values with an optional separator.",
    ),
    f(
        "string_split",
        "string_split(string, separator)",
        "Splits the `string` along the `separator`.",
    ),
    f(
        "string_split_regex",
        "string_split_regex(string, regex)",
        "Splits the `string` along the `regex`. A set of optional regex `options` can be set.",
    ),
    f(
        "string_to_array",
        "string_to_array(string, separator)",
        "Splits the `string` along the `separator`.",
    ),
    f(
        "strip_accents",
        "strip_accents(string)",
        "Strips accents from `string`.",
    ),
    f("strlen", "strlen(string)", "Number of bytes in `string`."),
    f(
        "strpos",
        "strpos(string, search_string)",
        "Returns location of first occurrence of `search_string` in `string`, counting from 1. Returns 0 if no match found.",
    ),
    f(
        "strptime",
        "strptime(text, format)",
        "Converts the `string` text to timestamp according to the format string. Throws an error on failure. To return `NULL` on failure, use try_strptime.",
    ),
    f(
        "struct_concat",
        "struct_concat()",
        "Merge the multiple STRUCTs into a single STRUCT.",
    ),
    f(
        "struct_contains",
        "struct_contains(struct, entry)",
        "Check if an unnamed STRUCT contains the value.",
    ),
    f(
        "struct_extract",
        "struct_extract(struct, entry)",
        "Extract the named entry from the STRUCT.",
    ),
    f(
        "struct_extract_at",
        "struct_extract_at(struct, index)",
        "Extract the entry from the STRUCT by position (starts at 1!).",
    ),
    f(
        "struct_has",
        "struct_has(struct, entry)",
        "Check if an unnamed STRUCT contains the value.",
    ),
    f(
        "struct_indexof",
        "struct_indexof(struct, entry)",
        "Get the position of the entry in an unnamed STRUCT, starting at 1.",
    ),
    f(
        "struct_insert",
        "struct_insert(struct)",
        "Adds field(s)/value(s) to an existing STRUCT with the argument values. The entry name(s) will be the bound variable name(s)",
    ),
    f(
        "struct_keys",
        "struct_keys(struct)",
        "Returns the field names of a STRUCT as a list",
    ),
    f(
        "struct_pack",
        "struct_pack()",
        "Create a STRUCT containing the argument values. The entry name will be the bound variable name.",
    ),
    f(
        "struct_position",
        "struct_position(struct, entry)",
        "Get the position of the entry in an unnamed STRUCT, starting at 1.",
    ),
    f(
        "struct_update",
        "struct_update(struct)",
        "Changes field(s)/value(s) to an existing STRUCT with the argument values. The entry name(s) will be the bound variable name(s)",
    ),
    f(
        "struct_values",
        "struct_values(struct)",
        "Returns the field values of a STRUCT as an UnnamedStruct",
    ),
    f(
        "substr",
        "substr(string, start)",
        "Extracts substring starting from character `start` up to the end of the string. If optional argument `length` is set, extracts a substring of `length` characters instead. Note that a `start` value of `1` refers to the first character of the `string`.",
    ),
    f(
        "substring",
        "substring(string, start)",
        "Extracts substring starting from character `start` up to the end of the string. If optional argument `length` is set, extracts a substring of `length` characters instead. Note that a `start` value of `1` refers to the first character of the `string`.",
    ),
    f(
        "substring_grapheme",
        "substring_grapheme(string, start)",
        "Extracts substring starting from grapheme clusters `start` up to the end of the string. If optional argument `length` is set, extracts a substring of `length` grapheme clusters instead. Note that a `start` value of `1` refers to the `first` character of the `string`.",
    ),
    f(
        "subtract",
        "subtract(x)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "suffix",
        "suffix(string, search_string)",
        "Returns `true` if `string` ends with `search_string`.",
    ),
    f(
        "sum",
        "sum(arg)",
        "Calculates the sum value for all tuples in arg.",
    ),
    f(
        "sum_no_overflow",
        "sum_no_overflow(arg)",
        "Internal only. Calculates the sum value for all tuples in arg without overflow checks.",
    ),
    f(
        "sumkahan",
        "sumkahan(arg)",
        "Calculates the sum using a more accurate floating point summation (Kahan Sum).",
    ),
    f(
        "summary",
        "summary(input)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "switch",
        "switch(map)",
        "Creates a switch statement similar to CASE WHEN/THEN",
    ),
    f("tan", "tan(x)", "Computes the tan of x"),
    f("tanh", "tanh(x)", "Computes the hyperbolic tan of x"),
    f(
        "test_all_types",
        "test_all_types(use_large_enum, use_large_bignum)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "test_vector_types",
        "test_vector_types(column, all_flat)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "time_bucket",
        "time_bucket(bucket_width, timestamp)",
        "Truncate TIMESTAMPTZ by the specified interval bucket_width. Buckets are aligned relative to origin TIMESTAMPTZ. The origin defaults to 2000-01-03 00:00:00+00 for buckets that do not include a month or year interval, and to 2000-01-01 00:00:00+00 for month and year buckets",
    ),
    f(
        "timetz_byte_comparable",
        "timetz_byte_comparable(time_tz)",
        "Converts a TIME WITH TIME ZONE to an integer sort key",
    ),
    f(
        "timezone",
        "timezone(col0)",
        "Extract the timezone component from a date or timestamp",
    ),
    f(
        "timezone_hour",
        "timezone_hour(col0)",
        "Extract the timezone_hour component from a date or timestamp",
    ),
    f(
        "timezone_minute",
        "timezone_minute(col0)",
        "Extract the timezone_minute component from a date or timestamp",
    ),
    f(
        "to_aggregate_state",
        "to_aggregate_state(data, name, signature)",
        "Converts a value into the aggregate state of the aggregate function with the given name and signature. The type of the value must exactly match the state layout of the aggregate function. An optional fourth argument supplies constant parameter values (e.g. string_agg's separator) as a list with one entry per argument, using NULL for arguments that are not bound to a constant.",
    ),
    f(
        "to_base",
        "to_base(number, radix, min_length)",
        "Converts `number` to a string in the given base `radix`, optionally padding with leading zeros to `min_length`.",
    ),
    f(
        "to_base64",
        "to_base64(blob)",
        "Converts a `blob` to a base64 encoded string.",
    ),
    f(
        "to_binary",
        "to_binary(string)",
        "Converts the `string` to binary representation.",
    ),
    f(
        "to_centuries",
        "to_centuries(integer)",
        "Construct a century interval",
    ),
    f("to_days", "to_days(integer)", "Construct a day interval"),
    f(
        "to_decades",
        "to_decades(integer)",
        "Construct a decade interval",
    ),
    f(
        "to_hex",
        "to_hex(blob)",
        "Converts the `string` to hexadecimal representation.",
    ),
    f("to_hours", "to_hours(integer)", "Construct a hour interval"),
    f(
        "to_json",
        "to_json()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "to_microseconds",
        "to_microseconds(integer)",
        "Construct a microsecond interval",
    ),
    f(
        "to_millennia",
        "to_millennia(integer)",
        "Construct a millenium interval",
    ),
    f(
        "to_milliseconds",
        "to_milliseconds(double)",
        "Construct a millisecond interval",
    ),
    f(
        "to_minutes",
        "to_minutes(integer)",
        "Construct a minute interval",
    ),
    f(
        "to_months",
        "to_months(integer)",
        "Construct a month interval",
    ),
    f(
        "to_quarters",
        "to_quarters(integer)",
        "Construct a quarter interval",
    ),
    f(
        "to_seconds",
        "to_seconds(double)",
        "Construct a second interval",
    ),
    f(
        "to_timestamp",
        "to_timestamp(sec)",
        "Converts secs since epoch to a timestamp with time zone",
    ),
    f("to_weeks", "to_weeks(integer)", "Construct a week interval"),
    f("to_years", "to_years(integer)", "Construct a year interval"),
    f(
        "transaction_timestamp",
        "transaction_timestamp()",
        "Returns the current timestamp",
    ),
    f(
        "translate",
        "translate(string, from, to)",
        "Replaces each character in `string` that matches a character in the `from` set with the corresponding character in the `to` set. If `from` is longer than `to`, occurrences of the extra characters in `from` are deleted.",
    ),
    f(
        "trim",
        "trim(string)",
        "Removes any occurrences of any of the `characters` from either side of the `string`. `characters` defaults to `space`.",
    ),
    f("trunc", "trunc(x)", "Truncates the number"),
    f(
        "truncate_duckdb_logs",
        "truncate_duckdb_logs()",
        "No description in `duckdb_functions()`",
    ),
    f(
        "try_strptime",
        "try_strptime(text, format)",
        "Converts the `string` text to timestamp according to the format string. Returns `NULL` on failure.",
    ),
    f(
        "txid_current",
        "txid_current()",
        "Returns the current transaction’s ID (a BIGINT). It will assign a new one if the current transaction does not have one already",
    ),
    f(
        "typeof",
        "typeof(expression)",
        "Returns the name of the data type of the result of the expression",
    ),
    f("ucase", "ucase(string)", "Converts `string` to upper case."),
    f(
        "unbin",
        "unbin(value)",
        "Converts a `value` from binary representation to a blob.",
    ),
    f(
        "unhex",
        "unhex(value)",
        "Converts a `value` from hexadecimal representation to a blob.",
    ),
    f(
        "unicode",
        "unicode(string)",
        "Returns an `INTEGER` representing the `unicode` codepoint of the first character in the `string`.",
    ),
    f(
        "union_extract",
        "union_extract(union, tag)",
        "Extract the value with the named tags from the union. NULL if the tag is not currently selected",
    ),
    f(
        "union_tag",
        "union_tag(union)",
        "Retrieve the currently selected tag of the union as an ENUM",
    ),
    f(
        "union_value",
        "union_value()",
        "Create a single member UNION containing the argument value. The tag of the value will be the bound variable name",
    ),
    f(
        "unnest",
        "unnest(list)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "unpivot_list",
        "unpivot_list()",
        "Identical to list_value, but generated as part of unpivot for better error messages.",
    ),
    f("upper", "upper(string)", "Converts `string` to upper case."),
    f(
        "url_decode",
        "url_decode(string)",
        "Decodes a URL from a representation using Percent-Encoding.",
    ),
    f(
        "url_encode",
        "url_encode(string)",
        "Encodes a URL to a representation using Percent-Encoding.",
    ),
    f("user", "user()", "No description in `duckdb_functions()`"),
    f(
        "uuid",
        "uuid()",
        "Returns a random UUID v4 similar to this: eeccb8c5-9943-b2bb-bb5e-222f4e14b687",
    ),
    f(
        "uuid_extract_timestamp",
        "uuid_extract_timestamp(uuid)",
        "Extract the timestamp for the given UUID v7.",
    ),
    f(
        "uuid_extract_version",
        "uuid_extract_version(uuid)",
        "Extract a version for the given UUID.",
    ),
    f(
        "uuidv4",
        "uuidv4()",
        "Returns a random UUIDv4 similar to this: eeccb8c5-9943-b2bb-bb5e-222f4e14b687",
    ),
    f(
        "uuidv7",
        "uuidv7()",
        "Returns a random UUID v7 similar to this: 019482e4-1441-7aad-8127-eec99573b0a0",
    ),
    f("var_pop", "var_pop(x)", "Returns the population variance."),
    f(
        "var_samp",
        "var_samp(x)",
        "Returns the sample variance of all input values.",
    ),
    f(
        "variance",
        "variance(x)",
        "Returns the sample variance of all input values.",
    ),
    f(
        "variant_array_length",
        "variant_array_length(input_variant)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "variant_bytes_to_variant",
        "variant_bytes_to_variant(blob)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "variant_comparator",
        "variant_comparator(input_variant)",
        "Returns a binary sort key that orders `input_variant` according to the logical ordering of VARIANT values (used internally for comparison and ordering).",
    ),
    f(
        "variant_contains",
        "variant_contains(variant_haystack, variant_needle)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "variant_exists",
        "variant_exists(input_variant, path)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "variant_extract",
        "variant_extract(input_variant, field)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "variant_extract_string",
        "variant_extract_string(input_variant, path)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "variant_group_array",
        "variant_group_array(x)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "variant_group_object",
        "variant_group_object(key, value)",
        "Returns a VARIANT object containing all key, value pairs in the aggregation.",
    ),
    f(
        "variant_keys",
        "variant_keys(input_variant)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "variant_normalize",
        "variant_normalize(input_variant)",
        "Normalizes the `input_variant` to a canonical representation.",
    ),
    f(
        "variant_to_parquet_variant",
        "variant_to_parquet_variant(variant)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "variant_type",
        "variant_type(input_variant)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "variant_typeof",
        "variant_typeof(input_variant)",
        "Returns the internal type of the `input_variant`.",
    ),
    f(
        "vector_type",
        "vector_type(col)",
        "Returns the VectorType of a given column",
    ),
    f(
        "version",
        "version()",
        "Returns the currently active version of DuckDB in this format: v0.3.2",
    ),
    f(
        "vertex_extract",
        "vertex_extract(geom, coordinate)",
        "Extracts the specified coordinate (X, Y, Z, M) from a point geometry",
    ),
    f(
        "wavg",
        "wavg(value, weight)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "week",
        "week(col0)",
        "Extract the week component from a date or timestamp",
    ),
    f(
        "weekday",
        "weekday(col0)",
        "Extract the weekday component from a date or timestamp",
    ),
    f(
        "weekofyear",
        "weekofyear(col0)",
        "Extract the weekofyear component from a date or timestamp",
    ),
    f(
        "weighted_avg",
        "weighted_avg(value, weight)",
        "No description in `duckdb_functions()`",
    ),
    f(
        "which_secret",
        "which_secret(path, type)",
        "No description in `duckdb_functions()`",
    ),
    f("write_log", "write_log(string)", "Writes to the logger"),
    f("xor", "xor(left, right)", "Bitwise XOR"),
    f(
        "year",
        "year(col0)",
        "Extract the year component from a date or timestamp",
    ),
    f(
        "yearweek",
        "yearweek(col0)",
        "Extract the yearweek component from a date or timestamp",
    ),
];

/// The words DuckDB reserves, upper-case, in order.
pub const DUCKDB_RESERVED: &[&str] = &[
    "ALL",
    "ANALYSE",
    "ANALYZE",
    "AND",
    "ANY",
    "ARRAY",
    "AS",
    "ASC",
    "ASYMMETRIC",
    "BOTH",
    "CASE",
    "CAST",
    "CHECK",
    "COLLATE",
    "COLUMN",
    "CONSTRAINT",
    "CREATE",
    "DEFAULT",
    "DEFERRABLE",
    "DESC",
    "DESCRIBE",
    "DISTINCT",
    "DO",
    "ELSE",
    "END",
    "EXCEPT",
    "FALSE",
    "FETCH",
    "FOR",
    "FOREIGN",
    "FROM",
    "GROUP",
    "HAVING",
    "IN",
    "INITIALLY",
    "INTERSECT",
    "INTO",
    "LAMBDA",
    "LATERAL",
    "LEADING",
    "LIMIT",
    "NOT",
    "NULL",
    "OFFSET",
    "ON",
    "ONLY",
    "OR",
    "ORDER",
    "PIVOT",
    "PIVOT_LONGER",
    "PIVOT_WIDER",
    "PLACING",
    "PRIMARY",
    "QUALIFY",
    "REFERENCES",
    "RETURNING",
    "SELECT",
    "SHOW",
    "SOME",
    "SUMMARIZE",
    "SYMMETRIC",
    "TABLE",
    "THEN",
    "TO",
    "TRAILING",
    "TRUE",
    "UNION",
    "UNIQUE",
    "UNPIVOT",
    "USING",
    "VARIADIC",
    "WHEN",
    "WHERE",
    "WINDOW",
    "WITH",
];
