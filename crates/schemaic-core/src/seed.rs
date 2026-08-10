//! AI-generated seed data — the pure, testable core.
//!
//! The UI/app side samples the base table and runs a one-shot `claude -p` call;
//! this module owns the two error-prone halves that must stay unit-tested:
//! **building the prompt** and **parsing the model's reply**. Nothing here touches
//! a DB, the network, or the CLI — the app feeds already-snapshotted data in and
//! stages the parsed result through the grid's normal write-back path.
//!
//! Two surfaces share the parsing risk:
//! - **Fill Value** — one cell → [`build_fill_prompt`] / [`parse_fill_response`].
//! - **Seed rows** (Insert Row / Seed Table) — N rows as JSON → [`parse_seed_response`].

/// One row as `(column_name, value)` pairs. `None` = SQL `NULL`; every value is a
/// plain string (bound server-side, coerced to the column type by the write-back
/// path). Key order is not significant — the grid maps names back to column
/// indices — and `parse_seed_response` emits keys in serde_json's (sorted) order.
pub type Row = Vec<(String, Option<String>)>;

/// Outcome of parsing a single-cell "fill value" reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FillOutcome {
    /// A concrete value to stage into the cell.
    Value(String),
    /// The model chose SQL `NULL`.
    Null,
    /// No usable content came back (empty reply) — the caller should surface an
    /// error rather than stage anything.
    Empty,
}

/// Why a seed-rows reply couldn't be turned into rows.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SeedError {
    #[error("the model returned no content")]
    Empty,
    #[error("the reply was not valid JSON: {0}")]
    NotJson(String),
    #[error("the reply was not a JSON array of row objects")]
    NotArrayOfObjects,
}

/// Strip a single leading/trailing markdown code fence if the model wrapped its
/// reply in one (```` ```json … ``` ````, ```` ``` … ``` ````), returning the inner
/// body trimmed. Text with no fence is returned trimmed unchanged.
fn strip_code_fence(text: &str) -> &str {
    let t = text.trim();
    let Some(rest) = t.strip_prefix("```") else {
        return t;
    };
    // Drop an optional language token on the opening line (```json, ```sql, …).
    let rest = match rest.find('\n') {
        Some(nl) => &rest[nl + 1..],
        None => rest.trim_start_matches(|c: char| c.is_ascii_alphanumeric()),
    };
    // Drop the closing fence if present.
    let body = match rest.rfind("```") {
        Some(idx) => &rest[..idx],
        None => rest,
    };
    body.trim()
}

/// Convert one JSON value into a staged cell string. `null` → `None`; booleans
/// become `1`/`0` (MySQL `tinyint(1)`-friendly); numbers/strings pass through;
/// arrays/objects serialize back to JSON text (for JSON columns).
fn json_value_to_cell(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::Null => None,
        serde_json::Value::Bool(b) => Some(if *b { "1" } else { "0" }.to_string()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

/// Parse a single-cell reply. Strips a code fence, trims, reads a bare/quoted
/// `null` (any case) as [`FillOutcome::Null`], unquotes a JSON string, maps a JSON
/// bool to `1`/`0`, and otherwise takes the trimmed text literally. Empty →
/// [`FillOutcome::Empty`].
pub fn parse_fill_response(stdout: &str) -> FillOutcome {
    let t = strip_code_fence(stdout);
    if t.is_empty() {
        return FillOutcome::Empty;
    }
    // A bare NULL (any case) — not valid JSON when upper-cased, so handle first.
    if t.eq_ignore_ascii_case("null") {
        return FillOutcome::Null;
    }
    match serde_json::from_str::<serde_json::Value>(t) {
        Ok(serde_json::Value::String(s)) => FillOutcome::Value(s),
        Ok(serde_json::Value::Null) => FillOutcome::Null,
        Ok(serde_json::Value::Bool(b)) => FillOutcome::Value(if b { "1" } else { "0" }.to_string()),
        // Numbers, and anything not cleanly JSON, are taken verbatim.
        _ => FillOutcome::Value(t.to_string()),
    }
}

/// Parse a seed-rows reply: a JSON array of objects, each becoming a [`Row`].
/// Strips a code fence first. Object key order is preserved as-is (irrelevant
/// downstream). Errors distinguish empty / not-JSON / wrong-shape so the caller
/// can message precisely.
pub fn parse_seed_response(stdout: &str) -> Result<Vec<Row>, SeedError> {
    let t = strip_code_fence(stdout);
    if t.is_empty() {
        return Err(SeedError::Empty);
    }
    let value: serde_json::Value =
        serde_json::from_str(t).map_err(|e| SeedError::NotJson(e.to_string()))?;
    let arr = value.as_array().ok_or(SeedError::NotArrayOfObjects)?;
    let mut rows = Vec::with_capacity(arr.len());
    for item in arr {
        let obj = item.as_object().ok_or(SeedError::NotArrayOfObjects)?;
        let row: Row = obj
            .iter()
            .map(|(k, v)| (k.clone(), json_value_to_cell(v)))
            .collect();
        rows.push(row);
    }
    Ok(rows)
}

/// Render a list of rows as a compact JSON array for embedding in a prompt (the
/// same shape the model is asked to emit back). `None` values render as `null`.
fn rows_to_json(rows: &[Row]) -> String {
    let arr: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            let map: serde_json::Map<String, serde_json::Value> = row
                .iter()
                .map(|(k, v)| {
                    let jv = match v {
                        Some(s) => serde_json::Value::String(s.clone()),
                        None => serde_json::Value::Null,
                    };
                    (k.clone(), jv)
                })
                .collect();
            serde_json::Value::Object(map)
        })
        .collect();
    serde_json::to_string(&serde_json::Value::Array(arr)).unwrap_or_else(|_| "[]".to_string())
}

