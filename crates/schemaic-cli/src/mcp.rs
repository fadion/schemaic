//! The MCP (stdio) server — one server, with two ways in.
//!
//! Speaks newline-delimited JSON-RPC over stdin/stdout and exposes these tools
//! ([`McpTool::ALL`]), none of which writes anything:
//!   - `run_query` — run a single read-only statement, return the rows.
//!   - `explain` — a read-only statement's plan, never executed, and what in it
//!     looks slow.
//!   - `sample_rows` — the first rows of one table, every column, unordered.
//!   - `list_databases` — the databases' names alone.
//!   - `list_schema` — the server as an overview (databases → table names), or
//!     one database in full (columns, PK/NOT NULL markers, foreign-key edges).
//!   - `describe_table` — one table's DDL, foreign keys, and — where the
//!     connection sends rows — a few sample rows.
//!   - `search_objects` — tables, views and columns by part of their name.
//!   - `propose_table_change` — check a schema change and show the DDL it would
//!     produce, for the AI panel to turn into a preview. Runs nothing.
//!   - `propose_sql` — what the write guard says of a statement the model means
//!     to offer, for the AI panel's code block to Insert or Run. Runs nothing.
//!
//! The split between the schema tools is deliberate: enriching every table
//! with keys makes each entry bigger, so the broad listings stay cheap and the
//! detail lives behind a drill-down rather than blowing the model's context on
//! a 50-database server. `list_databases` is the cheapest rung, the names
//! without even the tables.
//!
//! **Who starts it decides the [`Host`]**, and the host decides what is offered:
//!
//! - [`Host::Panel`] — `schemaic --mcp-serve`, spawned by whichever agent CLI
//!   (`claude`, `codex`, `agy`, …) the AI panel launched. The endpoint is the
//!   app's active connection, already tunnelled for SSH, and reaches the
//!   process in `$SCHEMAIC_MCP_ENDPOINT` or an `--endpoint-file` — never on a
//!   command line (review C6). The app builds it; `schemaic-app/src/ai.rs`
//!   parses it.
//! - [`Host::Standalone`] — `schemaic mcp -c <conn>`, spawned by any MCP client
//!   (an editor's agent, a desktop assistant). The connection is resolved
//!   through `schemaic-conn` exactly as `schemaic query` resolves it, gated on
//!   `cli_access`, and its tunnel is held for the server's lifetime. Nothing
//!   reads the model's reply there, so `propose_table_change` and
//!   `propose_sql` have nowhere to go and are withheld.
//!
//! Either way the endpoint is a structured `schemaic_db::Db` handle — no
//! credential URL is involved — and it lives in this crate rather than the app's
//! so the second way in is the same server, not a copy of its gates.

use std::collections::HashSet;

use schemaic_core::ddl::Target;
use schemaic_core::intel::SqlDialect;
use schemaic_core::model::ResultSet;
use schemaic_core::propose::{self, Proposal};
use schemaic_core::schema::{DbSchema, TableInfo};
use schemaic_db::Db;
use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;

// Max rows returned to the AI/MCP client per query (a small preview — the
// model reasons over a sample, not the full result). Distinct from QUERY_ROW_CAP.
const MCP_ROW_CAP: usize = 200;

// Characters of one cell a tool result carries. Deliberately narrower than a
// chat attachment's: this rides along on every turn that calls a tool, where the
// attachment was asked for once.
const MCP_CELL_CHARS: usize = 60;

/// The MCP protocol revisions this server actually implements, newest first.
///
/// **`2025-06-18` costs a stdio, tools-only server nothing it did not already
/// do**: its breaking changes are HTTP's (OAuth, the version header) and the
/// removal of batching, and everything it adds — structured output,
/// elicitation, resource links, `title`, `_meta` — is optional. The one thing
/// taken from it is tool `annotations` ([`annotated`]).
///
/// **`2025-03-26` is deliberately absent**: it *requires* accepting JSON-RPC
/// batches, which this server does not, and the next revision removed them
/// again. [`negotiate_protocol`] answers a client asking for it with
/// `2024-11-05`.
const SUPPORTED_PROTOCOLS: &[&str] = &["2025-06-18", "2024-11-05"];

/// The first revision whose tools carry `annotations`.
const ANNOTATIONS_SINCE: &str = "2025-03-26";

/// Answer the `initialize` handshake with a version this server really speaks.
///
/// It used to echo whatever the client asked for, which makes the handshake a
/// mirror: a future `claude` negotiating a revised protocol — different
/// tool-result shape, different error convention — would be told "yes, I speak
/// that" and then get `2024-11-05` behaviour. The failure mode isn't a crash but
/// a silent skew: results that parse as empty, or tools reported unavailable,
/// with nothing pointing at the version.
///
/// Per the MCP spec the server replies with a version it supports; if that isn't
/// the one requested, the client decides whether to proceed. So an unknown
/// request gets an answer rather than an error — the client can still walk
/// away, and it is told the truth either way.
///
/// **Which answer, once there are two.** A client names the newest revision it
/// speaks, so one of ours that is *newer* than the request is one it cannot
/// know, and the spec tells it to disconnect. So: the newest we speak that is
/// not newer than the request; our oldest when the request predates them all
/// (the nearest); our newest when it is newer than them all, or names no
/// revision. Revisions are `YYYY-MM-DD`, which compare as strings.
fn negotiate_protocol(requested: Option<&str>) -> &'static str {
    let newest = SUPPORTED_PROTOCOLS[0];
    let oldest = SUPPORTED_PROTOCOLS[SUPPORTED_PROTOCOLS.len() - 1];
    let Some(r) = requested.filter(|r| is_revision(r)) else {
        return newest;
    };
    SUPPORTED_PROTOCOLS
        .iter()
        .copied()
        .find(|s| *s <= r)
        .unwrap_or(oldest)
}

/// Is this shaped like a revision (`YYYY-MM-DD`), so that comparing it with
/// ours as a string means comparing dates?
fn is_revision(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            _ => c.is_ascii_digit(),
        })
}

/// `tools` as a session on `protocol` should see them: with each tool's
/// `annotations` from a revision that defines them, and exactly as before for
/// `2024-11-05`, whose strict clients have no field by that name.
///
/// Only `readOnlyHint`, from [`McpTool::read_only`] — the one claim every tool
/// here can make without qualification, and the one a client may use to decide
/// whether a call needs the user's say-so.
fn annotated(mut tools: Value, protocol: &str) -> Value {
    if protocol < ANNOTATIONS_SINCE {
        return tools;
    }
    for t in tools.as_array_mut().into_iter().flatten() {
        let name = t["name"].as_str().unwrap_or("");
        // `list_connections` is the server's own, not an `McpTool`: it names
        // what this process was given and reads nothing.
        let read_only = match McpTool::from_name(name) {
            Some(tool) => tool.read_only(),
            None if name == LIST_CONNECTIONS => true,
            None => continue,
        };
        t["annotations"] = json!({ "readOnlyHint": read_only });
    }
    tools
}

/// The lines an endpoint recorded, kept in memory — see
/// [`Endpoint::audit_sink`].
pub type AuditSink = std::sync::Arc<std::sync::Mutex<Vec<schemaic_core::write_log::Entry>>>;

/// One connection a server serves, under the name a tool call picks it by.
pub struct Named<'a> {
    pub name: &'a str,
    pub endpoint: &'a Endpoint,
}

/// The tool that names a several-connection server's connections — offered
/// only when there is more than one, so a one-connection server's listing is
/// exactly what it always was.
const LIST_CONNECTIONS: &str = "list_connections";

/// What `tools/list` advertises for `endpoints`.
///
/// **One connection is the listing it always was.** Several add a `connection`
/// argument to every tool and a `list_connections` tool, and offer a tool when
/// **any** connection's levels offer it — each call is still judged by its own
/// connection's levels ([`refusal_for`]), so a tool listed for one connection
/// is refused for another that withholds it, in that connection's words. The
/// descriptions name the first connection's engine; `list_connections` names
/// each one's.
fn listing(endpoints: &[Named<'_>]) -> Value {
    let first = endpoints[0].endpoint;
    if endpoints.len() == 1 {
        // No other connection to name, so no `source_connection` — as there
        // is no `connection`.
        let mut tools = tools_list(first.db.engine(), first.samples, first.schema, first.host);
        for t in tools.as_array_mut().into_iter().flatten() {
            if let Some(props) = t["inputSchema"]["properties"].as_object_mut() {
                props.remove("source_connection");
            }
        }
        return tools;
    }
    let reads_data = endpoints.iter().any(|n| n.endpoint.samples);
    let schema = endpoints.iter().any(|n| n.endpoint.schema);
    // `describe_table` promises sample rows only when **every** connection
    // sends them: a model told to expect them on one that does not plans its
    // turn around rows that never come.
    let all_rows = endpoints.iter().all(|n| n.endpoint.samples);
    let mut tools = tools_list(first.db.engine(), reads_data, schema, first.host);
    if reads_data && !all_rows {
        for t in tools.as_array_mut().into_iter().flatten() {
            if t["name"] == McpTool::DescribeTable.name() {
                t["description"] = Value::String(
                    "One table in depth: its CREATE TABLE (or CREATE VIEW) definition including \
                     indexes, its foreign keys, and — where the connection sends rows, which \
                     list_connections says — a few sample rows."
                        .to_string(),
                );
            }
        }
    }
    // **Each tool asked of each connection's own levels**, not of the levels
    // OR-ed apart: a tool that needs rows *and* the catalogue is offered by
    // neither of a connection with rows but no schema and one with the
    // reverse, though the OR-ed pair would list it.
    if let Some(list) = tools.as_array_mut() {
        list.retain(|t| {
            McpTool::from_name(t["name"].as_str().unwrap_or("")).is_some_and(|tool| {
                endpoints
                    .iter()
                    .any(|n| tool.offered(n.endpoint.samples, n.endpoint.schema, n.endpoint.host))
            })
        });
    }
    let pick = json!({
        "type": "string",
        "description": format!(
            "Which connection, as list_connections names it. Defaults to {}.",
            endpoints[0].name
        )
    });
    for t in tools.as_array_mut().into_iter().flatten() {
        t["inputSchema"]["properties"]["connection"] = pick.clone();
    }
    if let Some(list) = tools.as_array_mut() {
        list.push(json!({
            "name": LIST_CONNECTIONS,
            "description": "The connections this server was given: each one's name, engine, \
                            default database, and whether you may read its rows. Pass a name \
                            as any other tool's `connection`.",
            "inputSchema": { "type": "object", "properties": {} }
        }));
    }
    tools
}

/// The connection a call's `connection` argument names, or the first when it
/// names none — or why it cannot be served.
fn pick<'a>(endpoints: &'a [Named<'a>], args: &Value) -> Result<&'a Endpoint, String> {
    match name_arg(endpoints, args, "connection")? {
        None => Ok(endpoints[0].endpoint),
        Some(want) => find_named(endpoints, want).map(|n| n.endpoint),
    }
}

/// A connection-naming argument: `None` when absent, `null` or empty — the
/// default — or **why it is not a name**.
///
/// **A value that is not a string is refused, not taken for absent.** A
/// model writing a connection called `2024` as the number `2024` was
/// answered on the default connection, silently, and an agent comparing the
/// two got the same connection's answer twice.
fn name_arg<'v>(
    endpoints: &[Named<'_>],
    args: &'v Value,
    key: &str,
) -> Result<Option<&'v str>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str()).filter(|s| !s.is_empty())),
        Some(_) => Err(format!(
            "`{key}` must be a connection's name, as a string; {}",
            serves(endpoints)
        )),
    }
}

/// The served connection named `want`, or a refusal that says what is served.
fn find_named<'a>(endpoints: &'a [Named<'a>], want: &str) -> Result<&'a Named<'a>, String> {
    endpoints.iter().find(|n| n.name == want).ok_or_else(|| {
        format!(
            "No connection named {want:?} on this server; {}",
            serves(endpoints)
        )
    })
}

/// What a refusal says this server serves — its connections' names, or, for
/// a server whose one connection has none to give, that there is one.
fn serves(endpoints: &[Named<'_>]) -> String {
    let names: Vec<&str> = endpoints
        .iter()
        .map(|n| n.name)
        .filter(|n| !n.is_empty())
        .collect();
    if names.is_empty() {
        "it serves one connection: leave the argument out.".to_string()
    } else {
        format!("it serves {}.", names.join(", "))
    }
}

/// The other connection a `compare_schemas` call names, when it names one —
/// the one call that needs two endpoints, so [`serve_on`] answers it rather
/// than `call_tool`, which is handed one.
fn cross_connection_source<'a>(name: &str, args: &'a Value) -> Option<&'a Value> {
    (name == McpTool::CompareSchemas.name())
        .then(|| args.get("source_connection"))
        .flatten()
        .filter(|v| !v.is_null() && v.as_str() != Some(""))
}

/// `compare_schemas` across two served connections: each resolved by name,
/// each held to its own levels as `call_tool` holds one (`refusal_for`), and
/// the call logged under the target's name, as any call is under its own.
async fn compare_across(endpoints: &[Named<'_>], name: &str, args: &Value) -> Value {
    let by_name = |want: Option<&str>| match want {
        None => Ok(&endpoints[0]),
        Some(w) => find_named(endpoints, w),
    };
    // A name that is not a string is refused for either side, never read as
    // the default.
    let target_name = name_arg(endpoints, args, "connection");
    let source_name = name_arg(endpoints, args, "source_connection");
    let sides = target_name.clone().and_then(|t| {
        let s = source_name?;
        by_name(t).and_then(|t| by_name(s).map(|s| (t, s)))
    });
    let target_name = target_name.ok().flatten();
    let (text, is_error) = match sides {
        Err(why) => (why, true),
        Ok((t, s)) => {
            let refused = [t, s]
                .into_iter()
                .find_map(|n| refusal_on(n.endpoint, McpTool::CompareSchemas));
            match refused {
                Some(why) => (why.to_string(), true),
                None => {
                    Box::pin(compare_schemas(
                        (t.endpoint, t.name),
                        (s.endpoint, s.name),
                        args,
                    ))
                    .await
                }
            }
        }
    };
    // Logged under the target when it was found, else the default's name.
    let logged = sides_target(endpoints, target_name);
    let answer = Answer::from((text, is_error));
    if let Some(connection) = &logged.audit_as {
        record(logged, connection, name, args, &answer);
    }
    json!({ "content": [ { "type": "text", "text": answer.text }], "isError": answer.is_error })
}

/// The endpoint a call's log line goes under: the one it named, else the
/// default.
fn sides_target<'a>(endpoints: &'a [Named<'a>], named: Option<&str>) -> &'a Endpoint {
    named
        .and_then(|w| endpoints.iter().find(|n| n.name == w))
        .unwrap_or(&endpoints[0])
        .endpoint
}

/// `list_connections`' answer.
fn list_connections(endpoints: &[Named<'_>]) -> String {
    let mut out = String::from("Connections on this server:\n");
    for (i, n) in endpoints.iter().enumerate() {
        let e = n.endpoint;
        out.push_str(&format!(
            "- {}{} — {}, default database {}; rows: {}\n",
            n.name,
            if i == 0 { " (the default)" } else { "" },
            e.db.engine().as_str(),
            e.database.as_deref().unwrap_or("(none)"),
            if e.samples {
                "you may read them"
            } else {
                "withheld"
            },
        ));
    }
    out
}

/// Who started this server — and so where a tool's result can lead.
///
/// The distinction is what the *reply* reaches. In the panel the app reads
/// every reply the model writes, which is what makes `propose_table_change`
/// worth having: a proposal echoed into a fenced block becomes a change
/// preview. Standalone, the client is someone else's and nothing of ours reads
/// the reply, so the tool would describe a preview that never appears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Host {
    /// `schemaic --mcp-serve`, spawned for the AI panel.
    Panel,
    /// `schemaic mcp -c <conn>`, spawned by any MCP client.
    Standalone,
}

/// What the server is pointed at, and what it may hand over.
pub struct Endpoint {
    pub db: Db,
    /// The default database for tool calls — `USE`d by `run_query`, and where
    /// `describe_table` and `propose_table_change` look when not told.
    pub database: Option<String>,
    /// May the assistant read **rows** on its own — the connection's
    /// [`AiData::may_query`](schemaic_core::connection::AiData::may_query).
    /// With it off the server neither advertises nor answers `run_query`
    /// ([`tools_list`], [`refusal_for`]). `describe_table` is a schema tool the
    /// assistant keeps either way, but its sample-rows section reads real
    /// data — so that section is dropped rather than the whole tool.
    pub samples: bool,
    /// Databases the SCHEMA eye has hidden, as of the moment this session was
    /// spawned. `list_schema`'s server overview leaves them out — see
    /// [`listed_databases`] for which half of that tool they affect and why the
    /// other half is answered in full. Empty standalone: `schemaic databases`
    /// lists them all too, and the eye is the app's view state rather than
    /// anything saved on the connection.
    pub hidden: HashSet<String>,
    /// May the assistant read the **catalogue** — the app's *Schema context*
    /// setting, as of the moment this session was spawned.
    ///
    /// `false` is `SchemaScope::None`, where the system prompt carries no
    /// databases and no tables. Without it here the subprocess still advertised
    /// `list_schema`, whose first call hands back every database and every table
    /// name, so the setting was defeated in one call. Plumbed like `samples`
    /// rather than only into the prompt, for the same reason: a listing the
    /// model already holds must not reach the DB. Always `true` standalone,
    /// which has no system prompt to budget and whose `cli_access` already
    /// publishes the catalogue through `schemaic tables` and `describe`.
    pub schema: bool,
    pub host: Host,
    /// The connection's name, when every tool call is to be recorded in the
    /// agent log (`core::agent_log`) — set by the two front ends from the
    /// saved connection itself, `None` elsewhere, so a test's call writes no
    /// file.
    pub audit_as: Option<String>,
    /// Where an audited call's line goes instead of `agent_log.jsonl` — a
    /// test's vector, so that *whether and under which connection* a call is
    /// recorded can be pinned without writing the user's log. `None` writes
    /// the file.
    pub audit_sink: Option<AuditSink>,
    /// Is the connection marked read-only — what `propose_sql` judges a
    /// statement against, as the panel's Run will. From the saved connection,
    /// never the blob; `false` where nothing has said.
    pub read_only: bool,
    /// Is the AI data level `--ai-data`'s rather than a saved setting's — a
    /// `schemaic mcp --url` connection, which was never saved and so has no
    /// connection settings to change. What a refusal advises turns on it
    /// ([`refusal_on`]); whether it refuses does not.
    pub level_from_flag: bool,
}

impl Endpoint {
    /// The endpoint `schemaic mcp` serves for a saved connection, whose
    /// `ai_data` is `ai_data`.
    ///
    /// **Rows only at [`AiData::Full`](schemaic_core::connection::AiData::Full)**
    /// — `may_query`, the panel's own gate. `OnRequest` is the level whose
    /// rows reach a model only by the user's attach gesture, and an outside
    /// client has none, so it sends none rather than being promoted.
    ///
    /// **`None` is the default level, not the migration's answer.** The app
    /// resolves a never-chosen level at startup from the legacy global
    /// "run queries" flag, which defaulted *on*; the CLI cannot read that
    /// flag, and guessing `Full` would hand rows to an agent on a connection
    /// whose owner was never asked.
    ///
    /// **[`Db::implied_database`] comes first**, where the panel would have
    /// passed the tree's selection: a SQLite file's `main` is the database the
    /// user is in whether or not anyone named it — and, `catalog.rs`'s rule,
    /// whatever `-d` or `SCHEMAIC_DATABASE` says, since every operation opens
    /// its own connection and nothing else is ever attached.
    pub fn standalone(
        db: Db,
        database: Option<String>,
        ai_data: Option<schemaic_core::connection::AiData>,
    ) -> Endpoint {
        let database = db.implied_database().map(str::to_string).or(database);
        Endpoint {
            db,
            database,
            samples: ai_data.unwrap_or_default().may_query(),
            hidden: HashSet::new(),
            schema: true,
            host: Host::Standalone,
            audit_as: None,
            audit_sink: None,
            // `propose_sql` is the one reader, and it is panel-only.
            read_only: false,
            level_from_flag: false,
        }
    }

    /// The same endpoint, its level marked as `--ai-data`'s when `yes`.
    pub fn with_level_from_flag(mut self, yes: bool) -> Endpoint {
        self.level_from_flag = yes;
        self
    }

    /// The same endpoint, its calls recorded in the agent log under the
    /// connection named `name`.
    pub fn audited_as(mut self, name: &str) -> Endpoint {
        self.audit_as = Some(name.to_string());
        self
    }
}

/// Run the stdio JSON-RPC loop until stdin closes, against the resolved
/// endpoint (its `database` is the default schema `USE`d for `run_query`).
///
/// **Its one connection is named after the saved one** (`audit_as`), so a
/// model that names the active connection — the panel's prompt says which —
/// reaches it rather than a refusal listing nothing.
pub async fn serve(endpoint: Endpoint) {
    let name = sole_name(&endpoint);
    serve_on(
        &[Named {
            name: &name,
            endpoint: &endpoint,
        }],
        BufReader::new(tokio::io::stdin()),
        tokio::io::stdout(),
    )
    .await;
}

/// The name [`serve`] gives its one connection: the saved connection's.
fn sole_name(endpoint: &Endpoint) -> String {
    endpoint.audit_as.clone().unwrap_or_default()
}

/// [`serve`] for several connections, each under its name; the first is the
/// default for a call that names none ([`listing`]).
pub async fn serve_many(endpoints: Vec<(String, Endpoint)>) {
    let named: Vec<Named<'_>> = endpoints
        .iter()
        .map(|(name, endpoint)| Named { name, endpoint })
        .collect();
    serve_on(
        &named,
        BufReader::new(tokio::io::stdin()),
        tokio::io::stdout(),
    )
    .await;
}

/// [`serve`] over any reader and writer, so the protocol loop itself — not
/// just the pure pieces it calls — is tested.
async fn serve_on<R, W>(endpoints: &[Named<'_>], input: R, mut stdout: W)
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut input = input;
    let mut raw = Vec::new();
    // What `initialize` agreed on. The oldest until then: a client that lists
    // tools before the handshake gets nothing it might not understand.
    let mut protocol = SUPPORTED_PROTOCOLS[SUPPORTED_PROTOCOLS.len() - 1];
    loop {
        // **Bytes, then UTF-8, one line at a time.** `lines()` returns an
        // error for a line that is not UTF-8, and `while let Ok(Some(..))`
        // took that for the end of input: the server exited 0 mid-session as
        // though the client had hung up. JSON is UTF-8 by definition, so such
        // a line is no request — it is skipped, and said on stderr.
        raw.clear();
        match input.read_until(b'\n', &mut raw).await {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => {
                eprintln!("schemaic mcp: stopped reading requests: {e}");
                break;
            }
        }
        let Ok(line) = std::str::from_utf8(&raw) else {
            eprintln!("schemaic mcp: skipped a request line that is not UTF-8");
            continue;
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // **Not a request is still owed an answer.** A line that does not
        // parse, or parses to something other than an object (a batch — no
        // served revision accepts one), got nothing at all, and the client
        // waited on its id to its own timeout. JSON-RPC's reply has a null id,
        // since none could be read; the session goes on.
        let refused = match serde_json::from_str::<Value>(line) {
            Ok(req) if req.is_object() => Ok(req),
            Ok(_) => Err((
                INVALID_REQUEST,
                "Invalid Request: expected one JSON-RPC object",
            )),
            Err(_) => Err((PARSE_ERROR, "Parse error: the line is not JSON")),
        };
        let req = match refused {
            Ok(req) => req,
            Err((code, message)) => {
                eprintln!("schemaic mcp: {message}");
                let msg = json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": { "code": code, "message": message }
                });
                let _ = stdout.write_all(format!("{msg}\n").as_bytes()).await;
                let _ = stdout.flush().await;
                continue;
            }
        };
        let id = req.get("id").cloned();
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");

        let result: Option<Value> = match method {
            "initialize" => {
                protocol = negotiate_protocol(
                    req.pointer("/params/protocolVersion")
                        .and_then(|v| v.as_str()),
                );
                Some(json!({
                    "protocolVersion": protocol,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "schemaic", "version": env!("CARGO_PKG_VERSION") }
                }))
            }
            "tools/list" => Some(json!({
                "tools": annotated(listing(endpoints), protocol)
            })),
            "tools/call" => {
                let name = req
                    .pointer("/params/name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let args = req
                    .pointer("/params/arguments")
                    .cloned()
                    .unwrap_or(json!({}));
                let error = |text: String| json!({ "content": [ { "type": "text", "text": text } ], "isError": true });
                if name == LIST_CONNECTIONS && endpoints.len() > 1 {
                    // Logged like any call, under the default connection's
                    // name: the agent log is "every call", not every tool.
                    let first = endpoints[0].endpoint;
                    if let Some(connection) = &first.audit_as {
                        record(
                            first,
                            connection,
                            &name,
                            &args,
                            &(String::new(), false).into(),
                        );
                    }
                    Some(json!({
                        "content": [ { "type": "text", "text": list_connections(endpoints) } ],
                        "isError": false
                    }))
                } else if cross_connection_source(&name, &args).is_some() {
                    Some(compare_across(endpoints, &name, &args).await)
                } else {
                    match pick(endpoints, &args) {
                        Ok(endpoint) => Some(call_tool(endpoint, &name, &args).await),
                        Err(why) => {
                            // Refused before any connection was chosen, so
                            // logged under the default's name, as
                            // `list_connections` is.
                            let first = endpoints[0].endpoint;
                            if let Some(connection) = &first.audit_as {
                                let refused = (why.clone(), true).into();
                                record(first, connection, &name, &args, &refused);
                            }
                            Some(error(why))
                        }
                    }
                }
            }
            "ping" => Some(json!({})),
            // Anything else is not implemented here. A notification (no id)
            // is owed nothing; a request is owed JSON-RPC's "method not
            // found" rather than an empty result, which reads as success —
            // `resources/list` answered `{}` has no `resources`, and a strict
            // client fails its schema check and marks the server broken.
            _ => None,
        };

        let Some(id) = id else { continue };
        let msg = match result {
            Some(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            None => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": METHOD_NOT_FOUND, "message": format!("Method not found: {method}") }
            }),
        };
        let _ = stdout.write_all(format!("{msg}\n").as_bytes()).await;
        let _ = stdout.flush().await;
    }
}

/// JSON-RPC 2.0's code for a method the server does not implement.
const METHOD_NOT_FOUND: i64 = -32601;
/// JSON-RPC's code for a line that is not JSON.
const PARSE_ERROR: i64 = -32700;
/// JSON-RPC's code for JSON that is not one request object.
const INVALID_REQUEST: i64 = -32600;

/// The tools this server offers — **the set, as a type**.
///
/// The four names used to be written out independently in five places that the
/// compiler related to nothing: the `json!` array in [`tools_list`], a `==` in
/// `reads_row_data`, a `matches!` in `reads_schema`, [`call_tool`]'s dispatch
/// with its `other =>` catch-all, and `ai.rs`'s two `mcp__schemaic__*` lists. A
/// new tool therefore joined the advertised array, joined the dispatch, and
/// **defaulted to ungated at both gates** — which is exactly how
/// `propose_table_change` came to sit on neither, with full `fetch_schema`
/// access at the app's tightest setting, nothing failing to compile and nothing
/// failing to run.
///
/// Every question about a tool is now an exhaustive `match` on this enum, so a
/// new variant does not compile until every gate has answered for it. Same
/// rule `ai/harness.rs`'s `env_seal` states one crate over — *"a fifth harness
/// is a compile error here rather than a silent default there"*.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum McpTool {
    RunQuery,
    Explain,
    SampleRows,
    ListDatabases,
    ListSchema,
    DescribeTable,
    SearchObjects,
    CompareSchemas,
    ProposeTableChange,
    ProposeSql,
    TopStatements,
    IndexHealth,
    TableSizes,
    CurrentLocks,
}

