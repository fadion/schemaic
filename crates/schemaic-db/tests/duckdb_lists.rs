//! DuckDB's hand-kept lists in `schemaic-core`, checked against the engine
//! this workspace links.
//!
//! **The builtin catalogue beside them is generated and pinned**
//! (`db::duckdb`'s `the_builtin_catalogue_is_the_linked_engines`); these were
//! written by hand and disagreed with the engine without failing anything.
//! The formatter's operator table was one: it lacked `<->`, `<=>`, `~*`,
//! `!~`, `!~*` and `~~~`, so Format code cut each into pieces and wrote a
//! syntax error — or a different expression — into the buffer.
//!
//! **Engine-checked through the public [`DataSession`]**, an in-memory
//! database no file backs, which is why it may live in the pure tier: no
//! server, no file, no network. It is an integration test rather than a
//! module of `db::duckdb` because it asks only what any caller can.

use schemaic_core::intel::SqlDialect;
use schemaic_core::sqlfmt::{format_sql, operator_table};
use schemaic_db::duckdb::DataSession;
use schemaic_db::{Db, Engine};
use tokio_util::sync::CancellationToken;

/// One expression per operator in the table, which DuckDB 2.0.0 runs. A new
/// entry in the table needs a line here, or the test says which.
fn sample(op: &str) -> Option<&'static str> {
    Some(match op {
        "!~~*" => "SELECT 'abc' !~~* 'A%'",
        "~~*" => "SELECT 'abc' ~~* 'A%'",
        "!~~" => "SELECT 'abc' !~~ 'a%'",
        "~~~" => "SELECT 'abc' ~~~ 'a*'",
        "!~*" => "SELECT 'abc' !~* 'A.*'",
        "<->" => "SELECT [1.0, 2.0]::FLOAT[2] <-> [2.0, 4.0]::FLOAT[2]",
        "<=>" => "SELECT [1.0, 2.0]::FLOAT[2] <=> [2.0, 4.0]::FLOAT[2]",
        "->>" => "SELECT '{\"a\": 1}'::JSON ->> 'a'",
        "::" => "SELECT '7'::INTEGER",
        "->" => "SELECT '{\"a\": 1}'::JSON -> 'a'",
        ">=" => "SELECT 2 >= 1",
        "<=" => "SELECT 2 <= 1",
        "<>" => "SELECT 2 <> 1",
        "!=" => "SELECT 2 != 1",
        "==" => "SELECT 2 == 1",
        "||" => "SELECT 'a' || 'b'",
        "<<" => "SELECT 1 << 3",
        ">>" => "SELECT 16 >> 2",
        "**" => "SELECT 2 ** 5",
        "//" => "SELECT 7 // 2",
        "^@" => "SELECT 'abc' ^@ 'ab'",
        "@>" => "SELECT [1, 2, 3] @> [2]",
        "<@" => "SELECT [2] <@ [1, 2, 3]",
        "&&" => "SELECT [1, 2] && [2, 3]",
        "~~" => "SELECT 'abc' ~~ 'a%'",
        "!~" => "SELECT 'abc' !~ 'a.*'",
        "~*" => "SELECT 'abc' ~* 'A.*'",
        ":=" => "SELECT struct_pack(a := 1)",
        _ => return None,
    })
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// The single cell `sql` answers, as the grid would show it.
async fn answer(db: &Db, sql: &str) -> Result<String, String> {
    let rs = db
        .fetch_query(None, sql, 10, CancellationToken::new())
        .await
        .map_err(|e| e.to_string())?;
    Ok(rs
        .cell(0, 0)
        .map(|c| c.display().to_string())
        .unwrap_or_default())
}

/// **Every multi-byte operator the formatter keeps whole on DuckDB is one
/// DuckDB has, and survives Format code**: the formatted text runs and
/// answers what the original answers.
#[test]
fn every_duckdb_operator_in_the_formatter_runs_the_same_after_formatting() {
    let session = DataSession::open().unwrap();
    let db = Db::from_parts(
        Engine::DuckDb,
        String::new(),
        0,
        String::new(),
        String::new(),
        session.target().to_string(),
    );
    let rt = rt();
    let mut problems = Vec::new();
    for op in operator_table(SqlDialect::DuckDb) {
        let Some(sql) = sample(op) else {
            problems.push(format!("{op}: no sample expression in this test"));
            continue;
        };
        let formatted = format_sql(sql, "    ", SqlDialect::DuckDb);
        if !formatted.contains(op) {
            problems.push(format!("{op}: formatting lost it: {formatted}"));
        }
        let before = rt.block_on(answer(&db, sql));
        let after = rt.block_on(answer(&db, &formatted));
        match (&before, &after) {
            (Ok(b), Ok(a)) if a == b && !b.is_empty() => {}
            _ => problems.push(format!("{op}: {sql} → {before:?}; {formatted} → {after:?}")),
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}