/// Build the one-shot prompt to fill a single cell. Feeds the model the table's
/// DDL skeleton (structure), a bottom-sample of recent rows (conventions: enums,
/// formats, valid FK values), and the row being filled (for coherence), and
/// demands a bare value back. `sample` may be empty (new/empty table).
pub fn build_fill_prompt(
    table: &str,
    column: &str,
    ddl: &str,
    sample: &[Row],
    row_context: &[(String, Option<String>)],
    dialect: crate::intel::SqlDialect,
) -> String {
    let engine = dialect.engine_label();
    let sample_section = if sample.is_empty() {
        "The table currently has no rows to sample — infer a realistic value from \
         the column's type and name."
            .to_string()
    } else {
        format!(
            "Recent rows from the table (JSON, most recent last). Infer the format, \
             allowed values, and any pattern (enums, sequences, rotating values) from \
             these:\n{}",
            rows_to_json(sample)
        )
    };
    let context_section = if row_context.is_empty() {
        String::new()
    } else {
        format!(
            "\n\nThe row being filled, with its other columns already set:\n{}",
            rows_to_json(&[row_context.to_vec()])
        )
    };
    format!(
        "You are generating a single realistic test-data value for one column of a \
         {engine} table.\n\n\
         Table: {table}\n\
         Target column: {column}\n\n\
         Schema:\n{ddl}\n\n\
         {sample_section}{context_section}\n\n\
         Return ONLY the raw value for `{column}` — no quotes, no markdown, no \
         explanation. Use lowercase `null` for a SQL NULL. Match the style, format, \
         and value set of the sample rows, and keep it consistent with the row being \
         filled."
    )
}