impl McpTool {
    /// Every tool, in the order `tools/list` advertises them.
    pub const ALL: [McpTool; 14] = [
        McpTool::RunQuery,
        McpTool::Explain,
        McpTool::SampleRows,
        McpTool::ListDatabases,
        McpTool::ListSchema,
        McpTool::DescribeTable,
        McpTool::SearchObjects,
        McpTool::CompareSchemas,
        McpTool::ProposeTableChange,
        McpTool::ProposeSql,
        McpTool::TopStatements,
        McpTool::IndexHealth,
        McpTool::TableSizes,
        McpTool::CurrentLocks,
    ];

    /// The name on the wire.
    pub fn name(self) -> &'static str {
        match self {
            McpTool::RunQuery => "run_query",
            McpTool::Explain => "explain",
            McpTool::SampleRows => "sample_rows",
            McpTool::ListDatabases => "list_databases",
            McpTool::ListSchema => "list_schema",
            McpTool::DescribeTable => "describe_table",
            McpTool::SearchObjects => "search_objects",
            McpTool::ProposeTableChange => "propose_table_change",
            McpTool::ProposeSql => "propose_sql",
            McpTool::CompareSchemas => "compare_schemas",
            McpTool::TopStatements => "top_statements",
            McpTool::IndexHealth => "index_health",
            McpTool::TableSizes => "table_sizes",
            McpTool::CurrentLocks => "current_locks",
        }
    }

    /// The name a CLI harness's allow-list spells it under. Written out rather
    /// than `format!`ed so it is a `&'static str` the allow-lists can hold —
    /// and so a new tool has to state it here too.
    pub fn ai_name(self) -> &'static str {
        match self {
            McpTool::RunQuery => "mcp__schemaic__run_query",
            McpTool::Explain => "mcp__schemaic__explain",
            McpTool::SampleRows => "mcp__schemaic__sample_rows",
            McpTool::ListDatabases => "mcp__schemaic__list_databases",
            McpTool::ListSchema => "mcp__schemaic__list_schema",
            McpTool::DescribeTable => "mcp__schemaic__describe_table",
            McpTool::SearchObjects => "mcp__schemaic__search_objects",
            McpTool::ProposeTableChange => "mcp__schemaic__propose_table_change",
            McpTool::ProposeSql => "mcp__schemaic__propose_sql",
            McpTool::CompareSchemas => "mcp__schemaic__compare_schemas",
            McpTool::TopStatements => "mcp__schemaic__top_statements",
            McpTool::IndexHealth => "mcp__schemaic__index_health",
            McpTool::TableSizes => "mcp__schemaic__table_sizes",
            McpTool::CurrentLocks => "mcp__schemaic__current_locks",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        McpTool::ALL.into_iter().find(|t| t.name() == name)
    }

    /// Does this tool read **row data**, and so exist only on a connection
    /// whose [`AiData`](schemaic_core::connection::AiData) level lets the
    /// assistant fetch rows for itself?
    ///
    /// One predicate, consulted twice — [`tools_list`] leaves the tool out and
    /// [`refusal_for`] turns a call to it away — so a tool can never be
    /// advertised by one rule and denied by another.
    pub fn reads_row_data(self) -> bool {
        match self {
            // **A plan is not free of the data.** MySQL's `EXPLAIN` reads a
            // `const` table to plan it, and answers "Impossible WHERE noticed
            // after reading const tables" exactly when no row has that key —
            // an existence oracle over values, one key per call. So `explain`
            // stands wherever `run_query` does and nowhere else.
            McpTool::RunQuery | McpTool::Explain | McpTool::SampleRows => true,
            // **Other people's statements are their data.** PostgreSQL and
            // MySQL replace literals with placeholders, but SQL Server's Query
            // Store keeps the text as sent — `WHERE email = 'x@y'` and all.
            McpTool::TopStatements => true,
            // A lock row names what is locked, and on MySQL that is the
            // record's key values (`data_locks.LOCK_DATA`) — one row's worth
            // of data per lock.
            McpTool::CurrentLocks => true,
            McpTool::ListDatabases
            | McpTool::ListSchema
            | McpTool::DescribeTable
            | McpTool::SearchObjects
            | McpTool::ProposeTableChange
            | McpTool::ProposeSql
            | McpTool::CompareSchemas
            // Names and sizes — the catalogue, not the rows.
            | McpTool::IndexHealth
            | McpTool::TableSizes => false,
        }
    }

    /// Does this tool read the **catalogue** — the thing *Schema context*
    /// governs?
    ///
    /// **`propose_table_change` answers yes, and its `false` was a hole.** The
    /// exclusion was justified as "it carries the table it is about in the call
    /// and reads nothing the model did not already have", and both halves were
    /// false: [`propose_change`] calls `db.fetch_schema(database, …)`, a full
    /// catalogue read of an arbitrary database, and `propose::resolve_target`'s
    /// hit/miss split is a working existence oracle over every database and
    /// table name a model cares to guess — at the one setting whose entire
    /// purpose is to publish none of them. On SQLite it is not even an oracle:
    /// `AddCheck` is off the native allowlist, so `ddl::diff` collapses the set
    /// into one `RebuildTable`, and the emitted SQL is the table's complete
    /// `CREATE TABLE` followed by an `INSERT … SELECT` naming every column
    /// again. One call on a guessed name returned the whole declaration.
    ///
    /// The hidden-database set is a separate matter and still unconsulted on
    /// this path — it reaches only `listed_databases`.
    pub fn reads_schema(self) -> bool {
        match self {
            // A plan names the tables and indexes of a statement the model
            // wrote — what `run_query` on the same text would tell it.
            // `propose_sql` reads nothing at all: the guard is text only.
            McpTool::RunQuery | McpTool::Explain | McpTool::ProposeSql => false,
            // A name search is the existence oracle `propose_table_change`'s
            // note above describes, asked on purpose. `sample_rows` resolves
            // its table against the catalogue, which is the same oracle, and
            // `list_databases` is the overview's first half.
            McpTool::SampleRows
            | McpTool::ListDatabases
            | McpTool::ListSchema
            | McpTool::DescribeTable
            | McpTool::SearchObjects
            | McpTool::ProposeTableChange
            // Two whole catalogues, and a script naming every object in them.
            | McpTool::CompareSchemas
            // Every table and index name in a database, ranked — the
            // overview `list_schema` gives, with sizes. The statements and
            // the locks name tables too.
            | McpTool::IndexHealth
            | McpTool::TableSizes
            | McpTool::TopStatements
            | McpTool::CurrentLocks => true,
        }
    }

    /// Does this tool leave the database exactly as it found it — the
    /// `readOnlyHint` a `2025` session is told ([`annotated`])?
    ///
    /// All of them do, and that is the server's whole shape rather than a
    /// coincidence: `run_query` goes through `read_only_query` on a read-only
    /// session and `sample_rows` reads on one too, `explain` plans without the
    /// analysing form that would run the statement, `search_objects`,
    /// `list_databases` and the other listings read only the catalogue, and
    /// `propose_table_change` and `propose_sql` run nothing — the DDL or the statement goes to the user's
    /// preview. Asked per tool all the same, so a new one has to answer.
    pub fn read_only(self) -> bool {
        match self {
            McpTool::RunQuery
            | McpTool::Explain
            | McpTool::SampleRows
            | McpTool::ListDatabases
            | McpTool::ListSchema
            | McpTool::DescribeTable
            | McpTool::SearchObjects
            | McpTool::ProposeTableChange
            | McpTool::ProposeSql
            // The script it hands back is text; nothing runs it here.
            | McpTool::CompareSchemas
            // Catalogue and statistics views only, and an index suggestion
            // comes back as DDL to propose, never run.
            | McpTool::TopStatements
            | McpTool::IndexHealth
            | McpTool::TableSizes
            | McpTool::CurrentLocks => true,
        }
    }

    /// Does this tool only mean something when the **app reads the reply** —
    /// [`Host::Panel`]?
    ///
    /// `propose_table_change` does: its whole result is an instruction to echo
    /// the proposal into a fenced block "and Schemaic turns that into a change
    /// preview". Under `schemaic mcp` no Schemaic is reading, so the model
    /// would be told the user has a preview to review when they have a code
    /// block — and an assistant that believes the user reviewed a change is
    /// the one that stops describing it.
    pub fn needs_panel(self) -> bool {
        match self {
            // `propose_sql`'s answer tells the model to hand the statement over
            // in a ```sql block, which the panel gives Insert and Run; outside
            // it, a code block is all there is.
            McpTool::ProposeTableChange | McpTool::ProposeSql => true,
            McpTool::RunQuery
            | McpTool::Explain
            | McpTool::SampleRows
            | McpTool::ListDatabases
            | McpTool::ListSchema
            | McpTool::DescribeTable
            | McpTool::SearchObjects
            | McpTool::CompareSchemas
            | McpTool::TopStatements
            | McpTool::IndexHealth
            | McpTool::TableSizes
            | McpTool::CurrentLocks => false,
        }
    }

    /// Is this tool available at this connection's levels, from this host? The
    /// one place the gates are combined, so the listing, the refusal and the
    /// panel's allow-list cannot disagree about what "available" means.
    pub fn offered(self, reads_data: bool, schema: bool, host: Host) -> bool {
        (reads_data || !self.reads_row_data())
            && (schema || !self.reads_schema())
            && (host == Host::Panel || !self.needs_panel())
    }
}

/// What the server says when a schema tool is called at *Schema context: None*.
///
/// Names the setting, because the model's next move should be to ask the user
/// for the table and column names (or to raise the setting) rather than retry.
const NO_SCHEMA_ACCESS: &str = "Refused: the user's Schema context setting is None, so the \
     assistant is given no database structure and list_databases, list_schema, describe_table, \
     search_objects, sample_rows, compare_schemas, propose_table_change and the performance tools \
     (top_statements, index_health, table_sizes, current_locks) are unavailable. Ask the user for \
     the table and column names you need \
     — or tell them to set Schema context to a database in AI settings.";

/// What the server says when a withheld tool is called anyway. It names the
/// setting, because the model's next move should be to ask the user for the
/// values (or to change the level) rather than to retry the same call.
const NO_DATA_ACCESS: &str = "Refused: this connection's AI data access is set so the assistant \
     reads no rows on its own, so run_query, explain, sample_rows, top_statements and \
     current_locks are unavailable. Ask the user for the values you need \
     — they can attach rows from a result grid, or raise the connection's AI data access \
     setting.";

/// [`NO_DATA_ACCESS`] for [`Host::Standalone`]. The panel's advice — attach
/// rows from a result grid — names a gesture this client does not have, so a
/// model passing it on would send the user looking for a grid that is not
/// there.
const NO_DATA_ACCESS_STANDALONE: &str = "Refused: this connection's AI data access, set in \
     Schemaic, lets no assistant read rows on its own, so run_query, explain, sample_rows, \
     top_statements and current_locks are unavailable. Ask the \
     user \
     for the values you need — or ask them to set the connection's AI data access to \"Let it \
     read data\" in Schemaic's connection settings and restart this server.";

/// [`NO_DATA_ACCESS_STANDALONE`] for a `--url` connection. It was never saved,
/// so there are no connection settings to send the user to: its level is the
/// command line's.
const NO_DATA_ACCESS_URL: &str = "Refused: this connection was given to `schemaic mcp` as a \
     --url, whose AI data access lets no assistant read rows on its own unless the command \
     says otherwise, so run_query, explain, sample_rows, top_statements and current_locks are \
     unavailable. Ask the user for the \
     values you need — or ask them to add --ai-data full to the server's command line and \
     restart it.";

/// What the server says when a panel-only tool is called from outside the
/// panel. Says what to do instead, for the reason the other two do.
const NO_PANEL: &str = "Refused: propose_table_change and propose_sql hand their result to \
     Schemaic's AI panel, which turns it into a change preview or a statement to insert and run, \
     and this server was started outside the panel by `schemaic mcp`. Describe the change to the \
     user instead — list_schema and describe_table have the table's current definition.";

/// The refusal a tool call earns from the connection's levels and the host,
/// if any. Pure, so the gate is unit-tested without a live endpoint.
fn refusal_for(tool: McpTool, reads_data: bool, schema: bool, host: Host) -> Option<&'static str> {
    if !reads_data && tool.reads_row_data() {
        return Some(match host {
            Host::Panel => NO_DATA_ACCESS,
            Host::Standalone => NO_DATA_ACCESS_STANDALONE,
        });
    }
    if host != Host::Panel && tool.needs_panel() {
        return Some(NO_PANEL);
    }
    (!schema && tool.reads_schema()).then_some(NO_SCHEMA_ACCESS)
}

/// [`refusal_for`] on endpoint `e` — what every call site asks, so which
/// field is passed as which argument is decided once. A level set by
/// `--ai-data` ([`Endpoint::level_from_flag`]) changes the data refusal's
/// advice to [`NO_DATA_ACCESS_URL`], and nothing else.
pub(crate) fn refusal_on(e: &Endpoint, tool: McpTool) -> Option<&'static str> {
    match refusal_for(tool, e.samples, e.schema, e.host) {
        Some(NO_DATA_ACCESS_STANDALONE) if e.level_from_flag => Some(NO_DATA_ACCESS_URL),
        why => why,
    }
}

/// The tools this server offers, described for **this** connection's engine.
///
/// `run_query`'s advertised statement heads come from
/// [`schemaic_core::sql::read_only_heads`], the same list the gate enforces — a
/// hard-coded `SELECT/SHOW/DESCRIBE/EXPLAIN/WITH` told every model that a SQLite
/// connection accepted `SHOW TABLES` and `DESCRIBE t`, which SQLite has no syntax
/// for, so a model reasoning from the tool's own text would spend turns on
/// statements that could only ever come back as parser errors.
///
/// `reads_data` is the connection's level: with it off, the tools that read rows
/// are not advertised at all. Listing a tool and then denying the call is worse
/// than not listing it — the model plans a turn around the tool it can see,
/// offers the user an analysis it cannot deliver, and only discovers the denial
/// once the user has said yes.
///
/// `schema` is the app's *Schema context* setting, and it withholds the
/// catalogue tools the same way. With the setting at `None` the system prompt
/// carries no databases and no tables — and the model held `list_schema`, whose
/// first call hands back every database and every table name. Whether that
/// setting is read as a token budget or as consent, a budget of zero that one
/// tool call walks around is neither.
///
/// `host` withholds the tools whose result only the panel can act on
/// ([`McpTool::needs_panel`]).
pub fn tools_list(
    engine: schemaic_db::Engine,
    reads_data: bool,
    schema: bool,
    host: Host,
) -> Value {
    let dialect = engine.dialect();
    let heads = schemaic_core::sql::read_only_heads(dialect).join("/");
    let mut tools = json!([
        {
            "name": "run_query",
            "description": format!(
                "Execute ONE read-only SQL statement ({heads} only) against the user's \
                 database connection and return the rows. The connection is {}, so write it in \
                 that dialect. Read-only guards against mistakes and keeps you from writing; it \
                 is not a security boundary against a database whose owner is hostile.",
                dialect.engine_label(),
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "sql": { "type": "string", "description": "A single read-only SQL statement." },
                    "database": {
                        "type": "string",
                        "description": "The database to run in, as list_schema names it. Defaults \
                                        to the connection's active database."
                    }
                },
                "required": ["sql"]
            }
        },
        {
            "name": "explain",
            "description": format!(
                "Show the query plan for ONE read-only query (a SELECT, or WITH … SELECT) — which \
                 indexes it uses, where it scans, sorts or spills — followed by the likely \
                 performance problems. The statement is planned, never executed, so the plan \
                 carries the optimiser's estimates rather than measured times. The connection \
                 is {}, so write it in that dialect.",
                dialect.engine_label(),
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "sql": {
                        "type": "string",
                        "description": "The statement to plan, without EXPLAIN in front."
                    },
                    "database": {
                        "type": "string",
                        "description": "The database to plan in, as list_schema names it. \
                                        Defaults to the connection's active database."
                    }
                },
                "required": ["sql"]
            }
        },
        {
            "name": "sample_rows",
            "description": format!(
                "Read the first rows of one table or view, every column, without writing the \
                 query — to see what the values look like. Up to {SAMPLE_ROWS_MAX} rows; \
                 {SAMPLE_ROWS_DEFAULT} unless `limit` says otherwise. Unordered, so they are \
                 a sample, not the top of anything: for a filter or an order, use run_query."
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "table": {
                        "type": "string",
                        "description": "Table name, exactly as list_schema printed it — including \
                                        the `schema.` prefix when it showed one."
                    },
                    "limit": {
                        "type": "integer",
                        "description": format!("How many rows, 1 to {SAMPLE_ROWS_MAX}.")
                    },
                    "database": {
                        "type": "string",
                        "description": "Defaults to the connection's active database."
                    }
                },
                "required": ["table"]
            }
        },
        {
            "name": "list_databases",
            "description": "List the databases on the active connection, by name only — the \
                            cheap first look on a server with many of them. list_schema with \
                            `database` then shows one in full.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "list_schema",
            "description": "List what's on the active connection. With no arguments: every database \
                            and its table names. With `database`: that database's tables in full — \
                            columns with types, PK and NOT NULL markers, and foreign-key edges.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "database": {
                        "type": "string",
                        "description": "Limit to one database and include its columns and keys."
                    }
                }
            }
        },
        {
            "name": "describe_table",
            // The sample rows are promised only where they are sent: a model
            // told to expect them plans its turn around rows that never come.
            "description": format!(
                "One table in depth: its CREATE TABLE (or CREATE VIEW) definition including \
                 indexes{}",
                if reads_data {
                    ", its foreign keys, and a few sample rows."
                } else {
                    " and its foreign keys. No rows: this connection sends the assistant none."
                },
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "table": {
                        "type": "string",
                        "description": "Table name, exactly as list_schema printed it — including \
                                        the `schema.` prefix when it showed one."
                    },
                    "database": {
                        "type": "string",
                        "description": "Defaults to the connection's active database."
                    }
                },
                "required": ["table"]
            }
        },
        {
            "name": "search_objects",
            "description": "Find tables and views by part of their name, case-insensitively, \
                            across every database list_schema shows. With `database`: that one \
                            database, and its column names too, printed as `table.column`.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Part of a name — `cust` finds `customers` and \
                                        `v_customer_orders`."
                    },
                    "database": {
                        "type": "string",
                        "description": "Search only this database, and its columns as well."
                    }
                },
                "required": ["query"]
            }
        },
        {
            "name": "compare_schemas",
            "description": format!(
                "Compare two {} databases' structure and get the SQL that would make one match \
                 the other — tables, columns, indexes, keys, views, routines and triggers. \
                 **This runs nothing**: it reads both catalogues and hands back the script, with \
                 what in it is destructive and what it leaves out. `database` is the one the \
                 script would change, `source_database` the one it should come to match; on a \
                 server with several connections, `source_connection` may name another. Show \
                 the script to the user; never present it as applied.",
                dialect.engine_label(),
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "database": {
                        "type": "string",
                        "description": "The database the script would change. Defaults to the connection's active database."
                    },
                    "source_database": {
                        "type": "string",
                        "description": "The database to match. Defaults to the source connection's active database."
                    },
                    "source_connection": {
                        "type": "string",
                        "description": "On a server with several connections, the one holding source_database. Defaults to the same connection."
                    },
                    "tables": {
                        "type": "array",
                        "description": "Only these tables or views, and the triggers on them. Every difference when absent.",
                        "items": { "type": "string" }
                    }
                }
            }
        },
        {
            "name": "propose_table_change",
            "description": format!(
                "Check a proposed change to a table and see the {} SQL it would produce. \
                 **This runs nothing and changes nothing** — it reads the table, applies your ops \
                 to it, and hands back the DDL plus anything destructive in it. Call it before \
                 offering a schema change so you find out here, not in front of the user, that a \
                 column is spelled differently than you thought. When it comes back valid, put the \
                 *same* JSON in a ```{} fenced block in your reply: Schemaic turns that into a \
                 change preview the user reviews and applies themselves. Never write the DDL out \
                 for them to run.\n\n\
                 Ops are a patch — what you don't name, you don't touch, so send only what should \
                 change. One op per object: \
                 {{\"add_column\": {{\"name\", \"type\", \"nullable\"?, \"default\"?, \"comment\"?, \
                 \"auto_increment\"?, \"generated\"?}}}}, \
                 {{\"alter_column\": {{\"name\", \"type\"?, \"nullable\"?, \"default\"?, \
                 \"drop_default\"?, \"comment\"?}}}}, \
                 {{\"drop_column\": {{\"name\"}}}}, {{\"rename_column\": {{\"from\", \"to\"}}}}, \
                 {{\"set_primary_key\": {{\"columns\": []}}}}, \
                 {{\"add_index\": {{\"columns\": [], \"name\"?, \"unique\"?}}}}, \
                 {{\"drop_index\": {{\"name\"}}}}, \
                 {{\"add_foreign_key\": {{\"columns\": [], \"ref_table\", \"ref_columns\": [], \
                 \"name\"?, \"ref_schema\"?, \"on_delete\"?, \"on_update\"?}}}}, \
                 {{\"drop_foreign_key\": {{\"name\"}}}}, \
                 {{\"add_check\": {{\"expression\", \"name\"?}}}}, {{\"drop_check\": {{\"name\"}}}}, \
                 {{\"rename_table\": {{\"to\"}}}}. \
                 Types are written the way this server writes them ({}). Unknown keys are refused \
                 rather than ignored.",
                dialect.engine_label(),
                schemaic_core::propose::FENCE_TAG,
                dialect.engine_label(),
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "table": {
                        "type": "string",
                        "description": "The table to change, as list_schema printed it."
                    },
                    "database": {
                        "type": "string",
                        "description": "Defaults to the connection's active database."
                    },
                    "schema": {
                        "type": "string",
                        "description": "PostgreSQL namespace, when the table is outside the default."
                    },
                    "summary": {
                        "type": "string",
                        "description": "One line on what this change is for, shown above the preview."
                    },
                    "ops": {
                        "type": "array",
                        "description": "The changes to make, applied in order.",
                        "items": { "type": "object" }
                    }
                },
                "required": ["table", "ops"]
            }
        },
        {
            "name": "propose_sql",
            "description": format!(
                "Check a {} statement you mean to offer the user — an UPDATE, an INSERT, a fix to \
                 their query — against Schemaic's write guard. **This runs nothing.** It says \
                 whether the statement would be refused outright (a read-only connection, no \
                 database), or whether Schemaic will ask the user before running it (a DELETE or \
                 UPDATE with no WHERE, a DROP). Call it before proposing anything that writes. \
                 When it comes back valid, put the statement in a ```sql block in your reply: the \
                 user can Insert it into a query tab or Run it. For a change to a table's \
                 structure, use propose_table_change instead.",
                dialect.engine_label(),
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "sql": {
                        "type": "string",
                        "description": "The statement, or statements, to check."
                    },
                    "database": {
                        "type": "string",
                        "description": "Where it would run. Defaults to the connection's active database."
                    }
                },
                "required": ["sql"]
            }
        },
        {
            "name": "top_statements",
            "description": format!(
                "The statements this {} server has spent the most time on since its counters \
                 were last reset — total and mean milliseconds, calls and rows, from {}. Ranked \
                 by total time unless `order` says otherwise. When the source is off, the answer \
                 says which and what would turn it on; pass that on to the user rather than \
                 trying to enable it. Read-only.",
                dialect.engine_label(),
                statement_source(dialect),
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "order": {
                        "type": "string",
                        "enum": ["total", "mean", "calls"],
                        "description": "Rank by total time (default), mean time per call, or call count."
                    },
                    "limit": {
                        "type": "integer",
                        "description": format!(
                            "How many statements, 1 to {}. Defaults to 20.",
                            schemaic_core::workload::MAX_STATEMENT_LIMIT
                        )
                    },
                    "database": {
                        "type": "string",
                        "description": "Where the statistics source lives. Defaults to the connection's active database."
                    }
                }
            }
        },
        {
            "name": "index_health",
            "description": format!(
                "One database's index problems: indexes nothing has read since the counters were \
                 last reset, exact duplicates, and indexes whose key is a leading prefix of \
                 another's{}. Read-only: any index it suggests comes back as DDL to propose to \
                 the user, never run. Usage counts reset with the server, so say \"unused \
                 since …\", never \"never used\".",
                if schemaic_core::workload::supports_missing_index_hints(dialect) {
                    " — plus the indexes the optimiser recorded as missing, each with its \
                     CREATE INDEX"
                } else {
                    ""
                },
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "database": {
                        "type": "string",
                        "description": "Defaults to the connection's active database."
                    }
                }
            }
        },
        {
            "name": "table_sizes",
            "description": "One database's largest tables (data plus indexes) and largest \
                            indexes, ranked by bytes, with row estimates. Read-only.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "database": {
                        "type": "string",
                        "description": "Defaults to the connection's active database."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "How many of each, 1 to 100. Defaults to 20."
                    }
                }
            }
        },
        {
            "name": "current_locks",
            "description": "The locks this server's sessions hold and wait for right now, and \
                            which sessions block which. Read-only: to end a session, tell the \
                            user which one — Schemaic's Server Activity panel can kill it.",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ]);
    if let Some(list) = tools.as_array_mut() {
        // **Through the enum, so an entry the set does not know is dropped.**
        // A `json!` array is not a type; matching each advertised name back to
        // an `McpTool` is what makes the array and the gates one set rather
        // than two that happen to agree today.
        list.retain(|t| {
            McpTool::from_name(t["name"].as_str().unwrap_or(""))
                .is_some_and(|tool| tool.offered(reads_data, schema, host))
        });
        // **Every tool reads rows or the catalogue**, so each says what its
        // results are (security review S3): a row value or a comment phrased
        // as an instruction is data, and the model is told so where it learns
        // the tool, once, rather than around every result the panel shows.
        for t in list.iter_mut() {
            if let Some(d) = t["description"].as_str() {
                t["description"] =
                    Value::String(format!("{d} {}", schemaic_core::prompt::TOOL_RESULT_NOTE));
            }
        }
    }
    tools
}

/// The database a tool call runs against: its own `database` argument when it
/// names one, else the endpoint's default. Pure, so the precedence is tested.
fn tool_database<'a>(args: &'a Value, default: Option<&'a str>) -> Option<&'a str> {
    args.get("database")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or(default)
}

/// One `tools/call`: the tool's answer, recorded in the agent log when the
/// endpoint says so — refusals and unknown names included, since a call an
/// agent was refused is as much a part of what it tried as one it was given.
async fn call_tool(endpoint: &Endpoint, name: &str, args: &Value) -> Value {
    let a = Box::pin(answer(endpoint, name, args)).await;
    if let Some(connection) = &endpoint.audit_as {
        record(endpoint, connection, name, args, &a);
    }
    json!({ "content": [ { "type": "text", "text": a.text } ], "isError": a.is_error })
}

/// A tool's answer: its text, whether it is an error, and — for an answer
/// that showed the agent rows — how many, which the agent log keeps.
#[derive(Debug)]
struct Answer {
    text: String,
    is_error: bool,
    rows: Option<u64>,
}

impl Answer {
    /// An answer that showed `rows` rows.
    fn with_rows(text: String, rows: usize) -> Answer {
        Answer {
            text,
            is_error: false,
            rows: Some(rows as u64),
        }
    }
}

impl From<(String, bool)> for Answer {
    fn from((text, is_error): (String, bool)) -> Answer {
        Answer {
            text,
            is_error,
            rows: None,
        }
    }
}

/// The agent-log line for one call, appended — to the endpoint's
/// [`AuditSink`] where it has one, else to the file; a failure to write is
/// said on stderr, which is the server's log, and never fails the call.
fn record(endpoint: &Endpoint, connection: &str, name: &str, args: &Value, answer: &Answer) {
    let line = audit_entry(endpoint, connection, name, args, answer);
    if let Some(sink) = &endpoint.audit_sink {
        sink.lock().unwrap_or_else(|p| p.into_inner()).push(line);
        return;
    }
    let keep =
        schemaic_core::persist::load_json::<schemaic_core::persist::UiState>("ui_state.json")
            .write_log_values;
    if let Err(e) = schemaic_core::agent_log::append(&line, dialect_of(&endpoint.db), keep) {
        eprintln!("schemaic: couldn't append to the agent log: {e}");
    }
}

/// [`record`]'s line: the tool, the database it ran in (the call's own or the
/// endpoint's), its SQL, what else it named — a table, a search, or the
/// other side of a comparison — an error's text, never a successful answer's,
/// which is the data the agent was shown, and how many rows that answer held.
/// The text is masked, then cut, by `agent_log::line`.
fn audit_entry(
    endpoint: &Endpoint,
    connection: &str,
    name: &str,
    args: &Value,
    answer: &Answer,
) -> schemaic_core::write_log::Entry {
    use schemaic_core::agent_log::{Caller, entry};
    let arg = |key: &str| {
        args.get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
    };
    let caller = match endpoint.host {
        Host::Panel => Caller::Panel,
        Host::Standalone => Caller::Standalone,
    };
    // A comparison read a second catalogue, which the line has to name: the
    // other connection's, or this one's other database.
    let compared = (name == McpTool::CompareSchemas.name()).then(|| {
        let side = arg("source_connection").unwrap_or(connection);
        match arg("source_database") {
            Some(db) => format!("from {side}.{db}"),
            None => format!("from {side}"),
        }
    });
    let subject = arg("table").or(arg("query")).or(compared.as_deref());
    entry(
        caller,
        connection,
        tool_database(args, endpoint.database.as_deref()),
        name,
        arg("sql").unwrap_or(""),
        subject,
        answer.is_error.then_some(answer.text.as_str()),
        answer.rows.filter(|_| !answer.is_error),
    )
}

/// A tool's answer — its text, and whether it is an error.
async fn answer(endpoint: &Endpoint, name: &str, args: &Value) -> Answer {
    let Endpoint {
        db,
        database,
        samples: reads_data,
        hidden,
        // The three gates are `refusal_on`'s, which reads them off `endpoint`.
        schema: _,
        host: _,
        audit_as: _,
        audit_sink: _,
        // `propose_policy`'s, which reads it off `endpoint`.
        read_only: _,
        level_from_flag: _,
    } = endpoint;
    let reads_data = *reads_data;
    let database = database.as_deref();
    // The level gates the call as well as the listing: a client working from a
    // stale `tools/list` must not reach the DB through a tool this connection
    // withheld.
    let Some(tool) = McpTool::from_name(name) else {
        return (format!("Unknown tool: {name}"), true).into();
    };
    if let Some(why) = refusal_on(endpoint, tool) {
        return (why.to_string(), true).into();
    }
    let arg = |key: &str| {
        args.get(key)
            .and_then(|s| s.as_str())
            .filter(|s| !s.is_empty())
    };
    // Exhaustive, with no catch-all: the unknown name was refused above, so a
    // new variant fails to compile here rather than falling into an arm
    // written for something else.
    let answered: (String, bool) = match tool {
        // The three that show rows answer with their count.
        McpTool::RunQuery => {
            let sql = args.get("sql").and_then(|s| s.as_str()).unwrap_or("");
            return run_query(db, tool_database(args, database), sql).await;
        }
        McpTool::SampleRows => {
            return match (arg("table"), sample_limit(args)) {
                (None, _) => ("sample_rows needs a `table`.".to_string(), true).into(),
                (_, Err(e)) => (e, true).into(),
                (Some(table), Ok(n)) => {
                    Box::pin(sample_rows(db, database, arg("database"), table, n)).await
                }
            };
        }
        McpTool::DescribeTable => {
            return match arg("table") {
                Some(table) => {
                    describe_table(db, database, arg("database"), table, reads_data).await
                }
                None => ("describe_table needs a `table`.".to_string(), true).into(),
            };
        }
        McpTool::Explain => {
            let sql = args.get("sql").and_then(|s| s.as_str()).unwrap_or("");
            explain(db, tool_database(args, database), sql).await
        }
        // **Boxed**, so each adds a pointer to this future rather than its
        // own state: the serve loop holds `call_tool`'s future on the stack,
        // and inlining these overflowed a debug test thread's 2 MiB in
        // `a_call_is_gated_by_its_own_level_not_the_others` (`sample_rows`
        // above is boxed for the same reason).
        McpTool::ListDatabases => Box::pin(list_databases(db, hidden, database)).await,
        McpTool::ListSchema => list_schema(db, arg("database"), database, hidden, reads_data).await,
        McpTool::SearchObjects => match arg("query").map(str::trim).filter(|q| !q.is_empty()) {
            Some(query) => search_objects(db, query, arg("database"), database, hidden).await,
            None => ("search_objects needs a `query`.".to_string(), true),
        },
        McpTool::ProposeTableChange => propose_change(db, database, arg("database"), args).await,
        // Two databases on this one connection; a `source_connection` naming
        // another is resolved in `serve_on`, which sees every connection.
        McpTool::CompareSchemas => {
            Box::pin(compare_schemas((endpoint, ""), (endpoint, ""), args)).await
        }
        McpTool::ProposeSql => {
            propose_sql(arg("sql").unwrap_or(""), propose_policy(endpoint, args))
        }
        // Statements and locks are row data and answer with their count.
        McpTool::TopStatements => {
            return Box::pin(top_statements(db, tool_database(args, database), args)).await;
        }
        McpTool::CurrentLocks => return Box::pin(current_locks(db)).await,
        McpTool::IndexHealth => Box::pin(index_health(db, tool_database(args, database))).await,
        McpTool::TableSizes => Box::pin(table_sizes(db, tool_database(args, database), args)).await,
    };
    answered.into()
}

/// How many rows a performance tool returns when the call doesn't say.
const PERF_ROWS: usize = 20;

/// The most `table_sizes` returns of each kind.
const MAX_SIZE_ROWS: usize = 100;

/// A performance tool's `limit`: absent is [`PERF_ROWS`], anything else must
/// be a whole number in `1..=max`. Refused rather than clamped, so a model
/// that asked for 10,000 learns the ceiling instead of silently getting less.
fn perf_limit(args: &Value, max: usize) -> Result<usize, String> {
    match args.get("limit") {
        None | Some(Value::Null) => Ok(PERF_ROWS.min(max)),
        Some(v) => match v.as_u64() {
            Some(n) if n >= 1 && n as usize <= max => Ok(n as usize),
            _ => Err(format!("`limit` must be a whole number from 1 to {max}.")),
        },
    }
}

/// Where `top_statements` reads from, for its description — naming the
/// source a model can tell the user to turn on.
fn statement_source(dialect: SqlDialect) -> &'static str {
    match dialect {
        SqlDialect::Postgres => "the pg_stat_statements extension",
        SqlDialect::MySql => "performance_schema's statement digests",
        SqlDialect::MsSql => "the database's Query Store",
        SqlDialect::Sqlite => "nowhere: SQLite keeps no statement statistics",
    }
}

/// `top_statements`: [`Db::fetch_top_statements`], as a table, or what is
/// missing. An absent source is an **answer**, not an error — the model's next
/// move is to tell the user, not to retry.
async fn top_statements(db: &Db, database: Option<&str>, args: &Value) -> Answer {
    use schemaic_core::workload::{MAX_STATEMENT_LIMIT, StatementOrder, StatementReport};
    let order = match args.get("order").and_then(Value::as_str) {
        None => StatementOrder::Total,
        Some(s) => match StatementOrder::parse(s) {
            Some(o) => o,
            None => {
                return ("`order` must be total, mean or calls.".to_string(), true).into();
            }
        },
    };
    let limit = match perf_limit(args, MAX_STATEMENT_LIMIT) {
        Ok(n) => n,
        Err(e) => return (e, true).into(),
    };
    // Bounded twice: `fetch_top_statements` carries `WORKLOAD_TIMEOUT`, and
    // the server's own deadline stands over it as over every read here. It
    // takes no token, so the deadline abandons rather than cancels.
    let Some(read) =
        with_deadline_abandoning(db.fetch_top_statements(database, order, limit)).await
    else {
        return (
            format!(
                "Reading the statement statistics timed out ({}s).",
                QUERY_TIMEOUT.as_secs()
            ),
            true,
        )
            .into();
    };
    match read {
        Ok(StatementReport::Ready { statements, since }) => {
            if statements.is_empty() {
                return (
                    "The statistics source is on but has recorded no statements yet.".to_string(),
                    false,
                )
                    .into();
            }
            let n = statements.len();
            let mut out = format_table_cells(
                &crate::perf::statements_result(&statements),
                STATEMENT_CHARS,
            );
            if let Some(since) = since {
                out.push_str(&format!("\nCounted since {since}."));
            }
            Answer::with_rows(out, n)
        }
        Ok(StatementReport::Unavailable(u)) => (crate::perf::unavailable_text(&u), false).into(),
        Err(e) => (format!("Error reading statement statistics: {e}"), true).into(),
    }
}

/// `current_locks`: [`Db::fetch_locks`], bounded there by `PING_TIMEOUT`.
async fn current_locks(db: &Db) -> Answer {
    if !schemaic_core::activity::supports_activity(dialect_of(db)) {
        return (
            "This engine has no server sessions, so there are no locks to show.".to_string(),
            false,
        )
            .into();
    }
    let Some(read) = with_deadline_abandoning(db.fetch_locks()).await else {
        return ("Reading the locks timed out.".to_string(), true).into();
    };
    match read {
        Ok(list) if list.locks.is_empty() => {
            let said = if list.contended_only {
                "No session is waiting on a lock. (This server lists contended locks only.)"
            } else {
                "No locks are held or waited for."
            };
            (said.to_string(), false).into()
        }
        Ok(list) => {
            let n = list.locks.len();
            let mut out = format_table(&crate::perf::locks_result(&list));
            if list.truncated {
                out.push_str("\n(The list was capped; more locks exist.)");
            }
            if list.contended_only {
                out.push_str("\n(This server lists contended locks only.)");
            }
            Answer::with_rows(out, n)
        }
        Err(e) => (format!("Error reading locks: {e}"), true).into(),
    }
}

/// The database a per-database performance tool reads, or the refusal that
/// says one is needed.
fn perf_database(database: Option<&str>) -> Result<&str, (String, bool)> {
    database.ok_or_else(|| {
        (
            "No database given and the connection has no default — pass a `database`.".to_string(),
            true,
        )
    })
}

/// The table statistics `index_health` and `table_sizes` sweep. The read
/// takes no token on any engine, so the deadline abandons rather than
/// cancels it (`with_deadline_abandoning`'s reason).
async fn perf_stats(
    db: &Db,
    database: &str,
) -> Result<schemaic_core::stats::SchemaStats, (String, bool)> {
    match with_deadline_abandoning(db.fetch_table_stats(database)).await {
        Some(Ok(s)) => Ok(s),
        Some(Err(e)) => Err((
            format!("Error reading statistics for {database}: {e}"),
            true,
        )),
        None => Err((
            format!(
                "Reading the statistics for {database} timed out ({}s).",
                QUERY_TIMEOUT.as_secs()
            ),
            true,
        )),
    }
}

/// `index_health`: the unused, duplicate and redundant indexes of one
/// database, then — where the engine records them — the missing ones, each
/// with the DDL `ddl::diff` would emit for it. Nothing is run.
async fn index_health(db: &Db, database: Option<&str>) -> (String, bool) {
    use schemaic_core::workload as wl;
    let database = match perf_database(database) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let token = CancellationToken::new();
    let schema = match with_deadline(db.fetch_schema(database, token.clone()), token).await {
        Some(Ok(s)) => s,
        Some(Err(e)) => return (format!("Error reading schema for {database}: {e}"), true),
        None => {
            return (
                format!("Reading the schema for {database} timed out and was cancelled."),
                true,
            );
        }
    };
    let stats = match perf_stats(db, database).await {
        Ok(s) => s,
        Err(e) => return e,
    };
    let findings = wl::index_health(&schema.tables, &stats);
    let mut out = if findings.is_empty() {
        format!("No duplicate, redundant or unused indexes found in {database}.")
    } else {
        format_table(&crate::perf::index_health_result(&findings))
    };
    out.push_str("\n\n");
    out.push_str(crate::perf::usage_note(&stats));
    let dialect = dialect_of(db);
    if wl::supports_missing_index_hints(dialect) {
        out.push_str("\n\nMissing indexes the optimiser recorded since the last restart:\n");
        let read =
            with_deadline_abandoning(db.fetch_missing_indexes(Some(database), PERF_ROWS)).await;
        match read.unwrap_or_else(|| {
            Err(schemaic_db::DbError::Query(
                "the read timed out".to_string(),
            ))
        }) {
            Ok(missing) if missing.is_empty() => out.push_str("(none recorded)"),
            Ok(missing) => {
                let s = crate::perf::suggestions(&schema, dialect, missing);
                out.push_str(&format_table(&crate::perf::missing_result(&s)));
                out.push_str("\n\nThe DDL for each, to propose — never to run:\n");
                out.push_str(&crate::perf::suggestion_ddl_text(&s));
                out.push_str(
                    "\nEach is a suggestion from one optimiser's estimates: check it against the \
                     existing indexes above before proposing it.",
                );
            }
            Err(e) => out.push_str(&format!("(unavailable: {e})")),
        }
    }
    (out, false)
}

/// `table_sizes`: the largest tables and indexes of one database.
async fn table_sizes(db: &Db, database: Option<&str>, args: &Value) -> (String, bool) {
    let database = match perf_database(database) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let n = match perf_limit(args, MAX_SIZE_ROWS) {
        Ok(n) => n,
        Err(e) => return (e, true),
    };
    let stats = match perf_stats(db, database).await {
        Ok(s) => s,
        Err(e) => return e,
    };
    let (tables, indexes) = schemaic_core::workload::largest(&stats, n);
    if tables.is_empty() {
        return (format!("{database} reported no table sizes."), false);
    }
    let mut out = format!("Largest tables in {database}:\n");
    out.push_str(&format_table(&crate::perf::table_sizes_result(&tables)));
    if indexes.is_empty() {
        out.push_str(
            if schemaic_core::workload::index_sizes_per_table_only(&stats) {
                "\n\nThis engine reports index sizes per table, not per index."
            } else {
                "\n\nThis database has no indexes to size."
            },
        );
    } else {
        out.push_str("\n\nLargest indexes:\n");
        out.push_str(&format_table(&crate::perf::index_sizes_result(&indexes)));
    }
    (out, false)
}

/// The guard policy `propose_sql` judges a statement under: the endpoint's
/// read-only mark, its dialect, and whether the call has a database at all —
/// its own, or the endpoint's.
fn propose_policy(endpoint: &Endpoint, args: &Value) -> schemaic_core::sql::GuardPolicy {
    schemaic_core::sql::GuardPolicy {
        read_only: endpoint.read_only,
        // The panel's Run asks again under the user's own setting; this
        // answers what the guard says of the statement itself.
        confirm_writes: false,
        dialect: dialect_of(&endpoint.db),
        no_database: tool_database(args, endpoint.database.as_deref()).is_none(),
    }
}

/// `compare_schemas`' answer: the script that makes one database's structure
/// match another's, and what is said about it — `schemaic compare` as a tool,
/// through the same `cli::compare::report` over the same `core::compare`.
///
/// **Nothing runs.** Two catalogues are read, each under the server's
/// deadline, and the script comes back as text: the model may show it, and
/// the user runs it — `schemaic exec --script`, or the app — if they choose.
///
/// `target` is where the script would run and `source` what it should come to
/// match: the same endpoint for two databases on one connection, two served
/// connections otherwise ([`serve_on`] resolves the second). Both sides must
/// be the same engine, as the CLI's are.
async fn compare_schemas(
    target: (&Endpoint, &str),
    source: (&Endpoint, &str),
    args: &Value,
) -> (String, bool) {
    let ((target, target_name), (source, source_name)) = (target, source);
    let two = !std::ptr::eq(target, source);
    // **Each side resolved as `schemaic compare` resolves it**: the engine's
    // implied database first — SQLite's `main`, which `fetch_schema` reads
    // whatever it is called — then the argument, then the default. Taken raw,
    // `source_database: "staging"` on SQLite read `main` twice and answered
    // "main already matches staging" for a database that does not exist.
    fn side<'a>(e: &'a Endpoint, named: Option<&'a str>) -> Option<&'a str> {
        e.db.implied_database().or(named).or(e.database.as_deref())
    }
    let source_db = side(
        source,
        args.get("source_database")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty()),
    );
    let target_db = side(
        target,
        args.get("database")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty()),
    );
    let (Some(target_db), Some(source_db)) = (target_db, source_db) else {
        return (
            "compare_schemas needs a database on each side: name `database` (the one the \
             script would change) and `source_database` (the one to match)."
                .to_string(),
            true,
        );
    };
    if !two && target_db == source_db {
        return (
            format!(
                "Both sides are {target_db}: name another `source_database`, or a \
                 `source_connection`."
            ),
            true,
        );
    }
    let dialect = dialect_of(&target.db);
    if let Err(e) = schemaic_core::compare::comparable(dialect, dialect_of(&source.db)) {
        return (e, true);
    }
    let tables: Vec<String> = args
        .get("tables")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    // `schemaic compare`'s deadline, not a statement's: two whole catalogues
    // are read, which on a large schema takes longer than thirty seconds,
    // and the model has no argument to raise it.
    let (lt, rt) = (CancellationToken::new(), CancellationToken::new());
    let (left, right) = tokio::join!(
        with_deadline_for(
            COMPARE_TIMEOUT,
            target.db.fetch_schema(target_db, lt.clone()),
            lt
        ),
        with_deadline_for(
            COMPARE_TIMEOUT,
            source.db.fetch_schema(source_db, rt.clone()),
            rt
        )
    );
    let read = |got, db: &str| match got {
        Some(Ok(s)) => Ok(s),
        Some(Err(e)) => Err(format!("Error reading the schema of {db}: {e}")),
        None => Err(format!(
            "Reading the schema of {db} timed out ({}s) and was cancelled.",
            COMPARE_TIMEOUT.as_secs()
        )),
    };
    let (left, right) = match (read(left, target_db), read(right, source_db)) {
        (Ok(l), Ok(r)) => (l, r),
        (Err(e), _) | (_, Err(e)) => return (e, true),
    };
    let cmp = schemaic_core::compare::SchemaComparison::of(&left, &right, dialect);
    let label = |name: &str, db: &str| {
        if two {
            format!("{db} on {name}")
        } else {
            db.to_string()
        }
    };
    match crate::compare::report(
        &cmp,
        &tables,
        &label(target_name, target_db),
        &label(source_name, source_db),
    ) {
        Err(e) => (e, true),
        Ok(report) => {
            let mut out = String::from("Nothing has run.\n");
            for note in &report.notes {
                out.push_str(note);
                out.push('\n');
            }
            if let Some(script) = report.script {
                out.push_str("\nSQL:\n");
                out.push_str(&script);
            }
            (out, false)
        }
    }
}

/// `propose_sql`'s answer: what the write guard (`sql::run_verdict`) says of
/// `sql`, worded for the model — and the instruction to hand it over in a
/// fenced `sql` block, which the panel gives **Insert** and **Run**.
///
/// **Advice, not a gate.** Nothing runs here, and the statement the user
/// runs is judged again by the guard on the run action — so a `Confirm` is
/// passed on as "Schemaic will ask first", never as permission, and the model
/// is never told the guard has been satisfied. A `Block` is the one answer
/// worth refusing on: proposing a write on a read-only connection asks the
/// user for something the product will not do.
fn propose_sql(sql: &str, policy: schemaic_core::sql::GuardPolicy) -> (String, bool) {
    use schemaic_core::sql::RunVerdict;
    let Some(stmt) = crate::query::normalize_stmt(sql) else {
        return ("propose_sql needs the `sql` to propose.".to_string(), true);
    };
    let stmts = schemaic_core::sql::executable_statements(stmt, policy.dialect);
    let hand_over = "Put it in a ```sql block in your reply: the user can Insert it into a \
                     query tab or Run it, and Schemaic's guard checks it again then.";
    match schemaic_core::sql::run_verdict(&stmts, policy) {
        RunVerdict::Block(why) => (
            format!(
                "Refused: {} Don't propose it; tell the user what you would change and why.",
                why.message()
            ),
            true,
        ),
        RunVerdict::Confirm(why) => (
            format!(
                "Valid. Nothing has run. Schemaic will ask the user before running it: {} \
                 Say so when you propose it. {hand_over}",
                why.message()
            ),
            false,
        ),
        RunVerdict::Allow => (format!("Valid. Nothing has run. {hand_over}"), false),
    }
}