/// Build the one-shot prompt to generate `n` seed rows (Insert Row = 1, Seed
/// Table = N). Feeds the DDL + a bottom-sample and asks for a JSON array of
/// objects covering only `fill_columns` (the editable, non-auto-increment columns
/// — the grid skips the rest and lets the server default them). `sample` may be
/// empty (new/empty table).
pub fn build_seed_prompt(
    table: &str,
    ddl: &str,
    fill_columns: &[String],
    sample: &[Row],
    n: usize,
    dialect: crate::intel::SqlDialect,
) -> String {
    let engine = dialect.engine_label();
    let cols = fill_columns.join(", ");
    let sample_section = if sample.is_empty() {
        "The table currently has no rows to sample — infer realistic values from the \
         schema (column types, names, and any foreign keys)."
            .to_string()
    } else {
        format!(
            "Recent rows from the table (JSON, most recent last). Infer formats, allowed \
             values, sequences, and valid foreign-key values from these — reuse FK values \
             you see here:\n{}",
            rows_to_json(sample)
        )
    };
    format!(
        "You are generating {n} row(s) of realistic test data for a {engine} \
         table.\n\n\
         Table: {table}\n\
         Fill ONLY these columns: {cols}\n\
         (Auto-increment and default columns are intentionally omitted — do not \
         include them.)\n\n\
         Schema:\n{ddl}\n\n\
         {sample_section}\n\n\
         Return ONLY a JSON array of exactly {n} object(s) — one per row — each mapping \
         the columns above to values. Use JSON `null` for a SQL NULL. Keep values \
         consistent with the sample. No markdown, no prose, just the JSON array."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pairs: &[(&str, Option<&str>)]) -> Row {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.map(|s| s.to_string())))
            .collect()
    }

    // ── parse_fill_response ──────────────────────────────────────────────

    #[test]
    fn fill_bare_value() {
        assert_eq!(
            parse_fill_response("active"),
            FillOutcome::Value("active".into())
        );
    }

    #[test]
    fn fill_trims_surrounding_whitespace() {
        assert_eq!(
            parse_fill_response("  \n  active@example.com \n"),
            FillOutcome::Value("active@example.com".into())
        );
    }

    #[test]
    fn fill_unquotes_json_string() {
        assert_eq!(
            parse_fill_response("\"hello world\""),
            FillOutcome::Value("hello world".into())
        );
    }

    #[test]
    fn fill_strips_code_fence() {
        assert_eq!(
            parse_fill_response("```json\n\"boxed\"\n```"),
            FillOutcome::Value("boxed".into())
        );
    }

    #[test]
    fn fill_number_is_literal() {
        assert_eq!(parse_fill_response("42"), FillOutcome::Value("42".into()));
    }

    #[test]
    fn fill_bool_maps_to_one_zero() {
        assert_eq!(parse_fill_response("true"), FillOutcome::Value("1".into()));
        assert_eq!(parse_fill_response("false"), FillOutcome::Value("0".into()));
    }

    #[test]
    fn fill_null_any_case() {
        assert_eq!(parse_fill_response("null"), FillOutcome::Null);
        assert_eq!(parse_fill_response("NULL"), FillOutcome::Null);
        assert_eq!(parse_fill_response(" Null \n"), FillOutcome::Null);
    }

    #[test]
    fn fill_empty_is_empty() {
        assert_eq!(parse_fill_response(""), FillOutcome::Empty);
        assert_eq!(parse_fill_response("   \n  "), FillOutcome::Empty);
        assert_eq!(parse_fill_response("```\n```"), FillOutcome::Empty);
    }

    // ── parse_seed_response ──────────────────────────────────────────────

    #[test]
    fn seed_happy_path() {
        let rows = parse_seed_response(r#"[{"name":"Ada","age":36}]"#).unwrap();
        assert_eq!(rows.len(), 1);
        // Key order isn't guaranteed (serde_json sorts) — assert by membership.
        assert!(rows[0].contains(&("name".to_string(), Some("Ada".to_string()))));
        assert!(rows[0].contains(&("age".to_string(), Some("36".to_string()))));
        assert_eq!(rows[0].len(), 2);
    }

    #[test]
    fn seed_multiple_rows() {
        let rows = parse_seed_response(r#"[{"a":"x"},{"a":"y"},{"a":"z"}]"#).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2], row(&[("a", Some("z"))]));
    }

    #[test]
    fn seed_strips_json_fence() {
        let rows = parse_seed_response("```json\n[{\"a\":1}]\n```").unwrap();
        assert_eq!(rows[0], row(&[("a", Some("1"))]));
    }

    #[test]
    fn seed_bare_fence_no_lang() {
        let rows = parse_seed_response("```\n[{\"a\":1}]\n```").unwrap();
        assert_eq!(rows[0], row(&[("a", Some("1"))]));
    }

    #[test]
    fn seed_null_becomes_none() {
        let rows = parse_seed_response(r#"[{"a":null}]"#).unwrap();
        assert_eq!(rows[0], row(&[("a", None)]));
    }

    #[test]
    fn seed_bool_maps_to_one_zero() {
        let rows = parse_seed_response(r#"[{"active":true,"deleted":false}]"#).unwrap();
        assert_eq!(
            rows[0],
            row(&[("active", Some("1")), ("deleted", Some("0"))])
        );
    }

    #[test]
    fn seed_nested_value_is_json_text() {
        let rows = parse_seed_response(r#"[{"meta":{"k":1}}]"#).unwrap();
        assert_eq!(rows[0], row(&[("meta", Some(r#"{"k":1}"#))]));
    }

    #[test]
    fn seed_empty_reply_errors() {
        assert_eq!(parse_seed_response(""), Err(SeedError::Empty));
        assert_eq!(parse_seed_response("```\n```"), Err(SeedError::Empty));
    }

    #[test]
    fn seed_not_json_errors() {
        assert!(matches!(
            parse_seed_response("sorry, I can't do that"),
            Err(SeedError::NotJson(_))
        ));
    }

    #[test]
    fn seed_object_not_array_errors() {
        assert_eq!(
            parse_seed_response(r#"{"a":1}"#),
            Err(SeedError::NotArrayOfObjects)
        );
    }

    #[test]
    fn seed_array_of_scalars_errors() {
        assert_eq!(
            parse_seed_response("[1,2,3]"),
            Err(SeedError::NotArrayOfObjects)
        );
    }

    #[test]
    fn seed_empty_array_is_ok_empty() {
        assert_eq!(parse_seed_response("[]").unwrap(), Vec::<Row>::new());
    }

    // ── build_fill_prompt ────────────────────────────────────────────────

    #[test]
    fn fill_prompt_includes_key_context() {
        let sample = vec![
            row(&[("id", Some("1")), ("status", Some("active"))]),
            row(&[("id", Some("2")), ("status", Some("pending"))]),
        ];
        let ctx = row(&[("id", Some("3"))]);
        let p = build_fill_prompt(
            "shop.orders",
            "status",
            "CREATE TABLE ...",
            &sample,
            &ctx,
            crate::intel::SqlDialect::MySql,
        );
        assert!(p.contains("shop.orders"));
        assert!(p.contains("status"));
        assert!(p.contains("CREATE TABLE ..."));
        assert!(p.contains("active")); // sample value present for pattern inference
        assert!(p.contains("pending"));
        assert!(p.contains("null")); // NULL instruction present
        assert!(p.contains("ONLY")); // bare-value demand present
    }

    #[test]
    fn fill_prompt_handles_empty_sample() {
        let p = build_fill_prompt("t", "c", "ddl", &[], &[], crate::intel::SqlDialect::MySql);
        assert!(p.contains("no rows to sample"));
    }

    // ── build_seed_prompt ────────────────────────────────────────────────

    #[test]
    fn seed_prompt_includes_key_context() {
        let sample = vec![row(&[("name", Some("Ada")), ("role", Some("admin"))])];
        let cols = vec!["name".to_string(), "role".to_string()];
        let p = build_seed_prompt(
            "app.users",
            "CREATE TABLE users ...",
            &cols,
            &sample,
            5,
            crate::intel::SqlDialect::MySql,
        );
        assert!(p.contains("app.users"));
        assert!(p.contains("CREATE TABLE users ..."));
        assert!(p.contains("name, role")); // fill-column list
        assert!(p.contains("admin")); // sample value for inference
        assert!(p.contains('5')); // requested count
        assert!(p.contains("JSON array"));
    }

    #[test]
    fn seed_prompt_handles_empty_sample() {
        let p = build_seed_prompt(
            "t",
            "ddl",
            &["c".to_string()],
            &[],
            1,
            crate::intel::SqlDialect::MySql,
        );
        assert!(p.contains("no rows to sample"));
    }

    /// Every AI surface used to hardcode "MySQL/MariaDB", so on a PostgreSQL
    /// connection the model was asked for the wrong engine's SQL — and obliges,
    /// with backticks and `LIMIT x, y` the server rejects.
    #[test]
    fn seed_prompts_name_the_connections_own_engine() {
        use crate::intel::SqlDialect;
        for (dialect, want, wrong) in [
            (SqlDialect::Postgres, "PostgreSQL", "MySQL"),
            (SqlDialect::MySql, "MySQL/MariaDB", "PostgreSQL"),
        ] {
            let fill = build_fill_prompt("t", "c", "ddl", &[], &[], dialect);
            assert!(fill.contains(want), "fill prompt, {dialect:?}:\n{fill}");
            assert!(!fill.contains(wrong), "fill prompt, {dialect:?}:\n{fill}");

            let seed = build_seed_prompt("t", "ddl", &["c".to_string()], &[], 3, dialect);
            assert!(seed.contains(want), "seed prompt, {dialect:?}:\n{seed}");
            assert!(!seed.contains(wrong), "seed prompt, {dialect:?}:\n{seed}");
        }
    }
}