/// The connection's SQL dialect — shared by the read-only gate, the DDL
/// skeleton, and the sample-row query builder so all three agree on quoting.
///
/// It delegates to [`schemaic_db::Engine::dialect`], whose `match` is
/// exhaustive, and that is the point. This read
/// `if engine == Postgres { Postgres } else { MySql }` — which compiled cleanly
/// when SQLite arrived and quietly sorted it onto the MySQL side, so the
/// read-only gate lexed AI-issued SQL by MySQL's backslash and `#` rules (SQLite
/// has neither) and `describe_table`'s sample query quoted against MySQL's
/// reserved words instead of SQLite's, leaving a table named `virtual` or
/// `indexed` unquoted.
fn dialect_of(db: &Db) -> SqlDialect {
    db.engine().dialect()
}

/// Statement timeout for AI-issued queries — a backstop against `SLEEP()` /
/// heavy scans holding the connection open. The CLI's default, read from where
/// it is defined rather than restated.
const QUERY_TIMEOUT: std::time::Duration = crate::query::DEFAULT_TIMEOUT;

/// Await a database read with [`QUERY_TIMEOUT`] over it, cancelling the token on
/// expiry so the statement is killed **server-side** rather than merely
/// abandoned.
///
/// **A property of the server, not of one tool.** The deadline lived inside
/// `run_query`, so three of the four database reads this server performs had
/// none: `describe_table`'s sample, `list_schema`'s `fetch_schema` and its
/// per-database table-list loop, and `propose_table_change`'s `fetch_schema`.
/// Each built a fresh `CancellationToken` that nothing ever cancelled and
/// awaited it bare.
///
/// The serve loop is strictly sequential — it awaits `call_tool` inline before
/// reading the next line — so one slow read wedges the **whole** server, not
/// just its own call. Measured against PostgreSQL 16.15 through the shipped
/// binary with a view over `pg_sleep(120)`: `describe_table` on it returned
/// nothing for over 100 seconds and the `ping` queued behind it was never
/// answered either, so the agent's tool call never returned and the turn hung
/// with Stop — which ends the whole session — as the only way out. MySQL was
/// *accidentally* bounded at ~15 s by the driver, which made it a silent
/// per-engine divergence in a refusal path as well.
///
/// `None` is the timeout, and the caller words it: a timed-out `run_query` is an
/// error the model must see, while a timed-out sample is a bonus the table's DDL
/// and keys do not depend on.
async fn with_deadline<F>(fut: F, token: CancellationToken) -> Option<F::Output>
where
    F: std::future::Future,
{
    with_deadline_for(QUERY_TIMEOUT, fut, token).await
}

/// How long `compare_schemas` gives each catalogue — `schemaic compare`'s own
/// default.
const COMPARE_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(crate::args::COMPARE_TIMEOUT_SECS);

/// [`with_deadline`] under a deadline of `timeout`.
async fn with_deadline_for<F>(
    timeout: std::time::Duration,
    fut: F,
    token: CancellationToken,
) -> Option<F::Output>
where
    F: std::future::Future,
{
    tokio::pin!(fut);
    tokio::select! {
        r = &mut fut => Some(r),
        _ = tokio::time::sleep(timeout) => {
            token.cancel();
            // Kept alive across the cancel so the driver's KILL branch runs —
            // abandoning the future leaves the statement running on the server.
            //
            // **Only correct while `token` reaches a driver.** Over a read that
            // takes no token this line is the bug: the cancel reaches nothing
            // and the await then blocks for the server's own time, so the
            // "deadline" returns after the query rather than after
            // `QUERY_TIMEOUT`. [`with_deadline_abandoning`] is that case, and it
            // is a separate function so the choice has to be made rather than
            // inherited.
            //
            // **Bounded all the same**, at the CLI's `UNWIND_GRACE`: a driver
            // still *connecting* has not reached its token, and a host that
            // drops packets held the command for the OS connect timeout. The
            // grace outlasts a started statement's own KILL, which is bounded
            // by `CANCEL_TIMEOUT`.
            let _ = tokio::time::timeout(crate::deadline::UNWIND_GRACE, fut).await;
            None
        }
    }
}

/// [`with_deadline`] for a read with **nothing to cancel**: the future is
/// dropped at the deadline rather than awaited.
///
/// `Db::fetch_table_list` takes no `CancellationToken` on any of the three
/// engines, so `with_deadline`'s `token.cancel()` reaches no driver and its
/// `fut.await` waits out the server. `list_schema`'s per-database loop was the
/// one caller, and the wedge it exists to prevent was therefore still open: a
/// catalogue read stuck behind an `ALTER TABLE`'s metadata lock held the
/// sequential serve loop for as long as the lock lasted, answering nothing —
/// not even `ping`.
///
/// Dropping the future does leave the statement running on the server until the
/// connection goes, which is the trade the call site already documented and
/// believed it was making. It is the smaller half: the wedge being closed is
/// *ours*. Giving the read a real token is a `schemaic-db` change and would let
/// it use [`with_deadline`] instead.
async fn with_deadline_abandoning<F>(fut: F) -> Option<F::Output>
where
    F: std::future::Future,
{
    tokio::time::timeout(QUERY_TIMEOUT, fut).await.ok()
}

/// **One headless read path, shared with the CLI.**
///
/// The normalisation, the read-only gate, the row cap and the cancel-and-wait
/// timeout all live in [`crate::query::read_only_query`] now. They were
/// duplicated here, which is two copies of a *guard* — the shape this codebase
/// has already been bitten by, and one only the front end that got the fix
/// would have kept.
///
/// **The wording stays here** rather than coming from `NoRows::message`, which
/// is the CLI's: that one points a refused write at `schemaic exec`, and the
/// assistant's route to a write is `propose_table_change` or `propose_sql`. Same refusal, right
/// advice for whoever is reading it.
async fn run_query(db: &Db, database: Option<&str>, sql: &str) -> Answer {
    match crate::query::read_only_query(db, database, sql, MCP_ROW_CAP, QUERY_TIMEOUT).await {
        Ok(rs) => Answer::with_rows(format_table(&rs), rs.row_count()),
        Err(e) => no_rows_reply(e).into(),
    }
}

/// The `explain` tool: [`crate::query::read_only_explain`]'s plan, as
/// `core::plan` writes it for a model — the server's own rows, then the
/// warnings its heuristics found. Refused in `run_query`'s words, since it is
/// `run_query`'s gate that refused it.
async fn explain(db: &Db, database: Option<&str>, sql: &str) -> (String, bool) {
    match crate::query::read_only_explain(db, database, sql, QUERY_TIMEOUT).await {
        Ok(plan) if plan.rows.is_empty() => ("(the server returned no plan)".to_string(), false),
        Ok(plan) => (format_plan(&plan), false),
        Err(e) => no_rows_reply(e),
    }
}

/// A plan as a tool result: [`format_table`]'s shape and caps, not
/// `QueryPlan::to_prompt_text`'s whole plan at full width.
///
/// **Held to the row cap and the cell width a query is**, because a tool
/// result rides along on every turn: a plan over a table partitioned by day
/// runs two rows a partition, up to the fetch's 10,000, which is hundreds of
/// KB a call. **And it says when it was cut** — by this cap, or by the fetch's
/// own (`QueryPlan::truncated`) — since a cut plan handed over as whole is a
/// wrong answer. The heuristics' warnings follow for the rows shown; those
/// about rows cut away are counted rather than listed.
fn format_plan(plan: &schemaic_core::plan::QueryPlan) -> String {
    let shown = plan.rows.len().min(MCP_ROW_CAP);
    let mut out =
        schemaic_core::prompt::pipe_table(&plan.columns, &plan.rows[..shown], MCP_CELL_CHARS);
    if plan.truncated || shown < plan.rows.len() {
        out.push_str(&format!("\n({shown}+ plan rows, capped)"));
    } else {
        let rows = if shown == 1 { "row" } else { "rows" };
        out.push_str(&format!("\n({shown} plan {rows})"));
    }
    let (listed, unlisted): (Vec<_>, Vec<_>) = plan.warnings.iter().partition(|w| w.row < shown);
    if !listed.is_empty() || !unlisted.is_empty() {
        out.push_str("\n\nPotential issues:");
        for w in &listed {
            out.push_str("\n- ");
            out.push_str(&w.message());
        }
        if !unlisted.is_empty() {
            out.push_str(&format!(
                "\n- …and {} more in the plan rows not shown.",
                unlisted.len()
            ));
        }
    }
    out
}

/// A headless read's refusal or failure, in the words a model is given.
fn no_rows_reply(e: crate::query::NoRows) -> (String, bool) {
    use crate::query::NoRows;
    match e {
        NoRows::Empty => ("Empty query.".to_string(), true),
        NoRows::NotARead(reason) | NoRows::NotPermitted(reason) => {
            (format!("Rejected: {reason}."), true)
        }
        // The refusal's own sentence says what would get past it — the user's
        // settings, or a different statement — so the reply only says the query
        // never ran.
        NoRows::DbRefused(reason) => (format!("Rejected before the query ran: {reason}"), true),
        NoRows::Failed(e) => (format!("Query error: {e}"), true),
        // A read never reports `Indeterminate` — that is a write's timeout —
        // but a read-only session makes the two the same event here anyway.
        NoRows::TimedOut(d) | NoRows::Indeterminate(d) => (
            format!("Query timed out ({}s) and was cancelled.", d.as_secs()),
            true,
        ),
    }
}

/// Render a result set as a pipe table (capped), for the assistant to read.
fn format_table(rs: &ResultSet) -> String {
    format_table_cells(rs, MCP_CELL_CHARS)
}

/// How wide `top_statements` lets a statement run. The text *is* that
/// tool's answer — a statement cut at [`MCP_CELL_CHARS`] is usually cut
/// before its `WHERE` — while the list is short ([`PERF_ROWS`] by default),
/// so the wider cell costs little.
const STATEMENT_CHARS: usize = 400;

/// [`format_table`] at a cell width of the caller's.
fn format_table_cells(rs: &ResultSet, cell_chars: usize) -> String {
    if rs.columns.is_empty() {
        return "(no columns)".to_string();
    }
    let header: Vec<String> = rs.columns.iter().map(|c| c.name.clone()).collect();
    let ncols = rs.col_count();
    let rows: Vec<Vec<String>> = (0..rs.row_count())
        .map(|r| {
            (0..ncols)
                .map(|c| {
                    rs.cell(r, c)
                        .map(|cell| cell.display().to_string())
                        .unwrap_or_default()
                })
                .collect()
        })
        .collect();
    // One table renderer for everything a model reads (`core::prompt`), so a
    // cell carrying `|`, a newline or a novel is handled the same here as in a
    // chat attachment. The width is this call site's: a tool result rides along
    // on every turn, so it stays narrow.
    let mut out = schemaic_core::prompt::pipe_table(&header, &rows, cell_chars);
    let more = if rs.truncated {
        format!("\n({}+ rows, capped)", rs.row_count())
    } else {
        format!("\n({} rows)", rs.row_count())
    };
    out.push_str(&more);
    out
}

/// Which databases the server **overview** reports.
///
/// The SCHEMA eye's rule, reaching the assistant's tools as well as its prompt:
/// a database the user has put away is not one the assistant should be
/// discovering and proposing work in. `schema::db_contributes` carries the same
/// exception it does everywhere else — the session's own default database is
/// listed even when hidden, since that is the database the user is *in* and its
/// queries run there regardless.
///
/// **Only the overview.** `list_schema {"database": "archive"}` still answers in
/// full, and so do `describe_table` and `run_query`, which is the same line the
/// app draws for itself: hiding governs what is *offered* — a listing is an
/// offer — and never what is *true*, which a named lookup asks for. A tool that
/// refused to describe a table the user had explicitly named would be lying
/// about the server rather than tidying a view, and `run_query` reaching any
/// database means a half-filtered fence would only be theatre.
fn listed_databases(
    names: Vec<String>,
    hidden: &HashSet<String>,
    default_db: Option<&str>,
) -> Vec<String> {
    names
        .into_iter()
        .filter(|n| schemaic_core::schema::db_contributes(hidden, n, default_db))
        .collect()
}

/// `list_schema` — the whole server as an overview (databases + table names), or
/// one database in full (columns, keys, foreign keys) when `database` is given.
/// Splitting it this way keeps a 50-database server from burning the model's
/// context on a listing it didn't ask for.
async fn list_schema(
    db: &Db,
    database: Option<&str>,
    default_db: Option<&str>,
    hidden: &HashSet<String>,
    samples: bool,
) -> (String, bool) {
    if let Some(name) = database {
        let token = CancellationToken::new();
        return match with_deadline(db.fetch_schema(name, token.clone()), token).await {
            Some(Ok(schema)) => (format_database_schema(name, &schema), false),
            Some(Err(e)) => (format!("Error reading schema for {name}: {e}"), true),
            None => (
                format!("Reading the schema for {name} timed out (30s) and was cancelled."),
                true,
            ),
        };
    }
    match overview(db, hidden, default_db).await {
        Ok(dbs) => (format_database_list(&dbs, samples), false),
        Err(e) => (e, true),
    }
}

/// Every listed database with its table **names** — what `list_schema`'s
/// overview prints and `search_objects` searches. The hidden set applies, as
/// it does to any listing (`listed_databases`); `Err` is the message when the
/// databases themselves could not be listed, and a database that fails on its
/// own carries its error in place.
async fn overview(
    db: &Db,
    hidden: &HashSet<String>,
    default_db: Option<&str>,
) -> Result<Vec<(String, Result<DbSchema, String>)>, String> {
    // **The first read of the overview form, and it had no deadline at all.**
    // This is the tool's advertised entry point — what a fresh turn asks first —
    // and `fetch_databases` is `SHOW DATABASES` on MySQL: behind an
    // `ALTER TABLE`'s metadata lock, or on a server that has stopped answering
    // without dropping the socket, it blocks for the server's own time. `serve`
    // is a single sequential loop with no outer timeout, so for the whole of
    // that the MCP server answers nothing — not `initialize`, not `tools/list`,
    // not `ping`. That is the wedge this file's deadlines exist to close, one
    // line above the read that was given one.
    //
    // `with_deadline_abandoning`, for the reason spelled out below: this read
    // takes no `CancellationToken` on any of the three engines, and
    // `with_deadline` over a tokenless read is a wait for the server rather
    // than a bound on ours.
    let names = match with_deadline_abandoning(db.fetch_databases()).await {
        Some(Ok(d)) => listed_databases(d, hidden, default_db),
        Some(Err(e)) => return Err(format!("Error listing databases: {e}")),
        None => return Err("Listing databases timed out (30s).".to_string()),
    };
    let mut dbs = Vec::with_capacity(names.len());
    for name in names {
        // The *list*, not the schema: this prints table names, and introspecting
        // every column of every table of every database to do that is the whole
        // server's catalogue for an answer that doesn't use it.
        //
        // **Per database, and the deadline is per database too.** A server with
        // fifty of them is the case this split exists for, and one of them being
        // slow must not hold the other forty-nine — nor the serve loop, which is
        // sequential and answers nothing while this runs.
        //
        // **Bounded in time, but not cancelled at the server**, and that is the
        // one of the two reads where those differ: `fetch_table_list` takes no
        // `CancellationToken` — it is a name listing, not the full
        // introspection — so the statement runs to completion on the server
        // after we stop waiting. The wedge this is about is *ours*, so bounding
        // the wait is the whole of the fix here; giving it a real token is a
        // `schemaic-db` change.
        //
        // Through `with_deadline_abandoning`, not `with_deadline`, and the
        // difference is the whole of it: the latter keeps the future alive
        // across the cancel so a driver's KILL branch runs, which over a read
        // that has no token means waiting out the server — so this read was not
        // bounded at all, and this comment was describing an intention.
        let schema = match with_deadline_abandoning(db.fetch_table_list(&name)).await {
            Some(r) => r.map_err(|e| e.to_string()),
            None => Err("timed out (30s)".to_string()),
        };
        dbs.push((name, schema));
    }
    Ok(dbs)
}

/// `search_objects` — table and view names containing `query`, across every
/// listed database, or in `database` alone with its column names as well.
///
/// One database is read in full ([`Db::fetch_schema`]) because that is where
/// the column names are; the whole server only as names, for the reason the
/// overview gives — introspecting every column of every database for a name
/// search is the whole catalogue for an answer that mostly doesn't use it.
async fn search_objects(
    db: &Db,
    query: &str,
    database: Option<&str>,
    default_db: Option<&str>,
    hidden: &HashSet<String>,
) -> (String, bool) {
    let (dbs, columns) = match database {
        Some(name) => {
            let token = CancellationToken::new();
            let schema = match with_deadline(db.fetch_schema(name, token.clone()), token).await {
                Some(r) => r.map_err(|e| e.to_string()),
                None => Err("timed out (30s) and was cancelled".to_string()),
            };
            (vec![(name.to_string(), schema)], true)
        }
        None => match overview(db, hidden, default_db).await {
            Ok(dbs) => (dbs, false),
            Err(e) => return (e, true),
        },
    };
    (format_search_hits(query, &dbs, columns), false)
}

/// The hits of a name search over `dbs`: per database, the tables and views
/// whose name contains `query` — matched by `schema::object_name_matches`, the
/// predicate every schema-search surface in the app uses — then, with
/// `columns`, the matching columns as `table.column`. At most
/// [`MCP_ROW_CAP`] hits, and says when it stopped. Pure.
fn format_search_hits(
    query: &str,
    dbs: &[(String, Result<DbSchema, String>)],
    columns: bool,
) -> String {
    let needle = query.to_lowercase();
    let hit = |name: &str| schemaic_core::schema::object_name_matches(name, &needle);
    let mut out = String::new();
    let mut n = 0usize;
    let mut capped = false;
    'dbs: for (name, schema) in dbs {
        let s = match schema {
            Ok(s) => s,
            Err(e) => {
                out.push_str(&format!("## {name} (error: {e})\n"));
                continue;
            }
        };
        let qualify = needs_qualifier(s);
        let objects = s.tables.iter().filter(|t| hit(&t.name)).map(|t| {
            let view = if t.is_view { " (view)" } else { "" };
            format!("{}{view}", table_label(t, qualify))
        });
        let cols = s.tables.iter().filter(|_| columns).flat_map(|t| {
            t.columns
                .iter()
                .filter(|c| hit(&c.name))
                .map(move |c| format!("{}.{}", table_label(t, qualify), c.name))
        });
        let mut lines = objects.chain(cols).peekable();
        if lines.peek().is_none() {
            continue;
        }
        out.push_str(&format!("## {name}\n"));
        for line in lines {
            if n == MCP_ROW_CAP {
                capped = true;
                break 'dbs;
            }
            out.push_str(&format!("- {line}\n"));
            n += 1;
        }
    }
    if n == 0 {
        let what = if columns {
            "table, view or column"
        } else {
            "table or view"
        };
        let k = dbs.len();
        let plural = if k == 1 { "database" } else { "databases" };
        return format!("No {what} name contains \"{query}\" (searched {k} {plural}).\n{out}");
    }
    let plural = if n == 1 { "match" } else { "matches" };
    if capped {
        out.push_str(&format!("({n}+ {plural}, capped)\n"));
    } else {
        out.push_str(&format!("({n} {plural})\n"));
    }
    out
}

/// The server overview: one heading per database, its tables listed by name.
/// Columns deliberately stay out — the trailing hint points at the two calls
/// that carry them. Pure so the shape is unit-tested.
///
/// `samples` is the endpoint's: the pointer at `describe_table` promises sample
/// rows only where that tool sends them, as its own description does.
fn format_database_list(dbs: &[(String, Result<DbSchema, String>)], samples: bool) -> String {
    let mut out = String::new();
    for (name, schema) in dbs {
        match schema {
            Ok(s) => {
                let n = s.tables.len();
                let plural = if n == 1 { "table" } else { "tables" };
                let qualify = needs_qualifier(s);
                out.push_str(&format!("## {name} ({n} {plural})\n"));
                for t in &s.tables {
                    let view = if t.is_view { " (view)" } else { "" };
                    // Namespace-qualified so the assistant writes a name that
                    // actually resolves.
                    out.push_str(&format!("- {}{view}\n", table_label(t, qualify)));
                }
            }
            Err(e) => out.push_str(&format!("## {name} (error: {e})\n")),
        }
    }
    out.push_str(&format!(
        "\nCall list_schema with {{\"database\": \"<name>\"}} for that database's columns and \
         keys, or describe_table for one table's DDL{}.\n",
        if samples {
            ", foreign keys, and sample rows"
        } else {
            " and foreign keys"
        },
    ));
    out
}

/// One database in full: every table with its columns (marked `PK` / `NOT NULL`)
/// and its foreign-key edges. The FK lines are the point — without them the
/// assistant invents join conditions we already introspected. Pure.
fn format_database_schema(database: &str, schema: &DbSchema) -> String {
    let mut out = format!("## {database}\n");
    if schema.tables.is_empty() {
        out.push_str("(no tables)\n");
        return out;
    }
    let qualify = needs_qualifier(schema);
    // A partition is named under its parent, whose columns and keys it has.
    for t in schema
        .tables
        .iter()
        .filter(|t| !schema.lists_under_parent(t))
    {
        let view = if t.is_view { " (view)" } else { "" };
        out.push_str(&format!(
            "### {}{view}{}\n",
            table_label(t, qualify),
            partitioned_by(t)
        ));
        partition_lines(schema, t, qualify, 1, &mut out);
        for c in &t.columns {
            let pk = if c.primary_key { " PK" } else { "" };
            // Nullable is the default; only the constraint is worth the tokens.
            let not_null = if c.nullable { "" } else { " NOT NULL" };
            out.push_str(&format!("  {} {}{pk}{not_null}\n", c.name, c.type_name));
        }
        for line in fk_lines(t, qualify) {
            out.push_str(&format!("  {line}\n"));
        }
    }
    out
}

/// ` (partitioned by …)` for a partitioned table, with MySQL's
/// `SUBPARTITION BY` clause beside the key — it keeps no subpartition names,
/// so the clause is all there is to say the second level exists.
fn partitioned_by(t: &TableInfo) -> String {
    let Some(p) = &t.partitioning else {
        return String::new();
    };
    match &p.subpartition {
        Some(sub) => format!(" (partitioned by {}, subpartitioned by {sub})", p.key),
        None => format!(" (partitioned by {})", p.key),
    }
}

/// `t`'s partitions, `depth` levels in: tables of their own (PostgreSQL), each
/// followed by its own partitions where it is partitioned in turn, or else
/// storage the table itself declares (MySQL, SQL Server). Only the top level
/// was listed, so a sub-partition was nowhere in the assistant's view.
fn partition_lines(
    schema: &DbSchema,
    t: &TableInfo,
    qualify: bool,
    depth: usize,
    out: &mut String,
) {
    let pad = "  ".repeat(depth);
    let tables = schema.partitions_of(t);
    if tables.is_empty() {
        for p in t.partitioning.iter().flat_map(|p| &p.partitions) {
            // A hash or key partition has no bound to name.
            match p.bound.as_str() {
                "" => out.push_str(&format!("{pad}partition {}\n", p.name)),
                bound => out.push_str(&format!("{pad}partition {}: {bound}\n", p.name)),
            }
        }
        return;
    }
    // Bounded like `DbSchema::listed_as`: a catalogue cannot hold a cycle,
    // but a model built by hand could.
    if depth > schema.tables.len() {
        return;
    }
    for p in tables {
        let bound = p.partition_of.as_ref().map_or("", |o| o.bound.as_str());
        out.push_str(&format!(
            "{pad}partition {}: {bound}{}\n",
            table_label(p, qualify),
            partitioned_by(p)
        ));
        partition_lines(schema, p, qualify, depth + 1, out);
    }
}

/// One table in depth: the `CREATE TABLE` skeleton (columns, PK, indexes) plus a
/// foreign-key section — `create_ddl` documents that it emits no FK references,
/// so they'd otherwise be missing entirely. Sample rows are appended by the
/// caller, which needs the DB. Pure.
fn format_table_detail(
    t: &TableInfo,
    dialect: schemaic_core::intel::SqlDialect,
    qualify: bool,
) -> String {
    let mut out = format!("{}\n", t.create_ddl(dialect));
    let fks = fk_lines(t, qualify);
    if !fks.is_empty() {
        out.push_str("\nForeign keys:\n");
        for line in fks {
            out.push_str(&format!("- {}\n", line.trim_start_matches("FK: ")));
        }
    }
    out
}

/// The heading above a table's detail: a name the connection's dialect can
/// actually resolve.
///
/// On MySQL the database *is* the namespace, so `database.table` is valid SQL.
/// On PostgreSQL it isn't — `warehouse.sales.orders` names nothing — so the
/// heading carries the schema-qualified name and the database goes on its own
/// line. (An earlier `{database}.{table}` heading is exactly what led a model to
/// write `warehouse.orders` for a `public` table.)
fn format_table_heading(
    database: &str,
    t: &TableInfo,
    dialect: schemaic_core::intel::SqlDialect,
    qualify: bool,
) -> String {
    match dialect {
        schemaic_core::intel::SqlDialect::MySql => {
            format!("### {database}.{}\n", t.name)
        }
        _ => format!("### {}\nDatabase: {database}\n", table_label(t, qualify)),
    }
}

/// A table's FK edges as `FK: col -> other.col` (or, for a composite key,
/// `FK: (a, b) -> other(x, y)`). ASCII arrows — the result is read by a model,
/// not rendered in the UI.
fn fk_lines(t: &TableInfo, qualify: bool) -> Vec<String> {
    t.foreign_keys
        .iter()
        .map(|fk| {
            let target = namespaced(fk.ref_schema.as_deref(), &fk.ref_table, qualify);
            if fk.columns.len() == 1 && fk.ref_columns.len() == 1 {
                format!("FK: {} -> {target}.{}", fk.columns[0], fk.ref_columns[0])
            } else {
                format!(
                    "FK: ({}) -> {target}({})",
                    fk.columns.join(", "),
                    fk.ref_columns.join(", ")
                )
            }
        })
        .collect()
}

/// Resolve the name `describe_table` was given against the database's tables.
///
/// Every form the listings can print has to round-trip: `display_name`'s output
/// (`sales.orders`, or a bare name in `public`/MySQL), and — since a
/// multi-schema listing spells out `public.` — an explicitly qualified name that
/// `display_name` would never produce.
fn find_described<'a>(schema: &'a DbSchema, table: &str) -> Option<&'a TableInfo> {
    schema
        .find_by_display(table)
        .or_else(|| {
            let (ns, name) = table.split_once('.')?;
            schema.find_table(Some(ns), name)
        })
        .or_else(|| schema.find_table(None, table))
}

/// Does this database need its namespaces spelled out? True once more than one
/// is present — the same rule the schema tree uses to grow a namespace level.
/// `schemas()` is empty on MySQL, so those always render bare.
fn needs_qualifier(schema: &DbSchema) -> bool {
    schema.schemas().len() > 1
}

/// A table's name as the assistant should write it.
fn table_label(t: &TableInfo, qualify: bool) -> String {
    namespaced(t.schema.as_deref(), &t.name, qualify)
}

/// `schema.table` when the database has several namespaces, else
/// [`schemaic_core::schema::display_name`]'s rule (bare in `public`, qualified
/// elsewhere). Spelling out
/// `public.` matters only when something else could be meant.
fn namespaced(schema: Option<&str>, table: &str, qualify: bool) -> String {
    match schema {
        Some(s) if qualify => format!("{s}.{table}"),
        _ => schemaic_core::schema::display_name(schema, table),
    }
}

/// Number of sample rows `describe_table` returns — enough to show the shape of
/// the data (formats, typical magnitudes, whether a column is really populated)
/// without turning the tool result into a data dump.
const SAMPLE_ROWS: usize = 5;

/// `describe_table` — one table's DDL, foreign keys, and a few sample rows.
async fn describe_table(
    db: &Db,
    default_db: Option<&str>,
    database: Option<&str>,
    table: &str,
    samples: bool,
) -> Answer {
    let Some(database) = database.or(default_db) else {
        return (
            "No database given and the connection has no default — pass a `database`.".to_string(),
            true,
        )
            .into();
    };
    let token = CancellationToken::new();
    let schema = match with_deadline(db.fetch_schema(database, token.clone()), token).await {
        Some(Ok(s)) => s,
        Some(Err(e)) => return (format!("Error reading schema for {database}: {e}"), true).into(),
        None => {
            return (
                format!("Reading the schema for {database} timed out (30s) and was cancelled."),
                true,
            )
                .into();
        }
    };
    let Some(info) = find_described(&schema, table) else {
        return (
            format!("Table {table} not found in {database}. Call list_schema to see what exists."),
            true,
        )
            .into();
    };

    let dialect = dialect_of(db);
    let qualify = needs_qualifier(&schema);
    let mut out = format_table_heading(database, info, dialect, qualify);
    out.push_str(&format_table_detail(info, dialect, qualify));

    // Sample rows read real data, so they follow the panel's "run queries"
    // setting — with it off the assistant still gets the DDL and the keys.
    if !samples {
        return (out, false).into();
    }
    out.push_str(&format!("\nSample rows (up to {SAMPLE_ROWS}):\n"));
    // A sample is a bonus, so a failure or a timeout degrades to an
    // "(unavailable: …)" line: the table's DDL and keys are still returned.
    match sample(db, database, info, SAMPLE_ROWS).await {
        Ok(rs) => {
            out.push_str(&format_table(&rs));
            Answer::with_rows(out, rs.row_count())
        }
        Err(e) => {
            out.push_str(&format!("(unavailable: {e})"));
            (out, false).into()
        }
    }
}

/// The first `n` rows of `info`, unordered, every column — `describe_table`'s
/// sample and `sample_rows`' whole answer. `Err` is the reason, in words that
/// follow "unavailable:" or stand alone.
async fn sample(db: &Db, database: &str, info: &TableInfo, n: usize) -> Result<ResultSet, String> {
    // Reuse the same dialect-aware, properly quoted builder the grid's "open
    // table" uses, so a reserved-word or spaced name is safe. No key is passed
    // for either purpose: the sample is unordered on purpose, and an implicit row
    // key would add a column the assistant has no use for — nothing writes back
    // through this path.
    let sql = schemaic_core::filter::table_query(
        dialect_of(db),
        database,
        info.schema.as_deref(),
        &info.name,
        schemaic_core::filter::BrowseKey::None,
        schemaic_core::filter::Order::Asc,
        n,
    );
    let token = CancellationToken::new();
    // **The read this whole finding is about.** A view over an aggregate is the
    // ordinary expensive case and a steered model can pick one deliberately;
    // with no deadline it held the sequential serve loop open for as long as the
    // server took — measured over 100 s on PG 16.15, with the `ping` behind it
    // never answered.
    //
    // A read-only session, like `run_query`'s: the statement is ours, but the
    // object is not — a view can call a function that writes. `Generated`,
    // since the statement is ours: it names no function a shadow could stand
    // in for.
    match with_deadline(
        db.fetch_query_enforced(
            Some(database),
            &sql,
            n,
            token.clone(),
            schemaic_db::Enforce::Generated,
        ),
        token,
    )
    .await
    {
        Some(Ok(rs)) => Ok(rs),
        Some(Err(e)) => Err(e.to_string()),
        None => Err("timed out after 30s and was cancelled".to_string()),
    }
}

/// `sample_rows`' row count when the call names none — a screenful, more
/// than `describe_table`'s glimpse.
const SAMPLE_ROWS_DEFAULT: usize = 10;

/// The most `sample_rows` returns. A sample shows what values look like;
/// past this the model wants a query, and `run_query` is there.
const SAMPLE_ROWS_MAX: usize = 50;

/// `sample_rows`' `limit`: absent is [`SAMPLE_ROWS_DEFAULT`]; anything but a
/// whole number from 1 to [`SAMPLE_ROWS_MAX`] is refused in words the model can
/// correct from, rather than clamped into an answer it did not ask for. Pure.
fn sample_limit(args: &Value) -> Result<usize, String> {
    match args.get("limit") {
        None | Some(Value::Null) => Ok(SAMPLE_ROWS_DEFAULT),
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| (1..=SAMPLE_ROWS_MAX).contains(n))
            .ok_or_else(|| {
                format!("sample_rows' `limit` is a whole number from 1 to {SAMPLE_ROWS_MAX}.")
            }),
    }
}

/// `sample_rows` — the first `n` rows of one table, resolved as
/// `describe_table` resolves it, read as its sample is.
async fn sample_rows(
    db: &Db,
    default_db: Option<&str>,
    database: Option<&str>,
    table: &str,
    n: usize,
) -> Answer {
    let Some(database) = database.or(default_db) else {
        return (
            "No database given and the connection has no default — pass a `database`.".to_string(),
            true,
        )
            .into();
    };
    let token = CancellationToken::new();
    let schema = match with_deadline(db.fetch_schema(database, token.clone()), token).await {
        Some(Ok(s)) => s,
        Some(Err(e)) => return (format!("Error reading schema for {database}: {e}"), true).into(),
        None => {
            return (
                format!("Reading the schema for {database} timed out (30s) and was cancelled."),
                true,
            )
                .into();
        }
    };
    let Some(info) = find_described(&schema, table) else {
        return (
            format!("Table {table} not found in {database}. Call list_schema to see what exists."),
            true,
        )
            .into();
    };
    match sample(db, database, info, n).await {
        Ok(rs) => Answer::with_rows(format_table(&rs), rs.row_count()),
        Err(e) => (format!("Could not read {table}: {e}"), true).into(),
    }
}

/// `list_databases` — the overview's names without its tables: the hidden
/// set applied as every listing applies it ([`listed_databases`]), and the
/// endpoint's default marked, since that is where a call naming none runs.
async fn list_databases(
    db: &Db,
    hidden: &HashSet<String>,
    default_db: Option<&str>,
) -> (String, bool) {
    // Tokenless, so dropped at the deadline — `overview`'s reason.
    match with_deadline_abandoning(db.fetch_databases()).await {
        Some(Ok(names)) => (
            format_database_names(&listed_databases(names, hidden, default_db), default_db),
            false,
        ),
        Some(Err(e)) => (format!("Error listing databases: {e}"), true),
        None => ("Listing databases timed out (30s).".to_string(), true),
    }
}

/// `list_databases`' answer: a line per database, the default marked, a
/// count, and the call that shows one in full. Pure.
fn format_database_names(names: &[String], default_db: Option<&str>) -> String {
    let mut out = String::new();
    for n in names {
        let mark = if Some(n.as_str()) == default_db {
            " (default)"
        } else {
            ""
        };
        out.push_str(&format!("- {n}{mark}\n"));
    }
    let k = names.len();
    out.push_str(&format!(
        "({k} {})\n\nCall list_schema with {{\"database\": \"<name>\"}} for one database's tables, \
         columns and keys.\n",
        if k == 1 { "database" } else { "databases" }
    ));
    out
}

/// The proposal inside a `propose_table_change` call's arguments.
///
/// `database` is the *tool's* argument, not the proposal's, and
/// [`Proposal`] refuses unknown keys on purpose — a model that invents a field
/// has to be told rather than have it quietly dropped — so it comes off here
/// instead of being tolerated there. Pure, so both halves of that are tested.
fn proposal_from_args(args: &Value) -> Result<Proposal, String> {
    let mut fields = args.clone();
    if let Some(obj) = fields.as_object_mut() {
        obj.remove("database");
    }
    serde_json::from_value(fields).map_err(|e| format!("That isn't a valid proposal: {e}"))
}

/// `propose_table_change` — check a proposed change and show the DDL it would
/// produce. **Reads only.** Nothing here executes anything.
///
/// The value is the loop it closes: the model finds out *here* that it misread a
/// column name or asked for something the designer's own validation refuses,
/// instead of the user being handed a preview that can't be applied. What comes
/// back is the same SQL the preview modal will show, so the model is looking at
/// what the user will look at.
///
/// It deliberately stops there. The result goes to the *model*, and this process
/// has no route into the app's overlays — the proposal reaches the user only
/// when the model echoes it into its reply, where the app picks it up. So a
/// model that skips this tool has lost a check, not a safety rail: the preview
/// and the Apply click are downstream of both paths.
async fn propose_change(
    db: &Db,
    default_db: Option<&str>,
    database: Option<&str>,
    args: &Value,
) -> (String, bool) {
    let Some(database) = database.or(default_db) else {
        return (
            "No database given and the connection has no default — pass a `database`.".to_string(),
            true,
        );
    };
    let proposal = match proposal_from_args(args) {
        Ok(p) => p,
        Err(e) => return (e, true),
    };

    let token = CancellationToken::new();
    let schema = match with_deadline(db.fetch_schema(database, token.clone()), token).await {
        Some(Ok(s)) => s,
        Some(Err(e)) => return (format!("Error reading schema for {database}: {e}"), true),
        None => {
            return (
                format!("Reading the schema for {database} timed out (30s) and was cancelled."),
                true,
            );
        }
    };
    let dialect = dialect_of(db);
    // **The same resolver the proposal card uses**, so the table this validates
    // is the table the user is later offered. The two used to disagree about
    // `proposal.schema` and about a qualified `sales.orders`, which is how a
    // change reported valid for `public.orders` reached a modal targeting
    // `sales.orders`.
    let info = match propose::resolve_target(&schema, &proposal) {
        Ok(t) => t,
        Err(e) => return (format!("{e} (in {database})"), true),
    };
    let draft = match propose::apply(info, &proposal, dialect) {
        Ok(d) => d,
        Err(e) => return (e.to_string(), true),
    };
    // The one differ, and the flavour the schema was read with — the MySQL
    // emitter's `ALTER TABLE` path reads it, so dropping it here would produce a
    // preview that differs from the designer's for the same change.
    let changes = schemaic_core::ddl::diff(info, &draft, Target::new(dialect, schema.flavour));
    if changes.is_empty() {
        return (
            format!(
                "Those ops leave {} exactly as it is — there is nothing to propose.",
                proposal.table
            ),
            true,
        );
    }

    // The change list the preview shows, in the preview's own words, so the
    // model and the user are reading the same account of the change.
    let mut out = String::from("Valid. Nothing has run.\n\nChanges:\n");
    for c in &changes.changes {
        out.push_str(&format!("- {}\n", c.summary()));
    }
    // **`editor_script`, not `emit().join`.** This is a fifth builder joining
    // emitted statements into one script, and the rule the other four keep is
    // that every such join goes through `ddl::client_script` — which terminates
    // each statement and wraps a compound MySQL body in `DELIMITER $$`. A
    // single-object `ChangeSet` is exactly where a trigger or routine body
    // lives, so a raw join hands the model a `CREATE TRIGGER … BEGIN …; …; END`
    // that the app's own splitter cuts at the internal semicolons. It also
    // carries `withheld_header`, so a change this engine refuses is disclosed
    // here instead of printed as if it were part of the plan.
    out.push_str(&format!("\nSQL:\n{}\n", changes.editor_script()));
    let risks = changes.destructive();
    if !risks.is_empty() {
        out.push_str("\nDestructive — the preview will say so too:\n");
        for r in &risks {
            out.push_str(&format!("- {r}\n"));
        }
    }
    out.push_str(&format!(
        "\nTo offer this to the user, put the same JSON in a ```{} fenced block in your reply. \
         They get a change preview and apply it themselves — don't hand them the SQL to run.",
        propose::FENCE_TAG,
    ));
    (out, false)
}

#[cfg(test)]
mod tests {
    use super::{
        HashSet, Host, MCP_ROW_CAP, McpTool, NO_DATA_ACCESS, NO_DATA_ACCESS_STANDALONE, NO_PANEL,
        SUPPORTED_PROTOCOLS, dialect_of, find_described, format_database_list,
        format_database_schema, format_search_hits, format_table, format_table_detail,
        format_table_heading, listed_databases, negotiate_protocol, proposal_from_args,
        refusal_for,
    };

    /// **A read that finishes in time comes back whole**, and the clock is not
    /// consulted for it — a deadline that answered `None` for a fast query would
    /// be worse than none at all.
    #[tokio::test(start_paused = true)]
    async fn a_read_that_beats_the_deadline_is_returned() {
        let token = tokio_util::sync::CancellationToken::new();
        let got = super::with_deadline(
            async {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                "rows"
            },
            token.clone(),
        )
        .await;
        assert_eq!(got, Some("rows"));
        assert!(!token.is_cancelled(), "a read in time must not be killed");
    }

    /// **And a slow one is cancelled, not merely abandoned.** The token is what
    /// makes the driver KILL the statement server-side; dropping the future
    /// instead leaves it running on the server for as long as it takes, which is
    /// the whole reason the deadline keeps the future alive across the cancel.
    ///
    /// The serve loop is sequential — it awaits `call_tool` inline before reading
    /// the next line — so this is the difference between one slow read and a
    /// wedged server. Measured on PG 16.15 through the shipped binary: a
    /// `describe_table` on a view over `pg_sleep(120)` returned nothing for over
    /// 100 seconds and the `ping` queued behind it was never answered either.
    #[tokio::test(start_paused = true)]
    async fn a_read_past_the_deadline_is_cancelled_server_side() {
        let token = tokio_util::sync::CancellationToken::new();
        let killed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let saw = killed.clone();
        let inner = token.clone();
        let got = super::with_deadline(
            async move {
                // What a driver does: run long, and on cancellation issue the
                // KILL before returning.
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(600)) => "rows",
                    _ = inner.cancelled() => {
                        saw.store(true, std::sync::atomic::Ordering::SeqCst);
                        "killed"
                    }
                }
            },
            token.clone(),
        )
        .await;
        assert_eq!(got, None, "the caller must see the timeout");
        assert!(token.is_cancelled(), "the token was never cancelled");
        assert!(
            killed.load(std::sync::atomic::Ordering::SeqCst),
            "the future was dropped rather than awaited across the cancel, so \
             the statement is still running on the server"
        );
    }

    /// **And the sixth read, whose token reaches nothing, is bounded by giving
    /// up on it.**
    ///
    /// Keeping the future alive across the cancel is right for the five reads
    /// whose token reaches a driver — that is what makes the KILL run. It is
    /// exactly wrong for `fetch_table_list`, which takes **no**
    /// `CancellationToken` on any of the three engines: `token.cancel()` reaches
    /// nothing, and `fut.await` then blocks for the server's own time. So the
    /// deadline returned after the query, not after 30 s, and the sequential
    /// serve loop answered nothing — not even `ping` — for the whole of it. The
    /// call site's own comment said the opposite: "bounding the wait is the
    /// whole of the fix here."
    ///
    /// A `MetadataLock` waiting on an `ALTER TABLE` is the ordinary shape of
    /// this, and the same commit measured the sibling at >100 s on PG 16.15.
    #[tokio::test(start_paused = true)]
    async fn a_deadline_over_an_uncancellable_read_gives_up_on_it() {
        let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let saw = finished.clone();
        let started = std::time::Instant::now();
        let got = super::with_deadline_abandoning(async move {
            // What `fetch_table_list` is: a read with nothing to cancel it.
            tokio::time::sleep(std::time::Duration::from_secs(600)).await;
            saw.store(true, std::sync::atomic::Ordering::SeqCst);
            "rows"
        })
        .await;
        assert_eq!(got, None, "the caller must see the timeout");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(60),
            "the deadline waited for the read instead of giving up on it: {:?}",
            started.elapsed()
        );
        assert!(
            !finished.load(std::sync::atomic::Ordering::SeqCst),
            "the read was awaited to completion"
        );
        // …and one that beats the deadline still comes back whole.
        assert_eq!(
            super::with_deadline_abandoning(async { "rows" }).await,
            Some("rows")
        );
    }

    // The two source gates over this file's deadlines — every token belongs to
    // a `with_deadline`, and every awaited `db.` read sits inside one — live in
    // `schemaic-ui`'s `source_gate::mcp_deadline_gate`, beside the brace-aware
    // walk they need. This crate is Floem-free and a copy of the walk is what
    // that module exists to prevent.

    /// Tool names the server advertises in the panel, in order.
    fn offered(engine: schemaic_db::Engine, reads_data: bool) -> Vec<String> {
        offered_with(engine, reads_data, true, Host::Panel)
    }

    fn offered_with(
        engine: schemaic_db::Engine,
        reads_data: bool,
        reads_schema: bool,
        host: Host,
    ) -> Vec<String> {
        super::tools_list(engine, reads_data, reads_schema, host)
            .as_array()
            .expect("a list")
            .iter()
            .map(|t| t["name"].as_str().expect("a name").to_string())
            .collect()
    }

    /// A connection that lets the assistant read rows is offered every tool.
    #[test]
    fn the_query_tool_is_offered_when_the_connection_sends_rows() {
        for engine in [
            schemaic_db::Engine::MySql,
            schemaic_db::Engine::Postgres,
            schemaic_db::Engine::Sqlite,
        ] {
            assert!(
                offered(engine, true).contains(&"run_query".to_string()),
                "{engine:?} withholds run_query from a connection that allows it"
            );
        }
    }

    /// The tool is *withheld*, not merely refused, when the connection sends no
    /// rows on the assistant's own initiative. A model that can see the tool
    /// plans around it — it offers to "run a query and analyse the results for
    /// you", the user says yes, and only then does the call come back denied.
    /// Nothing in a system prompt reliably outranks a tool the model can see, so
    /// the listing is where the setting has to bite.
    #[test]
    fn the_query_tool_is_withheld_when_the_connection_sends_no_rows() {
        for engine in [
            schemaic_db::Engine::MySql,
            schemaic_db::Engine::Postgres,
            schemaic_db::Engine::Sqlite,
        ] {
            let names = offered(engine, false);
            for gone in ["run_query", "explain", "sample_rows"] {
                assert!(
                    !names.contains(&gone.to_string()),
                    "{engine:?} still advertises {gone}: {names:?}"
                );
            }
            // The schema tools are untouched — the level withholds *data*, not
            // the ability to look at the shape of the database.
            for kept in [
                "list_databases",
                "list_schema",
                "describe_table",
                "search_objects",
                "propose_table_change",
            ] {
                assert!(
                    names.contains(&kept.to_string()),
                    "{engine:?} dropped {kept}: {names:?}"
                );
            }
        }
    }

    /// One predicate decides both halves, so a tool can never be listed by one
    /// rule and refused by another.
    #[test]
    fn only_queries_plans_samples_statements_and_locks_count_as_reading_rows() {
        assert!(McpTool::RunQuery.reads_row_data());
        assert!(McpTool::Explain.reads_row_data());
        // A sample is rows, whoever wrote the `SELECT`.
        assert!(McpTool::SampleRows.reads_row_data());
        // Query Store keeps statements with their literals, and a MySQL lock
        // names the locked record's key values.
        assert!(McpTool::TopStatements.reads_row_data());
        assert!(McpTool::CurrentLocks.reads_row_data());
        for schema_tool in [
            McpTool::ListDatabases,
            McpTool::ListSchema,
            McpTool::DescribeTable,
            McpTool::ProposeTableChange,
            McpTool::IndexHealth,
            McpTool::TableSizes,
        ] {
            assert!(!schema_tool.reads_row_data());
        }
    }

    /// Every performance tool names tables, so *Schema context: None*
    /// withholds them all, and all of them only read.
    #[test]
    fn the_performance_tools_read_the_catalogue_and_write_nothing() {
        use super::{NO_DATA_ACCESS_URL, NO_SCHEMA_ACCESS};
        for tool in [
            McpTool::TopStatements,
            McpTool::IndexHealth,
            McpTool::TableSizes,
            McpTool::CurrentLocks,
        ] {
            assert!(tool.reads_schema(), "{tool:?}");
            assert!(tool.read_only(), "{tool:?}");
            assert!(!tool.needs_panel(), "{tool:?}");
            assert_eq!(
                refusal_for(tool, true, false, Host::Standalone),
                Some(NO_SCHEMA_ACCESS)
            );
            assert!(NO_SCHEMA_ACCESS.contains(tool.name()), "{tool:?}");
        }
        for tool in [McpTool::TopStatements, McpTool::CurrentLocks] {
            assert!(NO_DATA_ACCESS.contains(tool.name()), "{tool:?}");
            assert!(NO_DATA_ACCESS_STANDALONE.contains(tool.name()), "{tool:?}");
            assert!(NO_DATA_ACCESS_URL.contains(tool.name()), "{tool:?}");
        }
    }

    #[test]
    fn a_performance_limit_defaults_and_refuses_what_it_cannot_honour() {
        use super::{PERF_ROWS, perf_limit};
        use serde_json::json;
        assert_eq!(perf_limit(&json!({}), 100), Ok(PERF_ROWS));
        assert_eq!(perf_limit(&json!({"limit": null}), 100), Ok(PERF_ROWS));
        assert_eq!(perf_limit(&json!({"limit": 5}), 100), Ok(5));
        assert_eq!(perf_limit(&json!({"limit": 100}), 100), Ok(100));
        assert!(perf_limit(&json!({"limit": 0}), 100).is_err());
        assert!(perf_limit(&json!({"limit": 101}), 100).is_err());
        assert!(perf_limit(&json!({"limit": "ten"}), 100).is_err());
        assert!(perf_limit(&json!({"limit": 2.5}), 100).is_err());
    }

    /// **The set is one set.** Every advertised name maps back to a variant and
    /// every variant is advertised — the property the five independent
    /// spellings could not have, and the one that made a new tool default to
    /// ungated at both gates.
    #[test]
    fn every_advertised_tool_is_a_known_one_and_every_known_one_is_advertised() {
        for engine in [
            schemaic_db::Engine::MySql,
            schemaic_db::Engine::Postgres,
            schemaic_db::Engine::Sqlite,
        ] {
            let names = offered_with(engine, true, true, Host::Panel);
            for name in &names {
                assert!(
                    McpTool::from_name(name).is_some(),
                    "{engine:?} advertises {name}, which is not an McpTool"
                );
            }
            for tool in McpTool::ALL {
                assert!(
                    names.iter().any(|n| n == tool.name()),
                    "{engine:?} does not advertise {}: {names:?}",
                    tool.name()
                );
                // And its harness spelling is the wire name, prefixed — the
                // relationship `ai_allowed_tools` depends on.
                assert_eq!(
                    tool.ai_name(),
                    format!("mcp__schemaic__{}", tool.name()),
                    "{} names itself two ways",
                    tool.name()
                );
            }
        }
    }

    /// A client that calls the withheld tool anyway — an older `claude` that
    /// cached the listing, or one that never asked — is refused by the server
    /// itself, and told which setting to point the user at.
    #[test]
    fn a_call_to_the_withheld_tool_is_refused_with_the_reason() {
        assert_eq!(
            refusal_for(McpTool::RunQuery, false, true, Host::Panel),
            Some(NO_DATA_ACCESS)
        );
        assert_eq!(
            refusal_for(McpTool::RunQuery, true, true, Host::Panel),
            None
        );
        assert_eq!(
            refusal_for(McpTool::SampleRows, false, true, Host::Panel),
            Some(NO_DATA_ACCESS)
        );
        assert!(NO_DATA_ACCESS.contains("sample_rows"));
        for schema_tool in [
            McpTool::ListDatabases,
            McpTool::ListSchema,
            McpTool::DescribeTable,
            McpTool::ProposeTableChange,
        ] {
            assert_eq!(refusal_for(schema_tool, false, true, Host::Panel), None);
        }
    }

    /// **"Schema context: None" has to reach the tools, or it means nothing.**
    ///
    /// With the setting at `None` the system prompt carries no databases and no
    /// tables — and the model still held `list_schema`, whose first call returns
    /// every database and every table name, and `list_schema {"database": …}`,
    /// which returns every column, key and foreign key. The setting the user
    /// just chose was defeated in one tool call, with only the tool's collapsed
    /// result line on screen. `c7bc9d7` closed exactly this on the inline path
    /// and left the tool path open.
    ///
    /// The same shape `reads_data` already has: withheld from the listing *and*
    /// refused on call, so a stale `tools/list` cannot reach it either.
    #[test]
    fn the_schema_tools_are_withheld_when_the_user_asked_for_no_schema() {
        for engine in [
            schemaic_db::Engine::MySql,
            schemaic_db::Engine::Postgres,
            schemaic_db::Engine::Sqlite,
        ] {
            let names = offered_with(engine, true, false, Host::Panel);
            // **`propose_table_change` is on this list, and it was not.** Its
            // exclusion was justified as "it carries the table it is about in
            // the call and reads nothing the model did not already have", and
            // both halves were false: `propose_change` calls
            // `db.fetch_schema(database, …)` — a full catalogue read of an
            // arbitrary database — and `resolve_target`'s hit/miss split is a
            // working existence oracle over every database and table name a
            // model cares to guess, at the one setting whose purpose is to
            // publish none of them. On SQLite it is not an oracle but a dump:
            // `AddCheck` is off the native allowlist, so `diff` collapses the
            // set into one `RebuildTable`, whose emitted SQL is the table's
            // complete `CREATE TABLE` plus an `INSERT … SELECT` naming every
            // column again.
            // `sample_rows` too: it resolves its table against the catalogue,
            // the existence oracle above, before it reads a row.
            for gone in [
                "list_databases",
                "list_schema",
                "describe_table",
                "search_objects",
                "sample_rows",
                "propose_table_change",
            ] {
                assert!(
                    !names.contains(&gone.to_string()),
                    "{engine:?} still advertises {gone}: {names:?}"
                );
            }
            // `run_query` is the connection's decision, not this one.
            assert!(
                names.contains(&"run_query".to_string()),
                "{engine:?} dropped run_query: {names:?}"
            );
        }
        // …and a call to one anyway is refused with the setting named.
        for gone in [
            McpTool::ListDatabases,
            McpTool::ListSchema,
            McpTool::DescribeTable,
            McpTool::SampleRows,
            McpTool::ProposeTableChange,
        ] {
            let refusal = refusal_for(gone, true, false, Host::Panel).expect("refused");
            assert!(refusal.contains("Schema context"), "{refusal}");
        }
        assert_eq!(
            refusal_for(McpTool::RunQuery, true, false, Host::Panel),
            None
        );
    }

    /// **Outside the panel nothing reads the reply, so the proposal tool is
    /// withheld** — listed, it tells the model the user has a change preview to
    /// review, when under `schemaic mcp` they have a fenced block of JSON.
    /// Every other tool is the connection's decision, not the host's.
    #[test]
    fn the_proposal_tool_is_withheld_outside_the_panel() {
        for engine in [
            schemaic_db::Engine::MySql,
            schemaic_db::Engine::Postgres,
            schemaic_db::Engine::Sqlite,
        ] {
            let names = offered_with(engine, true, true, Host::Standalone);
            assert_eq!(
                names,
                [
                    "run_query",
                    "explain",
                    "sample_rows",
                    "list_databases",
                    "list_schema",
                    "describe_table",
                    "search_objects",
                    "compare_schemas",
                    "top_statements",
                    "index_health",
                    "table_sizes",
                    "current_locks"
                ],
                "{engine:?}"
            );
            let names = offered_with(engine, true, true, Host::Panel);
            assert!(
                names.contains(&"propose_table_change".to_string()),
                "{engine:?} lost the proposal tool in the panel: {names:?}"
            );
        }
        // …and a stale listing that still names it is refused on call.
        assert_eq!(
            refusal_for(McpTool::ProposeTableChange, true, true, Host::Standalone),
            Some(NO_PANEL)
        );
        for kept in [
            McpTool::RunQuery,
            McpTool::SampleRows,
            McpTool::ListDatabases,
            McpTool::ListSchema,
            McpTool::DescribeTable,
        ] {
            assert_eq!(refusal_for(kept, true, true, Host::Standalone), None);
        }
        assert!(McpTool::ProposeTableChange.needs_panel());
    }

    /// **The connection's AI data access still decides rows standalone.** The
    /// host changes what a refusal *advises*, never whether it refuses: the
    /// panel's "attach rows from a result grid" names a gesture an outside
    /// client does not have.
    #[test]
    fn rows_are_withheld_standalone_with_advice_that_fits_the_client() {
        let names = offered_with(schemaic_db::Engine::Postgres, false, true, Host::Standalone);
        assert_eq!(
            names,
            [
                "list_databases",
                "list_schema",
                "describe_table",
                "search_objects",
                "compare_schemas",
                "index_health",
                "table_sizes"
            ]
        );
        // Other people's statements and locked key values are rows.
        for withheld in [McpTool::TopStatements, McpTool::CurrentLocks] {
            assert_eq!(
                refusal_for(withheld, false, true, Host::Standalone),
                Some(NO_DATA_ACCESS_STANDALONE)
            );
        }
        let refusal = refusal_for(McpTool::RunQuery, false, true, Host::Standalone);
        assert_eq!(refusal, Some(NO_DATA_ACCESS_STANDALONE));
        assert!(!NO_DATA_ACCESS_STANDALONE.contains("grid"));
        assert!(NO_DATA_ACCESS_STANDALONE.contains("Let it read data"));
        assert_eq!(
            schemaic_core::connection::AiData::Full.label(),
            "Let it read data",
            "the refusal names the level by its label"
        );
    }

    /// The panel's allow-list and the server's listing are one predicate — so a
    /// tool offered by one and missing from the other is impossible, at every
    /// combination of the three gates.
    #[test]
    fn the_listing_is_exactly_the_tools_offered() {
        for host in [Host::Panel, Host::Standalone] {
            for reads_data in [false, true] {
                for schema in [false, true] {
                    let listed = offered_with(schemaic_db::Engine::MySql, reads_data, schema, host);
                    let offered: Vec<&str> = McpTool::ALL
                        .into_iter()
                        .filter(|t| t.offered(reads_data, schema, host))
                        .map(McpTool::name)
                        .collect();
                    assert_eq!(
                        listed, offered,
                        "{host:?} rows={reads_data} schema={schema}"
                    );
                }
            }
        }
    }

    /// Drive the real serve loop over `input`, and parse every line it wrote.
    /// No request here reaches the database, so the handle is never dialled.
    async fn exchange(input: &[u8]) -> Vec<serde_json::Value> {
        exchange_on(&standalone(None), input).await
    }

    /// [`exchange`] against a given endpoint.
    async fn exchange_on(endpoint: &super::Endpoint, input: &[u8]) -> Vec<serde_json::Value> {
        exchange_many(
            &[super::Named {
                name: "only",
                endpoint,
            }],
            input,
        )
        .await
    }

    /// [`exchange`] against several named endpoints.
    async fn exchange_many(endpoints: &[super::Named<'_>], input: &[u8]) -> Vec<serde_json::Value> {
        let mut out = Vec::new();
        super::serve_on(endpoints, input, &mut out).await;
        String::from_utf8(out)
            .expect("the server writes UTF-8")
            .lines()
            .map(|l| serde_json::from_str(l).expect("every line is one JSON message"))
            .collect()
    }

    /// **A method this server does not implement is `-32601`, not `{}`.** An
    /// empty result reads as success: a client that asks `resources/list`
    /// gets an object with no `resources` in it and fails its own schema check,
    /// marking the whole server broken. Standalone, any client can connect.
    /// A notification (no id) still gets nothing at all.
    #[tokio::test]
    async fn an_unknown_method_is_method_not_found() {
        let got = exchange(
            b"{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"resources/list\"}\n\
              {\"jsonrpc\":\"2.0\",\"method\":\"notifications/whatever\"}\n\
              {\"jsonrpc\":\"2.0\",\"id\":8,\"method\":\"ping\"}\n",
        )
        .await;
        assert_eq!(got.len(), 2, "{got:?}");
        assert_eq!(got[0]["id"], 7);
        assert_eq!(got[0]["error"]["code"], -32601, "{}", got[0]);
        assert!(got[0].get("result").is_none(), "{}", got[0]);
        assert_eq!(got[1]["id"], 8);
        assert_eq!(got[1]["result"], serde_json::json!({}));
    }

    /// **A line that is not a request is answered, not swallowed.** A
    /// truncated line and a batch (neither served revision accepts one) got
    /// no reply at all, so the client waited on its id to its own timeout and
    /// then marked the server failed, with nothing on stderr to say why.
    /// JSON-RPC's answer is an error with a null id; the session goes on.
    #[tokio::test]
    async fn a_line_that_is_not_a_request_is_answered_with_an_error() {
        let got = exchange(
            b"{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/list\"\n\
              [{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}]\n\
              {\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"ping\"}\n",
        )
        .await;
        assert_eq!(got.len(), 3, "{got:?}");
        assert_eq!(got[0]["error"]["code"], -32700, "{}", got[0]);
        assert_eq!(got[0]["id"], serde_json::Value::Null);
        assert_eq!(got[1]["error"]["code"], -32600, "{}", got[1]);
        assert_eq!(got[1]["id"], serde_json::Value::Null);
        assert_eq!(got[2]["id"], 9);
        assert_eq!(got[2]["result"], serde_json::json!({}));
    }

    /// An endpoint on a SQLite file that does not exist: a call the gate
    /// wrongly lets through fails at once on the missing file, rather than
    /// dialling a server.
    fn unreachable(samples: bool, schema: bool, host: super::Host) -> super::Endpoint {
        let db = schemaic_db::Db::from_parts(
            schemaic_db::Engine::Sqlite,
            String::new(),
            0,
            String::new(),
            String::new(),
            "no-such-file-5e1b9c.sqlite".to_string(),
        );
        super::Endpoint {
            db,
            database: None,
            samples,
            hidden: Default::default(),
            schema,
            host,
            audit_as: None,
            audit_sink: None,
            read_only: false,
            level_from_flag: false,
        }
    }

    /// **A refused call is logged with the refusal, and an answered one never
    /// with its answer** — the agent log records what was asked, and an error's
    /// sentence; the rows a query returned are the data it exists not to copy.
    #[test]
    fn the_agent_log_line_holds_the_ask_and_an_errors_reason_only() {
        let e = unreachable(true, true, super::Host::Standalone);
        let args = serde_json::json!({ "sql": "SELECT * FROM t", "database": "shop" });
        let rows = super::Answer::with_rows("| a |\n| 1 |".to_string(), 1);
        let ok = super::audit_entry(&e, "prod", "run_query", &args, &rows);
        assert_eq!(ok.sql, "SELECT * FROM t");
        assert_eq!(ok.database.as_deref(), Some("shop"));
        assert_eq!(ok.detail.as_deref(), Some("run_query (mcp)"));
        assert_eq!(ok.error, None, "a successful answer is not copied");
        assert_eq!(ok.rows, Some(1), "its row count is");
        let said = ("Refused: not a read.".to_string(), true).into();
        let refused = super::audit_entry(&e, "prod", "run_query", &args, &said);
        assert_eq!(refused.error.as_deref(), Some("Refused: not a read."));
        assert_eq!(refused.rows, None);
        let panel = unreachable(true, true, super::Host::Panel);
        let described = super::audit_entry(
            &panel,
            "prod",
            "describe_table",
            &serde_json::json!({ "table": "orders" }),
            &("…".to_string(), false).into(),
        );
        assert_eq!(
            described.detail.as_deref(),
            Some("describe_table orders (panel)")
        );
    }

    /// **A comparison's line names the other side it read** — the source
    /// connection, or this one's other database — so an agent reading prod's
    /// whole catalogue is not recorded as a call on dev alone.
    #[test]
    fn a_comparisons_line_names_its_source() {
        let e = unreachable(true, true, super::Host::Standalone);
        let ok = ("Nothing has run.".to_string(), false).into();
        let across = super::audit_entry(
            &e,
            "dev",
            "compare_schemas",
            &serde_json::json!({ "database": "app", "source_connection": "prod", "source_database": "app" }),
            &ok,
        );
        assert_eq!(
            across.detail.as_deref(),
            Some("compare_schemas from prod.app (mcp)")
        );
        let same = super::audit_entry(
            &e,
            "dev",
            "compare_schemas",
            &serde_json::json!({ "database": "staging", "source_database": "secret_db" }),
            &ok,
        );
        assert_eq!(
            same.detail.as_deref(),
            Some("compare_schemas from dev.secret_db (mcp)")
        );
    }

    /// **`propose_sql` judges a statement under the endpoint's own read-only
    /// mark and database**, the call's database standing in for a missing
    /// default — the policy `answer` builds, pinned.
    #[test]
    fn propose_sql_takes_its_policy_from_the_endpoint() {
        let mut e = unreachable(true, true, super::Host::Panel);
        let none = serde_json::json!({});
        let p = super::propose_policy(&e, &none);
        assert!(!p.read_only && p.no_database && !p.confirm_writes);
        assert!(!super::propose_policy(&e, &serde_json::json!({ "database": "shop" })).no_database);
        e.read_only = true;
        e.database = Some("shop".to_string());
        let p = super::propose_policy(&e, &none);
        assert!(p.read_only && !p.no_database);
    }

    fn policy(read_only: bool, no_database: bool) -> schemaic_core::sql::GuardPolicy {
        schemaic_core::sql::GuardPolicy {
            read_only,
            confirm_writes: false,
            dialect: schemaic_core::intel::SqlDialect::MySql,
            no_database,
        }
    }

    /// **The guard's three answers, worded for the model** — and none of them
    /// tells it the statement ran, or that the guard has been satisfied.
    #[test]
    fn propose_sql_passes_on_the_guards_verdict() {
        let (ok, err) = super::propose_sql("UPDATE t SET a = 1 WHERE id = 1", policy(false, false));
        assert!(!err && ok.starts_with("Valid. Nothing has run."), "{ok}");
        assert!(ok.contains("```sql"), "{ok}");

        let (ask, err) = super::propose_sql("DELETE FROM t", policy(false, false));
        assert!(
            !err && ask.contains("ask the user before running it"),
            "{ask}"
        );

        let (refused, err) =
            super::propose_sql("UPDATE t SET a = 1 WHERE id = 1", policy(true, false));
        assert!(err && refused.starts_with("Refused:"), "{refused}");
        assert!(!refused.contains("```sql"), "a refusal hands nothing over");

        let (empty, err) = super::propose_sql("  ;  ", policy(false, false));
        assert!(err && empty.contains("needs the `sql`"), "{empty}");

        // No database on PostgreSQL: an unscoped CREATE would land in the
        // maintenance database, which the guard refuses outright.
        let mut pg = policy(false, true);
        pg.dialect = schemaic_core::intel::SqlDialect::Postgres;
        let (unscoped, err) = super::propose_sql("CREATE TABLE t (a int)", pg);
        assert!(err && unscoped.starts_with("Refused:"), "{unscoped}");
    }

    /// A read on a read-only connection is a read: valid, as Run would find it.
    #[test]
    fn propose_sql_lets_a_read_through_on_a_read_only_connection() {
        let (ok, err) = super::propose_sql("SELECT 1", policy(true, false));
        assert!(!err, "{ok}");
    }

    /// Panel-only, like `propose_table_change`: outside the panel nobody gives
    /// the code block Insert and Run.
    #[test]
    fn propose_sql_is_offered_in_the_panel_only_and_reads_nothing() {
        use super::{Host, McpTool};
        assert!(McpTool::ProposeSql.offered(false, false, Host::Panel));
        assert!(!McpTool::ProposeSql.offered(true, true, Host::Standalone));
        assert!(!McpTool::ProposeSql.reads_row_data() && !McpTool::ProposeSql.reads_schema());
    }

    fn tool_names(listing: &serde_json::Value) -> Vec<String> {
        listing
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect()
    }

    /// **One connection is the listing it always was** — no `connection`
    /// argument, no `list_connections` — which is what keeps the panel's
    /// server and every existing client unchanged.
    #[test]
    fn one_connection_lists_no_connection_argument() {
        let e = unreachable(true, true, super::Host::Standalone);
        let listing = super::listing(&[super::Named {
            name: "a",
            endpoint: &e,
        }]);
        assert!(!tool_names(&listing).contains(&"list_connections".to_string()));
        for t in listing.as_array().unwrap() {
            assert!(
                t["inputSchema"]["properties"]["connection"].is_null(),
                "{t}"
            );
        }
    }

    /// Several connections: every tool takes a `connection`, `list_connections`
    /// is offered, and a tool is listed when **any** connection offers it.
    #[test]
    fn several_connections_list_the_union_and_a_connection_argument() {
        let rows = unreachable(true, true, super::Host::Standalone);
        let no_rows = unreachable(false, true, super::Host::Standalone);
        let listing = super::listing(&[
            super::Named {
                name: "a",
                endpoint: &no_rows,
            },
            super::Named {
                name: "b",
                endpoint: &rows,
            },
        ]);
        let names = tool_names(&listing);
        assert!(names.contains(&"run_query".to_string()), "{names:?}");
        assert!(names.contains(&"list_connections".to_string()));
        for t in listing.as_array().unwrap() {
            if t["name"] != "list_connections" {
                assert!(
                    t["inputSchema"]["properties"]["connection"].is_object(),
                    "{t}"
                );
            }
        }
    }

    /// **`compare_schemas` reads catalogues and nothing else**: offered from
    /// both hosts, withheld at Schema context None, and never a row tool.
    #[test]
    fn compare_schemas_is_a_catalogue_tool() {
        use super::{Host, McpTool};
        let t = McpTool::CompareSchemas;
        assert!(t.reads_schema() && !t.reads_row_data() && t.read_only() && !t.needs_panel());
        assert!(t.offered(false, true, Host::Standalone));
        assert!(!t.offered(true, false, Host::Panel));
    }

    /// Its refusals come before anything is read: no database on a side, the
    /// same database on both, and two engines — each its own sentence.
    #[tokio::test]
    async fn compare_schemas_refuses_before_reading() {
        let e = unreachable(true, true, super::Host::Standalone);
        let pg = schemaic_db::Db::from_parts(
            schemaic_db::Engine::Postgres,
            "127.0.0.1".into(),
            1,
            String::new(),
            String::new(),
            String::new(),
        );
        let mut other = unreachable(true, true, super::Host::Standalone);
        other.db = pg;
        let (none, err) =
            super::compare_schemas((&other, "b"), (&other, "b"), &serde_json::json!({})).await;
        assert!(err && none.contains("a database on each side"), "{none}");
        let (same, err) = super::compare_schemas(
            (&e, "a"),
            (&e, "a"),
            &serde_json::json!({ "database": "main", "source_database": "main" }),
        )
        .await;
        assert!(err && same.contains("Both sides are main"), "{same}");
        // **SQLite's one database is `main` whatever a side is called**, as
        // `schemaic compare` resolves it: a name that is not there is not a
        // second database that happens to match.
        let (named, err) = super::compare_schemas(
            (&e, "a"),
            (&e, "a"),
            &serde_json::json!({ "source_database": "staging" }),
        )
        .await;
        assert!(err && named.contains("Both sides are main"), "{named}");
        let (engines, err) = super::compare_schemas(
            (&e, "a"),
            (&other, "b"),
            &serde_json::json!({ "database": "main", "source_database": "shop" }),
        )
        .await;
        assert!(err, "{engines}");
    }

    /// Across two connections it is answered in `serve_on`: a source that is
    /// not served is refused with the names, and a source whose own Schema
    /// context withholds the catalogue is refused in that one's words.
    #[tokio::test]
    async fn compare_schemas_across_connections_holds_each_side_to_its_levels() {
        let open = unreachable(true, true, super::Host::Standalone);
        let blind = unreachable(true, false, super::Host::Standalone);
        let named = [
            super::Named {
                name: "a",
                endpoint: &open,
            },
            super::Named {
                name: "b",
                endpoint: &blind,
            },
        ];
        let call = |args: &str| {
            format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":\
                 {{\"name\":\"compare_schemas\",\"arguments\":{args}}}}}\n"
            )
        };
        let text = |got: &[serde_json::Value]| {
            got[0]["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .to_string()
        };
        let unknown = exchange_many(&named, call(r#"{"source_connection":"c"}"#).as_bytes()).await;
        assert!(text(&unknown).contains("a, b"), "{}", text(&unknown));
        let refused = exchange_many(&named, call(r#"{"source_connection":"b"}"#).as_bytes()).await;
        assert!(text(&refused).starts_with("Refused:"), "{}", text(&refused));
        assert_eq!(refused[0]["result"]["isError"], true);
    }

    /// A tool needing rows and the catalogue is offered only by a connection
    /// with both — not by one with rows and another with the catalogue.
    #[test]
    fn the_union_asks_each_connection_its_own_levels() {
        let rows_only = unreachable(true, false, super::Host::Standalone);
        let schema_only = unreachable(false, true, super::Host::Standalone);
        let listing = super::listing(&[
            super::Named {
                name: "a",
                endpoint: &rows_only,
            },
            super::Named {
                name: "b",
                endpoint: &schema_only,
            },
        ]);
        let names = tool_names(&listing);
        assert!(!names.contains(&"sample_rows".to_string()), "{names:?}");
        assert!(names.contains(&"run_query".to_string()), "{names:?}");
        assert!(names.contains(&"list_schema".to_string()), "{names:?}");
    }

    /// **Every call is recorded, and under the connection it reached** —
    /// the four `record` sites driven through the real serve loop into an
    /// in-memory sink: a call naming `b`, `list_connections`, a name the
    /// server does not serve, a refusal, a cross-connection comparison, and a
    /// `connection` that is not a string, which is refused rather than
    /// answered on the default.
    #[tokio::test]
    async fn every_call_is_recorded_under_the_connection_it_reached() {
        let sink = super::AuditSink::default();
        let audited = |samples: bool, name: &str| {
            let mut e = unreachable(samples, true, super::Host::Standalone).audited_as(name);
            e.audit_sink = Some(sink.clone());
            e
        };
        let (a, b) = (audited(true, "a"), audited(false, "b"));
        let named = [
            super::Named {
                name: "a",
                endpoint: &a,
            },
            super::Named {
                name: "b",
                endpoint: &b,
            },
        ];
        let call = |tool: &str, args: &str| {
            format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":\
                 {{\"name\":\"{tool}\",\"arguments\":{args}}}}}\n"
            )
        };
        let input = [
            call("describe_table", r#"{"table":"t","connection":"b"}"#),
            call("list_connections", "{}"),
            call("run_query", r#"{"sql":"SELECT 1","connection":"c"}"#),
            call("run_query", r#"{"sql":"SELECT 1","connection":"b"}"#),
            call(
                "compare_schemas",
                r#"{"connection":"b","source_connection":"a"}"#,
            ),
            call("run_query", r#"{"sql":"SELECT 1","connection":2}"#),
        ]
        .concat();
        let got = exchange_many(&named, input.as_bytes()).await;
        let lines = sink.lock().unwrap();
        let under: Vec<&str> = lines.iter().map(|l| l.connection.as_str()).collect();
        assert_eq!(under, ["b", "a", "a", "b", "b", "a"], "{lines:?}");
        assert_eq!(
            lines[4].detail.as_deref(),
            Some("compare_schemas from a (mcp)")
        );
        // The number is refused, in words that say what a name is.
        let last = got.last().unwrap();
        assert_eq!(last["result"]["isError"], true);
        let text = last["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("as a string") && text.contains("a, b"),
            "{text}"
        );
    }

    /// **The panel's one connection answers to its saved name**, and its
    /// listing offers no `source_connection` to name another; a refusal never
    /// lists an empty set of names.
    #[test]
    fn a_one_connection_server_is_named_and_offers_no_other() {
        let e = unreachable(true, true, super::Host::Panel).audited_as("Local");
        assert_eq!(super::sole_name(&e), "Local");
        let only = [super::Named {
            name: "",
            endpoint: &e,
        }];
        let tools = super::listing(&only);
        let compare = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "compare_schemas")
            .unwrap();
        assert!(compare["inputSchema"]["properties"]["source_connection"].is_null());
        assert!(!super::serves(&only).ends_with("serves ."));
    }

    /// **`describe_table` promises sample rows only where every connection
    /// sends them** — a several-connection listing is asked of the OR of the
    /// levels, which promised rows on a connection that sends none.
    #[test]
    fn a_mixed_listing_does_not_promise_rows_everywhere() {
        let (rows, none) = (
            unreachable(true, true, super::Host::Standalone),
            unreachable(false, true, super::Host::Standalone),
        );
        let named = [
            super::Named {
                name: "prod",
                endpoint: &none,
            },
            super::Named {
                name: "stage",
                endpoint: &rows,
            },
        ];
        let tools = super::listing(&named);
        let d = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "describe_table")
            .unwrap()["description"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(d.contains("where the connection sends rows"), "{d}");
    }

    /// **Each call is judged by its own connection** — a tool the listing
    /// offers for one connection is refused for another that withholds it, in
    /// that one's words; an unknown name is refused with the names there are.
    #[tokio::test]
    async fn a_call_is_judged_by_the_connection_it_names() {
        let rows = unreachable(true, true, super::Host::Standalone);
        let no_rows = unreachable(false, true, super::Host::Standalone);
        let named = [
            super::Named {
                name: "a",
                endpoint: &rows,
            },
            super::Named {
                name: "b",
                endpoint: &no_rows,
            },
        ];
        let call = |args: &str| {
            format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":\
                 {{\"name\":\"run_query\",\"arguments\":{args}}}}}\n"
            )
        };
        let text = |got: &[serde_json::Value]| {
            got[0]["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .to_string()
        };
        let refused = exchange_many(
            &named,
            call(r#"{"sql":"SELECT 1","connection":"b"}"#).as_bytes(),
        )
        .await;
        assert!(text(&refused).starts_with("Refused:"), "{}", text(&refused));
        let unknown = exchange_many(
            &named,
            call(r#"{"sql":"SELECT 1","connection":"c"}"#).as_bytes(),
        )
        .await;
        assert!(text(&unknown).contains("a, b"), "{}", text(&unknown));
        assert_eq!(unknown[0]["result"]["isError"], true);
        let listed = exchange_many(
            &named,
            b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"list_connections\",\"arguments\":{}}}\n",
        )
        .await;
        let l = text(&listed);
        assert!(l.contains("a (the default)") && l.contains("- b"), "{l}");
    }

    /// A front end's endpoint is audited; the defaults are not, so no test
    /// that drives `call_tool` writes the user's log.
    #[test]
    fn only_an_audited_endpoint_logs() {
        let db = schemaic_db::Db::from_parts(
            schemaic_db::Engine::Sqlite,
            String::new(),
            0,
            String::new(),
            String::new(),
            "x.sqlite".to_string(),
        );
        let e = super::Endpoint::standalone(db, None, None);
        assert_eq!(e.audit_as, None);
        assert_eq!(e.audited_as("prod").audit_as.as_deref(), Some("prod"));
    }

    /// **`sample_rows`' `limit` is a count it can honour or a refusal** —
    /// never clamped into a number the model did not ask for, and never a
    /// string read as one.
    #[test]
    fn a_sample_limit_is_a_whole_number_in_range_or_refused() {
        use serde_json::json;
        assert_eq!(
            super::sample_limit(&json!({})),
            Ok(super::SAMPLE_ROWS_DEFAULT)
        );
        assert_eq!(
            super::sample_limit(&json!({ "limit": null })),
            Ok(super::SAMPLE_ROWS_DEFAULT)
        );
        assert_eq!(super::sample_limit(&json!({ "limit": 1 })), Ok(1));
        assert_eq!(
            super::sample_limit(&json!({ "limit": super::SAMPLE_ROWS_MAX })),
            Ok(super::SAMPLE_ROWS_MAX)
        );
        for bad in [
            json!(0),
            json!(super::SAMPLE_ROWS_MAX + 1),
            json!(-3),
            json!(2.5),
            json!("10"),
        ] {
            let err = super::sample_limit(&json!({ "limit": bad })).unwrap_err();
            assert!(
                err.contains(&super::SAMPLE_ROWS_MAX.to_string()),
                "{bad}: {err}"
            );
        }
    }

    /// **`list_databases` is names only**, with the database a call naming
    /// none lands in marked — the one fact a model needs before it picks.
    #[test]
    fn the_database_names_mark_the_default_and_point_at_list_schema() {
        let names = vec!["shop".to_string(), "archive".to_string()];
        let out = super::format_database_names(&names, Some("shop"));
        assert!(out.starts_with("- shop (default)\n- archive\n"), "{out}");
        assert!(out.contains("(2 databases)"), "{out}");
        assert!(out.contains("list_schema"), "{out}");
        let one = super::format_database_names(&names[..1], None);
        assert!(
            one.contains("(1 database)") && !one.contains("default"),
            "{one}"
        );
    }

    /// **The new tools are gated through the call**, at the gate each one
    /// answers: `sample_rows` reads rows and resolves a name against the
    /// catalogue, so either setting off refuses it; `list_databases` reads
    /// the catalogue alone.
    ///
    /// On the stack the real server runs on (`run::block_on_own_stack`):
    /// several calls through the serve loop in one future outgrow a debug
    /// test thread's 2 MiB.
    #[test]
    fn the_new_tools_are_refused_at_their_own_gates() {
        crate::run::block_on_own_stack(the_new_tools_gates).expect("the runtime starts");
    }

    async fn the_new_tools_gates() {
        let ep = unreachable(false, true, super::Host::Standalone);
        assert_eq!(
            call(&ep, "sample_rows", r#"{"table":"t"}"#).await,
            (super::NO_DATA_ACCESS_STANDALONE.to_string(), true)
        );
        let ep = unreachable(true, false, super::Host::Standalone);
        for (tool, args) in [
            ("sample_rows", r#"{"table":"t"}"#),
            ("list_databases", "{}"),
        ] {
            assert_eq!(
                call(&ep, tool, args).await,
                (super::NO_SCHEMA_ACCESS.to_string(), true),
                "{tool}"
            );
        }
        // Past the gates, a malformed call is told what it lacks before
        // anything is read.
        let ep = unreachable(true, true, super::Host::Standalone);
        let (text, err) = call(&ep, "sample_rows", "{}").await;
        assert!(err && text.contains("`table`"), "{text}");
        let (text, err) = call(&ep, "sample_rows", r#"{"table":"t","limit":0}"#).await;
        assert!(err && text.contains("`limit`"), "{text}");
    }

    /// One `tools/call`'s text and `isError`, through the real serve loop.
    async fn call(endpoint: &super::Endpoint, tool: &str, args: &str) -> (String, bool) {
        let line = format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\
             \"params\":{{\"name\":\"{tool}\",\"arguments\":{args}}}}}\n"
        );
        let got = exchange_on(endpoint, line.as_bytes()).await;
        assert_eq!(got.len(), 1, "{got:?}");
        let r = &got[0]["result"];
        (
            r["content"][0]["text"].as_str().unwrap_or("").to_string(),
            r["isError"].as_bool().unwrap_or(false),
        )
    }

    /// **The gate on the call, through the call.** `refusal_for` is tested on
    /// its own; its composition with `call_tool` — which endpoint field is
    /// passed as which argument, and the `Host` — was not, so forcing
    /// `Host::Panel` at the call site kept the suite green while a standalone
    /// client working from a stale tool list reached `propose_table_change`.
    /// `search_objects` reads the catalogue, so *Schema context: None* withholds
    /// it as it does `list_schema` — a name search is an existence oracle over
    /// exactly what that setting publishes none of. It reads no rows, so the
    /// data level does not.
    #[tokio::test]
    async fn search_objects_is_a_schema_tool_and_not_a_data_one() {
        let tool = McpTool::SearchObjects;
        assert!(tool.reads_schema() && !tool.reads_row_data() && tool.read_only());
        let ep = unreachable(false, false, super::Host::Standalone);
        assert_eq!(
            call(&ep, "search_objects", r#"{"query":"cust"}"#).await,
            (super::NO_SCHEMA_ACCESS.to_string(), true)
        );
        // Offered at the lowest data level, refused only for want of a query —
        // before anything is dialled.
        let ep = unreachable(false, true, super::Host::Standalone);
        let (text, err) = call(&ep, "search_objects", r#"{"query":"  "}"#).await;
        assert!(err && text.contains("needs a `query`"), "{text}");
    }

    #[tokio::test]
    async fn a_standalone_call_to_a_withheld_tool_is_refused_by_the_server() {
        let ep = unreachable(false, true, super::Host::Standalone);
        assert_eq!(
            call(&ep, "run_query", r#"{"sql":"SELECT 1"}"#).await,
            (super::NO_DATA_ACCESS_STANDALONE.to_string(), true)
        );
        assert_eq!(
            call(
                &ep,
                "propose_table_change",
                r#"{"table":"t","request":"x"}"#
            )
            .await,
            (super::NO_PANEL.to_string(), true)
        );
    }

    /// **A `--url` connection's refusal names the flag**: it was never saved,
    /// so "set it in Schemaic's connection settings" sends the user looking
    /// for a setting that does not exist. Through the serve loop, for the
    /// composition's sake.
    #[tokio::test]
    async fn a_url_connections_withheld_rows_name_the_flag() {
        let ep = unreachable(false, true, super::Host::Standalone).with_level_from_flag(true);
        for (tool, args) in [
            ("run_query", r#"{"sql":"SELECT 1"}"#),
            ("explain", r#"{"sql":"SELECT 1"}"#),
        ] {
            assert_eq!(
                call(&ep, tool, args).await,
                (super::NO_DATA_ACCESS_URL.to_string(), true),
                "{tool}"
            );
        }
        assert!(super::NO_DATA_ACCESS_URL.contains("--ai-data full"));
        assert!(!super::NO_DATA_ACCESS_URL.contains("connection settings"));
        // Only the advice changes: a level that allows rows refuses nothing,
        // and a refusal for another reason keeps its own words.
        let open = unreachable(true, true, super::Host::Standalone).with_level_from_flag(true);
        assert_eq!(super::refusal_on(&open, McpTool::RunQuery), None);
        assert_eq!(
            super::refusal_on(&open, McpTool::ProposeSql),
            Some(super::NO_PANEL)
        );
    }

    /// And the two levels are not crossed on the way in: a panel endpoint
    /// that may read rows but not schema refuses the schema tool.
    #[tokio::test]
    async fn a_call_is_gated_by_its_own_level_not_the_others() {
        let ep = unreachable(true, false, super::Host::Panel);
        assert_eq!(
            call(&ep, "list_schema", "{}").await,
            (super::NO_SCHEMA_ACCESS.to_string(), true)
        );
        let ep = unreachable(false, true, super::Host::Panel);
        assert_eq!(
            call(&ep, "run_query", r#"{"sql":"SELECT 1"}"#).await,
            (super::NO_DATA_ACCESS.to_string(), true)
        );
    }

    /// **`explain` stands wherever `run_query` does, and behind its gate.** A
    /// plan reads the data (MySQL's const-table "Impossible WHERE" answers
    /// whether a key exists), so the data level withholds it; and the statement
    /// it plans is refused by `run_query`'s own read-only gate before anything
    /// is dialled — the endpoint here reaches nothing, so getting past the gate
    /// would read as a connection failure rather than a rejection.
    #[tokio::test]
    async fn explain_is_gated_as_run_query_is() {
        for host in [super::Host::Panel, super::Host::Standalone] {
            for reads in [false, true] {
                for schema in [false, true] {
                    assert_eq!(
                        McpTool::Explain.offered(reads, schema, host),
                        McpTool::RunQuery.offered(reads, schema, host),
                        "{host:?} rows={reads} schema={schema}"
                    );
                }
            }
        }
        let ep = unreachable(false, true, super::Host::Standalone);
        assert_eq!(
            call(&ep, "explain", r#"{"sql":"SELECT 1"}"#).await,
            (super::NO_DATA_ACCESS_STANDALONE.to_string(), true)
        );
        let ep = unreachable(true, true, super::Host::Standalone);
        // Straight to the tool rather than through `call`: the gate's parse
        // under the serve loop overflows a debug test thread's 2 MiB stack —
        // `run_query`'s too — where the real server runs on its own
        // (`run::block_on_own_stack`).
        let (text, err) = super::explain(&ep.db, None, "DELETE FROM t").await;
        assert!(err && text.starts_with("Rejected: "), "{text}");
        assert_eq!(
            super::explain(&ep.db, None, " ; ").await,
            ("Empty query.".to_string(), true)
        );
    }

    /// **A refusal of the database layer's says the query never ran**, then
    /// the refusal's own words, which say what would get past it. No advice of
    /// its own: for a shadowed builtin a rewrite is useless, for a plan MySQL
    /// will not make read-only it is the answer.
    #[test]
    fn a_database_refusal_says_the_query_never_ran() {
        let (text, err) = super::no_rows_reply(crate::query::NoRows::DbRefused(
            "Refused before your statement ran: public.lower is …".into(),
        ));
        assert!(err);
        assert_eq!(
            text,
            "Rejected before the query ran: Refused before your statement ran: public.lower is …"
        );
    }

    /// **A server's plan reaches the model as the tool writes it**: SQLite's
    /// `EXPLAIN QUERY PLAN` columns, as `Db::explain` returns them (its own
    /// test, `a_read_only_plan_runs_on_a_query_only_connection`, runs them on
    /// an in-memory database — a `file:` URI this crate cannot open, since the
    /// boundary refuses one outside `schemaic-db`'s own tests), read by
    /// `QueryPlan::from_result` and laid out by `format_plan`.
    #[test]
    fn a_sqlite_plan_reaches_the_model_whole() {
        use schemaic_core::model::{Column, ResultSet, Value};
        let col = |n: &str| Column {
            name: n.to_string(),
            type_name: String::new(),
            origin: None,
        };
        let rs = ResultSet::from_rows(
            vec![col("id"), col("parent"), col("notused"), col("detail")],
            vec![vec![
                Value::Str("2".into()),
                Value::Str("0".into()),
                Value::Str("216".into()),
                Value::Str("SCAN t".into()),
            ]],
        );
        let out = super::format_plan(&schemaic_core::plan::QueryPlan::from_result(&rs));
        assert!(out.contains("| detail |"), "{out}");
        assert!(out.contains("SCAN t"), "{out}");
        assert!(out.ends_with("(1 plan row)"), "{out}");
    }

    /// **A plan is held to the row cap a query is**, and says when it was cut:
    /// a partitioned table's plan has two lines per partition, and a capped
    /// plan handed over as whole is a wrong answer.
    #[test]
    fn a_long_plan_stops_at_the_cap_and_says_so() {
        let plan = schemaic_core::plan::QueryPlan {
            columns: vec!["QUERY PLAN".to_string()],
            rows: (0..MCP_ROW_CAP + 5)
                .map(|i| vec![format!("Seq Scan on p_{i} {}", "x".repeat(200))])
                .collect(),
            ..Default::default()
        };
        let out = super::format_plan(&plan);
        assert_eq!(out.matches("Seq Scan on p_").count(), MCP_ROW_CAP, "{out}");
        assert!(
            out.contains(&format!("({MCP_ROW_CAP}+ plan rows, capped)")),
            "{out}"
        );
        assert!(
            out.lines()
                .all(|l| l.chars().count() < super::MCP_CELL_CHARS + 20),
            "cells trimmed as run_query's are"
        );
        // Cut by the server's own cap: the rows that came are all there, and
        // the note still says it is not the whole plan.
        let server_cut = schemaic_core::plan::QueryPlan {
            columns: vec!["QUERY PLAN".to_string()],
            rows: vec![vec!["Seq Scan on a".to_string()]],
            truncated: true,
            ..Default::default()
        };
        assert!(
            super::format_plan(&server_cut).contains("(1+ plan rows, capped)"),
            "{}",
            super::format_plan(&server_cut)
        );
        let whole = schemaic_core::plan::QueryPlan {
            columns: vec!["QUERY PLAN".to_string()],
            rows: vec![vec!["Seq Scan on a".to_string()]],
            ..Default::default()
        };
        assert!(!super::format_plan(&whole).contains("capped"));
    }

    /// **A line that is not UTF-8 is skipped, not the end of the session.**
    /// `while let Ok(Some(line))` ended the loop on the first read error, and
    /// the process exited 0 as though the client had hung up — the server
    /// vanished mid-session with nothing on stderr.
    #[tokio::test]
    async fn a_line_that_is_not_utf8_does_not_end_the_session() {
        let got =
            exchange(b"\xff\xfe not text\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n")
                .await;
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0]["id"], 1);
        assert_eq!(got[0]["result"], serde_json::json!({}));
    }

    /// **Every tool tells the model its results are data** (security review
    /// S3). The panel's own prompts fence what they read from the database
    /// (`core::prompt`), but a tool result reached the model with nothing
    /// saying so, and a row value or a column comment phrased as an
    /// instruction — to call `propose_table_change`, say — had only the
    /// model's judgement against it. Every tool reads rows or the catalogue,
    /// so every description carries the note.
    #[test]
    fn every_tool_says_its_results_are_data_not_instructions() {
        let tools = super::tools_list(schemaic_db::Engine::Postgres, true, true, Host::Panel);
        let tools = tools.as_array().expect("a list");
        assert!(!tools.is_empty());
        for t in tools {
            let description = t["description"].as_str().expect("a description");
            assert!(
                description.ends_with(schemaic_core::prompt::TOOL_RESULT_NOTE),
                "{}: {description}",
                t["name"]
            );
        }
    }

    /// **`run_query` takes a `database`, like the tools beside it.** It had
    /// none, so the one database a standalone agent could query was the
    /// endpoint's default — while `list_schema` showed it every other — and a
    /// `database` it passed anyway was dropped: `SELECT count(*) FROM track`
    /// with `"database": "chinook"` ran in `world` and answered `relation
    /// "track" does not exist`. On PostgreSQL nothing else reaches another
    /// database at all.
    #[test]
    fn run_query_takes_a_database_and_uses_it() {
        let tools = super::tools_list(schemaic_db::Engine::Postgres, true, true, Host::Standalone);
        let props = &tools[0]["inputSchema"]["properties"];
        assert_eq!(tools[0]["name"], "run_query");
        assert!(props.get("database").is_some(), "{props}");
        assert_eq!(
            tools[0]["inputSchema"]["required"],
            serde_json::json!(["sql"])
        );
        let args = serde_json::json!({"sql": "SELECT 1", "database": "chinook"});
        assert_eq!(super::tool_database(&args, Some("world")), Some("chinook"));
        let args = serde_json::json!({"sql": "SELECT 1"});
        assert_eq!(super::tool_database(&args, Some("world")), Some("world"));
        let args = serde_json::json!({"sql": "SELECT 1", "database": ""});
        assert_eq!(super::tool_database(&args, Some("world")), Some("world"));
        assert_eq!(super::tool_database(&args, None), None);
    }

    /// **`describe_table` promises sample rows only where it sends them.** At
    /// every level below "Let it read data" — the standalone default — the
    /// model was told to expect rows it would never get, and planned the turn
    /// around them.
    #[test]
    fn describe_table_promises_samples_only_when_it_sends_them() {
        let describe = |samples| {
            super::tools_list(schemaic_db::Engine::MySql, samples, true, Host::Standalone)
                .as_array()
                .expect("a list")
                .iter()
                .find(|t| t["name"] == "describe_table")
                .expect("offered")["description"]
                .as_str()
                .expect("a description")
                .to_string()
        };
        assert!(describe(true).contains("sample rows"), "{}", describe(true));
        assert!(
            !describe(false).contains("sample rows"),
            "{}",
            describe(false)
        );
    }

    fn standalone(ai_data: Option<schemaic_core::connection::AiData>) -> super::Endpoint {
        let db = schemaic_db::Db::from_parts(
            schemaic_db::Engine::MySql,
            "127.0.0.1".to_string(),
            3306,
            String::new(),
            String::new(),
            String::new(),
        );
        super::Endpoint::standalone(db, Some("shop".to_string()), ai_data)
    }

    /// **Only "Let it read data" lets an outside agent read rows**, the level
    /// whose consent covers a value the user did not hand over. "Only what I
    /// attach" is the default and means no rows here: an outside client has
    /// no attach gesture, so its half of that level is empty rather than
    /// promoted.
    #[test]
    fn a_standalone_endpoint_reads_rows_only_at_full_access() {
        use schemaic_core::connection::AiData;
        for (level, rows) in [
            (AiData::SchemaOnly, false),
            (AiData::OnRequest, false),
            (AiData::Full, true),
        ] {
            let e = standalone(Some(level));
            assert_eq!(e.samples, rows, "{level:?}");
            assert_eq!(e.host, Host::Standalone);
            assert!(
                e.schema,
                "{level:?}: the catalogue is cli_access's to publish"
            );
            assert!(e.hidden.is_empty());
            assert_eq!(e.database.as_deref(), Some("shop"));
        }
    }

    /// **A connection that never chose a level is not `Full`.** The app
    /// resolves `None` at startup from the legacy global flag — which defaulted
    /// *on* — and the CLI cannot read that flag. Guessing the migration's
    /// answer would hand rows to an agent on a connection whose owner was
    /// never asked; the default level is the one that sends none.
    #[test]
    fn a_connection_that_never_chose_a_level_sends_no_rows() {
        assert!(!standalone(None).samples);
    }

    /// **A SQLite connection has a database even when nobody names it.** The
    /// panel always passes the one selected in the tree (`main`); standalone
    /// there is no tree, and `describe_table {"table": "orders"}` answered "the
    /// connection has no default — pass a `database`" about a file that has
    /// exactly one. A server engine with no default stays `None`: there the
    /// model has to pick, and `list_schema` shows it the names.
    ///
    /// **And a named one does not displace it on SQLite**, `catalog.rs`'s rule:
    /// every operation opens its own connection, so nothing is ever attached,
    /// and a `SCHEMAIC_DATABASE` exported for another engine would otherwise
    /// point every tool at a database that cannot exist.
    #[test]
    fn a_standalone_endpoint_defaults_to_the_engines_only_database() {
        let db = |engine| {
            schemaic_db::Db::from_parts(
                engine,
                String::new(),
                0,
                String::new(),
                String::new(),
                "shop.db".to_string(),
            )
        };
        let e = super::Endpoint::standalone(db(schemaic_db::Engine::Sqlite), None, None);
        assert_eq!(e.database.as_deref(), Some("main"));
        for engine in [schemaic_db::Engine::MySql, schemaic_db::Engine::Postgres] {
            let e = super::Endpoint::standalone(db(engine), None, None);
            assert_eq!(e.database, None, "{engine:?}");
        }
        let e = super::Endpoint::standalone(
            db(schemaic_db::Engine::Sqlite),
            Some("shop".to_string()),
            None,
        );
        assert_eq!(e.database.as_deref(), Some("main"));
        for engine in [schemaic_db::Engine::MySql, schemaic_db::Engine::Postgres] {
            let e = super::Endpoint::standalone(db(engine), Some("shop".to_string()), None);
            assert_eq!(e.database.as_deref(), Some("shop"), "{engine:?}");
        }
    }

    /// The tool's arguments carry `database` alongside the proposal's own
    /// fields; the proposal type refuses unknown keys, so it has to come off
    /// before parsing or every call fails.
    #[test]
    fn a_proposal_parses_out_of_the_tools_arguments() {
        let args = serde_json::json!({
            "database": "shop",
            "table": "orders",
            "ops": [{"add_column": {"name": "deleted_at", "type": "datetime"}}]
        });
        let p = proposal_from_args(&args).expect("parses");
        assert_eq!(p.table, "orders");
        assert_eq!(p.ops.len(), 1);
    }

    /// And nothing *else* unknown gets the same pass — a model inventing a field
    /// must be told, because a silently-dropped key is a change the user was
    /// promised and wouldn't get.
    #[test]
    fn an_invented_argument_is_still_refused() {
        let args = serde_json::json!({
            "database": "shop",
            "table": "orders",
            "cascade": true,
            "ops": []
        });
        assert!(proposal_from_args(&args).is_err());
    }

    /// The proposal tool has to advertise the fence tag the app actually reads,
    /// or the model writes a block nothing picks up and the user is told a
    /// change is waiting that never arrives.
    #[test]
    fn the_proposal_tool_advertises_the_tag_the_app_looks_for() {
        for engine in [
            schemaic_db::Engine::MySql,
            schemaic_db::Engine::Postgres,
            schemaic_db::Engine::Sqlite,
        ] {
            let tools = super::tools_list(engine, true, true, Host::Panel);
            let tool = tools
                .as_array()
                .expect("a list")
                .iter()
                .find(|t| t["name"] == "propose_table_change")
                .expect("the tool is offered");
            let description = tool["description"].as_str().expect("a description");
            assert!(
                description.contains(schemaic_core::propose::FENCE_TAG),
                "{engine:?} doesn't name the tag the app extracts on"
            );
        }
    }

    /// What `run_query` advertises is what the gate enforces, per engine. The
    /// description used to be a fixed `SELECT/SHOW/DESCRIBE/EXPLAIN/WITH`, so a
    /// SQLite connection told the model it could `SHOW TABLES` — a statement
    /// SQLite has no syntax for, and the model had only a parser error to learn
    /// from. This walks the advertised heads back through the gate.
    #[test]
    fn run_query_advertises_exactly_the_heads_its_gate_allows() {
        for engine in [
            schemaic_db::Engine::MySql,
            schemaic_db::Engine::Postgres,
            schemaic_db::Engine::Sqlite,
        ] {
            let dialect = engine.dialect();
            let tools = super::tools_list(engine, true, true, Host::Standalone);
            let desc = tools[0]["description"].as_str().expect("a description");
            assert_eq!(tools[0]["name"], "run_query");
            assert!(
                desc.contains(dialect.engine_label()),
                "{engine:?} is not named: {desc}"
            );
            // Every head it names is one the gate accepts…
            for head in schemaic_core::sql::read_only_heads(dialect) {
                assert!(desc.contains(head), "{engine:?} omits {head}: {desc}");
                let sql = if *head == "WITH" {
                    "WITH c AS (SELECT 1) SELECT * FROM c".to_string()
                } else {
                    format!("{head} t")
                };
                assert!(
                    schemaic_core::sql::read_only_reason(&sql, dialect).is_ok(),
                    "{engine:?} advertises {head} but the gate refuses it"
                );
            }
            // …and it names no head the gate would refuse.
            for head in ["SELECT", "SHOW", "DESCRIBE", "EXPLAIN", "WITH"] {
                if desc.contains(head) {
                    assert!(
                        schemaic_core::sql::read_only_heads(dialect).contains(&head),
                        "{engine:?} advertises {head}, which its gate rejects: {desc}"
                    );
                }
            }
        }
    }

    /// Every engine reaches its **own** dialect. This was an
    /// `if Postgres { … } else { MySql }`, which compiles cleanly while sorting a
    /// third engine onto whichever side it happens to fall — so a SQLite
    /// connection had the read-only gate lexed by MySQL's rules and its sample
    /// query quoted against MySQL's reserved words.
    #[test]
    fn the_dialect_is_the_engines_own_for_every_engine() {
        let db = |engine| {
            schemaic_db::Db::from_parts(
                engine,
                String::new(),
                0,
                String::new(),
                String::new(),
                String::new(),
            )
        };
        for (engine, want) in [
            (schemaic_db::Engine::MySql, SqlDialect::MySql),
            (schemaic_db::Engine::Postgres, SqlDialect::Postgres),
            (schemaic_db::Engine::Sqlite, SqlDialect::Sqlite),
        ] {
            assert_eq!(dialect_of(&db(engine)), want, "{engine:?}");
        }
    }
    use schemaic_core::intel::SqlDialect;
    use schemaic_core::model::{Column, ResultSet, Value};
    use schemaic_core::schema::{ColumnInfo, DbSchema, ForeignKeyInfo, IndexInfo, TableInfo};

    fn tbl(name: &str, columns: Vec<ColumnInfo>) -> TableInfo {
        TableInfo {
            name: name.to_string(),
            columns,
            ..Default::default()
        }
    }

    fn c(name: &str, type_name: &str, nullable: bool, primary_key: bool) -> ColumnInfo {
        ColumnInfo {
            name: name.to_string(),
            type_name: type_name.to_string(),
            nullable,
            primary_key,
            ..Default::default()
        }
    }

    fn fk(columns: &[&str], ref_table: &str, ref_columns: &[&str]) -> ForeignKeyInfo {
        ForeignKeyInfo {
            name: format!("fk_{ref_table}"),
            columns: columns.iter().map(|s| s.to_string()).collect(),
            ref_schema: None,
            ref_table: ref_table.to_string(),
            ref_columns: ref_columns.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn database_list_names_tables_and_marks_views() {
        let mut view = tbl("v_active", vec![c("id", "int", true, false)]);
        view.is_view = true;
        let dbs = vec![(
            "shop".to_string(),
            Ok(DbSchema {
                tables: vec![tbl("orders", vec![c("id", "int", false, true)]), view],
                ..Default::default()
            }),
        )];
        let out = format_database_list(&dbs, true);
        assert!(out.contains("## shop (2 tables)"));
        assert!(out.contains("- orders\n"));
        assert!(out.contains("- v_active (view)"));
        // No columns at this level — that's what the drill-down calls are for.
        assert!(!out.contains("int"));
        assert!(out.contains("describe_table"));
    }

    /// **The overview's pointer at `describe_table` promises rows only where
    /// they are sent**, like the tool's own description. Its footer said
    /// "sample rows" at every level, so below "Let it read data" the first
    /// `list_schema` of a session re-made the promise the listing had just
    /// stopped making.
    #[test]
    fn the_overview_promises_samples_only_when_it_sends_them() {
        let dbs = || vec![("shop".to_string(), Ok(DbSchema::default()))];
        assert!(format_database_list(&dbs(), true).contains("sample rows"));
        let out = format_database_list(&dbs(), false);
        assert!(!out.contains("sample rows"), "{out}");
        assert!(out.contains("describe_table"), "{out}");
    }

    #[test]
    fn the_overview_leaves_out_a_database_the_user_hid() {
        let names = || {
            vec![
                "sakila".to_string(),
                "archive".to_string(),
                "shop".to_string(),
            ]
        };
        let hidden: HashSet<String> = ["archive".to_string()].into_iter().collect();
        assert_eq!(
            listed_databases(names(), &hidden, Some("sakila")),
            vec!["sakila".to_string(), "shop".to_string()]
        );
        // Nothing hidden → the server's own list, in the server's own order.
        assert_eq!(listed_databases(names(), &HashSet::new(), None), names());
    }

    #[test]
    fn the_session_still_sees_the_database_it_is_pointed_at() {
        // The exception every other surface makes: hiding the database the user
        // is working in doesn't move the tab, and an assistant that couldn't
        // name the database its own queries default to would be useless there.
        let hidden: HashSet<String> = ["archive".to_string()].into_iter().collect();
        assert_eq!(
            listed_databases(vec!["archive".to_string()], &hidden, Some("archive")),
            vec!["archive".to_string()]
        );
        assert!(listed_databases(vec!["archive".to_string()], &hidden, Some("shop")).is_empty());
    }

    /// `search_objects` matches table and view names case-insensitively, as
    /// Find Anywhere does, and qualifies them as `list_schema` would.
    #[test]
    fn a_search_names_the_matching_tables_and_views_per_database() {
        let mut view = tbl("v_customers", vec![]);
        view.is_view = true;
        let dbs = vec![
            (
                "shop".to_string(),
                Ok(DbSchema {
                    tables: vec![tbl("Customers", vec![]), tbl("orders", vec![]), view],
                    ..Default::default()
                }),
            ),
            (
                "hr".to_string(),
                Ok(DbSchema {
                    tables: vec![tbl("staff", vec![])],
                    ..Default::default()
                }),
            ),
            ("locked".to_string(), Err("access denied".to_string())),
        ];
        let out = format_search_hits("CUST", &dbs, false);
        assert!(
            out.contains("## shop\n- Customers\n- v_customers (view)\n"),
            "{out}"
        );
        assert!(!out.contains("orders"), "{out}");
        assert!(
            !out.contains("## hr"),
            "a database with no hit is left out: {out}"
        );
        assert!(out.contains("## locked (error: access denied)"), "{out}");
        assert!(out.contains("(2 matches)"), "{out}");
    }

    /// Given one database, the search reaches its columns too — after the
    /// object names, the order Find Anywhere searches in.
    #[test]
    fn a_search_of_one_database_also_names_matching_columns() {
        let dbs = vec![(
            "shop".to_string(),
            Ok(DbSchema {
                tables: vec![
                    tbl("orders", vec![c("customer_id", "int", false, false)]),
                    tbl("customers", vec![c("id", "int", false, true)]),
                ],
                ..Default::default()
            }),
        )];
        let out = format_search_hits("customer", &dbs, true);
        let tables = out.find("- customers\n").expect("the table");
        let column = out.find("- orders.customer_id\n").expect("the column");
        assert!(tables < column, "{out}");
        assert!(out.contains("(2 matches)"), "{out}");
        assert!(
            !format_search_hits("customer", &dbs, false).contains("customer_id"),
            "columns only when they were read"
        );
    }

    #[test]
    fn a_search_that_finds_nothing_says_so_and_where_it_looked() {
        let dbs = vec![("shop".to_string(), Ok(DbSchema::default()))];
        let out = format_search_hits("zzz", &dbs, false);
        assert!(
            out.starts_with("No table or view name contains \"zzz\""),
            "{out}"
        );
        assert!(out.contains("1 database"), "{out}");
    }

    /// A long hit list stops at the row cap and says so, rather than spending
    /// the model's context on every `log_*` table of a fifty-database server.
    #[test]
    fn a_search_stops_at_the_cap_and_says_so() {
        let tables = (0..MCP_ROW_CAP + 5)
            .map(|i| tbl(&format!("log_{i}"), vec![]))
            .collect();
        let dbs = vec![(
            "shop".to_string(),
            Ok(DbSchema {
                tables,
                ..Default::default()
            }),
        )];
        let out = format_search_hits("log", &dbs, false);
        assert_eq!(out.matches("\n- log_").count(), MCP_ROW_CAP, "{out}");
        assert!(
            out.contains(&format!("({MCP_ROW_CAP}+ matches, capped)")),
            "{out}"
        );
    }

    #[test]
    fn database_list_reports_a_failed_introspection() {
        let dbs = vec![("locked".to_string(), Err("access denied".to_string()))];
        let out = format_database_list(&dbs, true);
        assert!(out.contains("## locked (error: access denied)"));
    }

    #[test]
    fn database_schema_marks_keys_not_null_and_foreign_keys() {
        let mut orders = tbl(
            "orders",
            vec![
                c("id", "int", false, true),
                c("customer_id", "int", false, false),
                c("total", "decimal(10,2)", true, false),
            ],
        );
        orders.foreign_keys = vec![fk(&["customer_id"], "customers", &["id"])];
        let schema = DbSchema {
            tables: vec![orders],
            ..Default::default()
        };
        let out = format_database_schema("shop", &schema);
        assert!(out.contains("## shop"));
        assert!(out.contains("### orders"));
        assert!(out.contains("  id int PK NOT NULL"));
        assert!(out.contains("  customer_id int NOT NULL"));
        // Nullable is the default — no marker, no noise.
        assert!(out.contains("  total decimal(10,2)\n"));
        assert!(out.contains("  FK: customer_id -> customers.id"));
    }

    /// A partition is named under its parent, not listed as a table of its
    /// own with the parent's columns and keys again.
    #[test]
    fn database_schema_names_partitions_under_their_parent() {
        let mut events = tbl("events", vec![c("id", "int", false, true)]);
        events.partitioning = Some(schemaic_core::schema::Partitioning {
            key: "RANGE (id)".into(),
            ..Default::default()
        });
        let mut part = tbl("events_2024", vec![c("id", "int", false, true)]);
        part.partition_of = Some(schemaic_core::schema::PartitionOf {
            schema: None,
            parent: "events".into(),
            bound: "FOR VALUES FROM (1) TO (10)".into(),
            ..Default::default()
        });
        let schema = DbSchema {
            tables: vec![events, part],
            ..Default::default()
        };
        let out = format_database_schema("shop", &schema);
        assert!(
            out.contains("### events (partitioned by RANGE (id))\n"),
            "{out}"
        );
        assert!(
            out.contains("  partition events_2024: FOR VALUES FROM (1) TO (10)\n"),
            "{out}"
        );
        assert!(!out.contains("### events_2024"), "{out}");
    }

    /// **A partition that is itself partitioned names its own partitions**,
    /// one level further in. Only the top level's children were listed, so
    /// PostgreSQL's sub-partitions were nowhere in the assistant's view.
    #[test]
    fn database_schema_names_sub_partitions_under_their_partition() {
        let part_of = |parent: &str, bound: &str| schemaic_core::schema::PartitionOf {
            schema: None,
            parent: parent.into(),
            bound: bound.into(),
            ..Default::default()
        };
        let mut events = tbl("events", vec![c("id", "int", false, true)]);
        events.partitioning = Some(schemaic_core::schema::Partitioning {
            key: "RANGE (id)".into(),
            ..Default::default()
        });
        let mut y2024 = tbl("events_2024", vec![c("id", "int", false, true)]);
        y2024.partition_of = Some(part_of("events", "FOR VALUES FROM (1) TO (10)"));
        y2024.partitioning = Some(schemaic_core::schema::Partitioning {
            key: "HASH (id)".into(),
            ..Default::default()
        });
        let mut h0 = tbl("events_2024_h0", vec![c("id", "int", false, true)]);
        h0.partition_of = Some(part_of(
            "events_2024",
            "FOR VALUES WITH (modulus 2, remainder 0)",
        ));
        let out = format_database_schema(
            "shop",
            &DbSchema {
                tables: vec![events, y2024, h0],
                ..Default::default()
            },
        );
        assert!(
            out.contains(
                "  partition events_2024: FOR VALUES FROM (1) TO (10) (partitioned by HASH (id))\n    \
                 partition events_2024_h0: FOR VALUES WITH (modulus 2, remainder 0)\n"
            ),
            "{out}"
        );
        assert!(!out.contains("### events_2024"), "{out}");
    }

    /// MySQL keeps no subpartition names, only the clause; it is named with
    /// the key, so the assistant knows the second level exists.
    #[test]
    fn database_schema_names_a_mysql_subpartition_clause() {
        let mut r = tbl("r", vec![c("y", "int", false, false)]);
        r.partitioning = Some(schemaic_core::schema::Partitioning {
            key: "RANGE (`y`)".into(),
            partitions: Vec::new(),
            subpartition: Some("HASH (`y`) SUBPARTITIONS 2".into()),
        });
        let out = format_database_schema(
            "shop",
            &DbSchema {
                tables: vec![r],
                ..Default::default()
            },
        );
        assert!(
            out.contains(
                "### r (partitioned by RANGE (`y`), subpartitioned by HASH (`y`) SUBPARTITIONS 2)\n"
            ),
            "{out}"
        );
    }

    /// Where a partition is storage inside its table (MySQL), the table's
    /// own partitions are named under it the same way.
    #[test]
    fn database_schema_names_a_mysql_tables_partitions() {
        let mut r = tbl("r", vec![c("y", "int", false, false)]);
        r.partitioning = Some(schemaic_core::schema::Partitioning {
            key: "RANGE (`y`)".into(),
            partitions: vec![schemaic_core::schema::Partition {
                name: "pmax".into(),
                schema: None,
                bound: "VALUES LESS THAN MAXVALUE".into(),
            }],
            subpartition: None,
        });
        let out = format_database_schema(
            "shop",
            &DbSchema {
                tables: vec![r],
                ..Default::default()
            },
        );
        assert!(
            out.contains(
                "### r (partitioned by RANGE (`y`))\n  partition pmax: VALUES LESS THAN MAXVALUE\n"
            ),
            "{out}"
        );
    }

    #[test]
    fn database_schema_renders_a_composite_foreign_key() {
        let mut t = tbl(
            "line_items",
            vec![
                c("order_id", "int", false, true),
                c("line_no", "int", false, true),
            ],
        );
        t.foreign_keys = vec![fk(&["order_id", "line_no"], "shipments", &["ord", "line"])];
        let out = format_database_schema(
            "shop",
            &DbSchema {
                tables: vec![t],
                ..Default::default()
            },
        );
        assert!(out.contains("  FK: (order_id, line_no) -> shipments(ord, line)"));
    }

    #[test]
    fn database_schema_marks_a_view() {
        let mut view = tbl("v_active", vec![c("id", "int", true, false)]);
        view.is_view = true;
        let out = format_database_schema(
            "shop",
            &DbSchema {
                tables: vec![view],
                ..Default::default()
            },
        );
        assert!(out.contains("### v_active (view)"));
    }

    #[test]
    fn database_schema_reports_an_empty_database() {
        let out = format_database_schema("shop", &DbSchema::default());
        assert!(out.contains("(no tables)"));
    }

    #[test]
    fn table_detail_carries_ddl_and_foreign_keys() {
        let mut orders = tbl(
            "orders",
            vec![
                c("id", "int", false, true),
                c("customer_id", "int", false, false),
            ],
        );
        orders.indexes = vec![IndexInfo {
            foreign: true,
            ..IndexInfo::plain("ix_customer", vec!["customer_id"], false)
        }];
        orders.foreign_keys = vec![fk(&["customer_id"], "customers", &["id"])];
        let out = format_table_detail(&orders, SqlDialect::MySql, false);
        // The DDL skeleton carries columns + PK + indexes…
        assert!(out.contains("CREATE TABLE `orders`"));
        assert!(out.contains("PRIMARY KEY (`id`)"));
        assert!(out.contains("KEY `ix_customer`"));
        // …but never FK references, so they get their own section.
        assert!(out.contains("Foreign keys:\n- customer_id -> customers.id"));
    }

    fn in_schema(mut t: TableInfo, schema: &str) -> TableInfo {
        t.schema = Some(schema.to_string());
        t
    }

    #[test]
    fn multi_schema_database_qualifies_every_table_including_public() {
        // `display_name` leaves a `public` table bare, which is unambiguous on
        // its own but not beside `sales.orders` — a model reading the bare name
        // invented `warehouse.orders` (database.table, invalid in PostgreSQL).
        let schema = DbSchema {
            tables: vec![
                in_schema(tbl("orders", vec![c("id", "int", false, true)]), "public"),
                in_schema(tbl("orders", vec![c("id", "int", false, true)]), "sales"),
            ],
            ..Default::default()
        };
        let out = format_database_schema("warehouse", &schema);
        assert!(out.contains("### public.orders"));
        assert!(out.contains("### sales.orders"));
        // The overview qualifies the same way.
        let list = format_database_list(&[("warehouse".to_string(), Ok(schema))], true);
        assert!(list.contains("- public.orders"));
        assert!(list.contains("- sales.orders"));
    }

    #[test]
    fn single_schema_database_leaves_public_tables_bare() {
        // One namespace → no ambiguity to resolve, and `public.` prefixes would
        // just be noise. MySQL tables (no namespace at all) take this path too.
        let pg = DbSchema {
            tables: vec![in_schema(
                tbl("orders", vec![c("id", "int", false, true)]),
                "public",
            )],
            ..Default::default()
        };
        let out = format_database_schema("shop", &pg);
        assert!(out.contains("### orders"));
        assert!(!out.contains("public."));

        let mysql = DbSchema {
            tables: vec![tbl("orders", vec![c("id", "int", false, true)])],
            ..Default::default()
        };
        assert!(format_database_schema("shop", &mysql).contains("### orders"));
    }

    #[test]
    fn multi_schema_database_qualifies_foreign_key_targets() {
        let mut child = in_schema(
            tbl("daily_revenue", vec![c("order_id", "int", false, false)]),
            "analytics",
        );
        child.foreign_keys = vec![ForeignKeyInfo {
            columns: vec!["order_id".to_string()],
            ref_schema: Some("public".to_string()),
            ref_table: "orders".to_string(),
            ref_columns: vec!["id".to_string()],
            ..Default::default()
        }];
        let schema = DbSchema {
            tables: vec![
                child,
                in_schema(tbl("orders", vec![c("id", "int", false, true)]), "public"),
            ],
            ..Default::default()
        };
        let out = format_database_schema("warehouse", &schema);
        // A `public` FK target is spelled out too, or the edge reads as though it
        // pointed at whatever `orders` means here.
        assert!(out.contains("FK: order_id -> public.orders.id"));
    }

    #[test]
    fn table_detail_heading_is_a_name_the_dialect_can_actually_resolve() {
        // MySQL: the database *is* the namespace, so `db.table` is valid SQL.
        let t = tbl("payments", vec![c("id", "int", false, true)]);
        let out = format_table_heading("classicmodels", &t, SqlDialect::MySql, false);
        assert_eq!(out, "### classicmodels.payments\n");

        // PostgreSQL: `warehouse.sales.orders` is not a thing, so the heading
        // carries the schema-qualified name and states the database separately.
        let t = in_schema(tbl("orders", vec![c("id", "int", false, true)]), "sales");
        let out = format_table_heading("warehouse", &t, SqlDialect::Postgres, true);
        assert!(out.starts_with("### sales.orders\n"));
        assert!(out.contains("Database: warehouse"));
        assert!(!out.contains("warehouse.sales"));
    }

    #[test]
    fn lookup_resolves_every_name_the_listing_advertises() {
        let schema = DbSchema {
            tables: vec![
                in_schema(tbl("orders", vec![c("id", "int", false, true)]), "public"),
                in_schema(tbl("orders", vec![c("id", "int", false, true)]), "sales"),
                tbl("payments", vec![c("id", "int", false, true)]),
            ],
            ..Default::default()
        };
        let found = |n: &str| find_described(&schema, n).map(|t| t.schema.clone());
        // A multi-schema listing prints `public.orders`, so that name has to
        // resolve — `display_name` renders a `public` table bare, so the plain
        // display lookup never matches it.
        assert_eq!(found("public.orders"), Some(Some("public".to_string())));
        assert_eq!(found("sales.orders"), Some(Some("sales".to_string())));
        // A bare name still lands on `public` (the display-name rule).
        assert_eq!(found("orders"), Some(Some("public".to_string())));
        // MySQL tables carry no namespace.
        assert_eq!(found("payments"), Some(None));
        assert!(find_described(&schema, "nope").is_none());
    }

    #[test]
    fn table_detail_omits_the_fk_section_when_there_are_none() {
        let t = tbl("logs", vec![c("id", "int", false, true)]);
        let out = format_table_detail(&t, SqlDialect::MySql, false);
        assert!(!out.contains("Foreign keys:"));
    }

    // `normalize_stmt`'s tests went with the function, to
    // `crate::query`, which owns the one copy both front ends now call.

    fn col(name: &str) -> Column {
        Column {
            name: name.to_string(),
            type_name: "VARCHAR".to_string(),
            origin: None,
        }
    }

    #[test]
    fn empty_columns_reported() {
        assert_eq!(format_table(&ResultSet::default()), "(no columns)");
    }

    #[test]
    fn renders_pipe_table_with_header_and_row_count() {
        let rs = ResultSet::from_rows(
            vec![col("id"), col("name")],
            vec![
                vec![Value::Int(1), Value::Str("Ada".into())],
                vec![Value::Null, Value::Str("Bo".into())],
            ],
        );
        let out = format_table(&rs);
        assert!(out.contains("| id | name |"));
        assert!(out.contains("| --- | --- |"));
        assert!(out.contains("| 1 | Ada |"));
        assert!(out.contains("| NULL | Bo |"));
        assert!(out.ends_with("(2 rows)"));
    }

    #[test]
    fn escapes_pipes_and_newlines_and_truncates_long_cells() {
        let rs = ResultSet::from_rows(
            vec![col("v")],
            vec![
                vec![Value::Str("a|b\nc".into())],
                vec![Value::Str("x".repeat(80))],
            ],
        );
        let out = format_table(&rs);
        // Pipe escaped, newline flattened to a space.
        assert!(out.contains(r"a\|b c"));
        // Long value truncated to 60 chars + ellipsis.
        assert!(out.contains(&format!("{}…", "x".repeat(60))));
        assert!(!out.contains(&"x".repeat(61)));
    }

    #[test]
    fn truncated_result_notes_capped_rows() {
        let rs =
            ResultSet::from_rows(vec![col("id")], vec![vec![Value::Int(1)]]).with_truncated(true);
        assert!(format_table(&rs).ends_with("(1+ rows, capped)"));
    }

    #[test]
    fn a_supported_protocol_is_agreed_to() {
        assert_eq!(negotiate_protocol(Some("2024-11-05")), "2024-11-05");
    }

    #[test]
    fn an_unknown_protocol_gets_one_we_really_speak() {
        // The handshake exists to catch a skew. Echoing the request back claims
        // fluency in a protocol whose tool-result shape and error convention we
        // don't implement, and the failure that follows looks like empty results
        // rather than a version mismatch.
        assert_eq!(
            negotiate_protocol(Some("2099-01-01")),
            SUPPORTED_PROTOCOLS[0]
        );
        assert_eq!(negotiate_protocol(Some("")), SUPPORTED_PROTOCOLS[0]);
        // A client that names no version gets the same answer.
        assert_eq!(negotiate_protocol(None), SUPPORTED_PROTOCOLS[0]);
    }

    /// **Two revisions, and the answer is the newest one the client can
    /// have heard of.** A client names the latest it speaks, so anything
    /// newer is one it does not — answering `2025-03-26` (which this server
    /// does not speak: it requires accepting JSON-RPC batches) with
    /// `2025-06-18` would hand an older client a version it must walk away
    /// from, where `2024-11-05` is one it certainly knows.
    #[test]
    fn the_answer_is_the_newest_version_the_client_can_know() {
        assert_eq!(negotiate_protocol(Some("2025-06-18")), "2025-06-18");
        assert_eq!(negotiate_protocol(Some("2024-11-05")), "2024-11-05");
        // Between ours: the newest we speak that is not newer than asked.
        assert_eq!(negotiate_protocol(Some("2025-03-26")), "2024-11-05");
        // Newer than anything we speak: our newest, for the client to judge.
        assert_eq!(negotiate_protocol(Some("2025-11-25")), "2025-06-18");
        // Older than everything we speak: the nearest, our oldest.
        assert_eq!(negotiate_protocol(Some("2024-10-07")), "2024-11-05");
        // Not a revision at all: our newest.
        assert_eq!(negotiate_protocol(Some("latest")), "2025-06-18");
    }

    /// The version a session agreed on, read off the real handshake, and the
    /// tool list that session is then given.
    async fn session(protocol: &str) -> (String, Vec<serde_json::Value>) {
        let input = format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\
             \"params\":{{\"protocolVersion\":\"{protocol}\"}}}}\n\
             {{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}}\n"
        );
        let got = exchange(input.as_bytes()).await;
        let agreed = got[0]["result"]["protocolVersion"]
            .as_str()
            .expect("a version")
            .to_string();
        let tools = got[1]["result"]["tools"]
            .as_array()
            .expect("a list")
            .clone();
        (agreed, tools)
    }

    /// **Every tool says it is read-only — to a session that can hear it.**
    /// `annotations` arrived in `2025-03-26`; a `2024-11-05` session gets the
    /// list exactly as it always did, since a strict client of that revision
    /// has no field by that name.
    #[tokio::test]
    async fn a_2025_session_is_told_every_tool_is_read_only() {
        let (agreed, tools) = session("2025-06-18").await;
        assert_eq!(agreed, "2025-06-18");
        assert!(!tools.is_empty());
        for t in &tools {
            assert_eq!(
                t["annotations"]["readOnlyHint"],
                serde_json::json!(true),
                "{t}"
            );
        }
        let (agreed, tools) = session("2024-11-05").await;
        assert_eq!(agreed, "2024-11-05");
        for t in &tools {
            assert!(t.get("annotations").is_none(), "{t}");
        }
    }

    #[test]
    fn the_supported_list_is_newest_first_and_never_empty() {
        // `SUPPORTED_PROTOCOLS[0]` is what an unknown request is answered with,
        // so the ordering is load-bearing rather than cosmetic.
        assert!(!SUPPORTED_PROTOCOLS.is_empty());
        let mut sorted = SUPPORTED_PROTOCOLS.to_vec();
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        assert_eq!(sorted, SUPPORTED_PROTOCOLS);
    }
}
