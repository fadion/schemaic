//! Claude Code CLI integration for Schemaic's AI panel.
//!
//! We drive one long-lived `claude` process per conversation in streaming mode
//! (`-p --input-format stream-json --output-format stream-json --include-partial-messages`):
//! user turns are written to stdin as JSON lines and the response streams back
//! as JSONL events. This crate is pure — arg building, the stdin encoder, and
//! parsing/accumulating the event stream into a renderable transcript. The app
//! owns the subprocess and the async→UI marshalling.

use schemaic_core::transcript::{Seg, ToolCall, TurnStats};

/// SQL keywords + schema identifiers come from the app; this is the tool set we
/// forbid so the assistant stays a conversational SQL helper (the MCP DB tools
/// are allow-listed separately by the app).
const DISALLOWED_TOOLS: &[&str] = &[
    "Bash",
    "Edit",
    "Write",
    "Read",
    "Glob",
    "Grep",
    "NotebookEdit",
    "WebFetch",
    "WebSearch",
    "Task",
];

/// Build the args for a persistent streaming session.
///
/// `mcp_config_json` (if set) is passed to `--mcp-config` and its tools are
/// allow-listed so the assistant can call them without an interactive prompt.
pub fn build_session_args(
    system_context: &str,
    model: Option<&str>,
    effort: Option<&str>,
    mcp_config_json: Option<&str>,
    mcp_tools: &[&str],
) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "-p".into(),
        "--input-format".into(),
        "stream-json".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
        "--include-partial-messages".into(),
        "--permission-mode".into(),
        "default".into(),
    ];
    if let Some(m) = model {
        a.push("--model".into());
        a.push(m.into());
    }
    if let Some(e) = effort {
        a.push("--effort".into());
        a.push(e.into());
    }
    if !system_context.is_empty() {
        a.push("--append-system-prompt".into());
        a.push(system_context.into());
    }
    if let Some(cfg) = mcp_config_json {
        a.push("--mcp-config".into());
        a.push(cfg.into());
        if !mcp_tools.is_empty() {
            a.push("--allowedTools".into());
            for t in mcp_tools {
                a.push((*t).into());
            }
        }
    }
    // Variadic — keep last so it doesn't swallow later flags.
    a.push("--disallowedTools".into());
    for t in DISALLOWED_TOOLS {
        a.push((*t).into());
    }
    a
}

/// Args for a one-shot inline (Ctrl+K) generation: `-p <intent>
/// --append-system-prompt <system> --model <model>`. Pure — the app spawns
/// `claude` with these — so the flag set/order is unit-tested.
pub fn inline_args(intent: &str, system: &str, model: &str) -> Vec<String> {
    vec![
        "-p".into(),
        intent.into(),
        "--append-system-prompt".into(),
        system.into(),
        "--model".into(),
        model.into(),
    ]
}

/// Encode a user turn as a `stream-json` stdin line (newline-terminated).
pub fn user_message_line(text: &str) -> String {
    let v = serde_json::json!({
        "type": "user",
        "message": { "role": "user", "content": text }
    });
    format!("{v}\n")
}

/// A meaningful event decoded from one `stream-json` output line.
#[derive(Clone, Debug)]
pub enum StreamEvent {
    /// A streamed chunk of assistant text.
    TextDelta(String),
    /// The assistant invoked a tool (SQL captured when it's `run_query`).
    ToolUse { name: String, sql: Option<String> },
    /// A tool returned a result.
    ToolResult { text: String, is_error: bool },
    /// The turn finished, with its cost/usage summary.
    TurnDone { is_error: bool, stats: TurnStats },
}

/// Parse one output line into zero or more [`StreamEvent`]s.
pub fn parse_stream_line(line: &str) -> Vec<StreamEvent> {
    let line = line.trim();
    if line.is_empty() {
        return Vec::new();
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        return Vec::new();
    };
    match v.get("type").and_then(|t| t.as_str()) {
        // Live token stream.
        Some("stream_event") => {
            if v.pointer("/event/type").and_then(|t| t.as_str()) == Some("content_block_delta")
                && v.pointer("/event/delta/type").and_then(|t| t.as_str()) == Some("text_delta")
                && let Some(t) = v.pointer("/event/delta/text").and_then(|t| t.as_str())
            {
                return vec![StreamEvent::TextDelta(t.to_string())];
            }
            Vec::new()
        }
        // Full assistant message — used for tool_use blocks (text is streamed).
        Some("assistant") => {
            let mut out = Vec::new();
            if let Some(blocks) = v.pointer("/message/content").and_then(|c| c.as_array()) {
                for b in blocks {
                    if b.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                        let name = b
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("")
                            .to_string();
                        let sql = b
                            .pointer("/input/sql")
                            .or_else(|| b.pointer("/input/query"))
                            .and_then(|s| s.as_str())
                            .map(|s| s.to_string());
                        out.push(StreamEvent::ToolUse { name, sql });
                    }
                }
            }
            out
        }
        // Tool results arrive as a synthetic user message.
        Some("user") => {
            let mut out = Vec::new();
            if let Some(blocks) = v.pointer("/message/content").and_then(|c| c.as_array()) {
                for b in blocks {
                    if b.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                        out.push(StreamEvent::ToolResult {
                            text: tool_result_text(b),
                            is_error: b.get("is_error").and_then(|e| e.as_bool()).unwrap_or(false),
                        });
                    }
                }
            }
            out
        }
        Some("result") => vec![StreamEvent::TurnDone {
            is_error: v.get("is_error").and_then(|b| b.as_bool()).unwrap_or(false),
            stats: parse_stats(&v),
        }],
        _ => Vec::new(),
    }
}

/// Pull the cost/usage summary out of a `result` event.
fn parse_stats(v: &serde_json::Value) -> TurnStats {
    let u64_at = |ptr: &str| v.pointer(ptr).and_then(|n| n.as_u64());
    TurnStats {
        duration_ms: v.get("duration_ms").and_then(|d| d.as_u64()),
        input_tokens: u64_at("/usage/input_tokens"),
        output_tokens: u64_at("/usage/output_tokens"),
    }
}

/// Extract text from a `tool_result` block (content is a string or blocks).
fn tool_result_text(block: &serde_json::Value) -> String {
    match block.get("content") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(|i| i.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Accumulates a turn's events into an ordered list of renderable segments:
/// assistant prose interleaved with tool-call chips (each carrying the SQL it
/// ran and, once it returns, the result).
#[derive(Default)]
pub struct TurnState {
    segs: Vec<Seg>,
}

impl TurnState {
    pub fn apply(&mut self, ev: &StreamEvent) {
        match ev {
            StreamEvent::TextDelta(t) => {
                if let Some(Seg::Text(s)) = self.segs.last_mut() {
                    s.push_str(t);
                } else {
                    self.segs.push(Seg::Text(t.clone()));
                }
            }
            StreamEvent::ToolUse { name, sql } => {
                // De-dup: the assistant often prints the SQL in a fenced block
                // *and* then runs it. Drop that echoed fence from the prose so
                // the SQL shows once — in the chip.
                if let Some(sql) = sql
                    && let Some(Seg::Text(s)) = self.segs.last_mut()
                {
                    strip_matching_fence(s, sql);
                    if s.trim().is_empty() {
                        self.segs.pop();
                    }
                }
                self.segs.push(Seg::Tool(ToolCall {
                    name: name.clone(),
                    sql: sql.clone(),
                    result: None,
                    is_error: false,
                }));
            }
            StreamEvent::ToolResult { text, is_error } => {
                // Attach to the most recent tool call still awaiting a result.
                if let Some(Seg::Tool(tc)) = self
                    .segs
                    .iter_mut()
                    .rev()
                    .find(|s| matches!(s, Seg::Tool(tc) if tc.result.is_none()))
                {
                    tc.result = Some(text.clone());
                    tc.is_error = *is_error;
                }
            }
            StreamEvent::TurnDone { .. } => {}
        }
    }

    /// The accumulated segments (trimmed prose), ready to render.
    pub fn segments(&self) -> Vec<Seg> {
        self.segs
            .iter()
            .map(|s| match s {
                Seg::Text(t) => Seg::Text(t.trim_matches('\n').to_string()),
                Seg::Tool(tc) => Seg::Tool(tc.clone()),
            })
            .filter(|s| !matches!(s, Seg::Text(t) if t.is_empty()))
            .collect()
    }
}

/// Remove a fenced code block from `text` whose body matches `sql`
/// (whitespace-insensitive). Leaves `text` untouched if there's no match.
fn strip_matching_fence(text: &mut String, sql: &str) {
    let target = normalize_ws(sql);
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
    let mut i = 0;
    let mut stripped = false;
    while i < lines.len() {
        if !stripped && lines[i].trim_start().starts_with("```") {
            // Find the closing fence.
            if let Some(close) =
                (i + 1..lines.len()).find(|&j| lines[j].trim_start().starts_with("```"))
            {
                let body = lines[i + 1..close].join("\n");
                if normalize_ws(&body) == target {
                    i = close + 1; // skip the whole block
                    stripped = true;
                    continue;
                }
            }
        }
        out.push(lines[i]);
        i += 1;
    }
    if stripped {
        *text = out.join("\n");
    }
}

/// Normalize for loose SQL equality: collapse whitespace and lowercase, so
/// reformatting (indentation, keyword case) between the prose echo and the
/// actual tool call doesn't defeat de-dup. Only used to hide a cosmetic echo
/// that still shows in the chip, so over-matching is harmless.
fn normalize_ws(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assistant_tool_use(name: &str, sql: &str) -> String {
        serde_json::json!({
            "type": "assistant",
            "message": { "content": [
                { "type": "tool_use", "name": name, "input": { "sql": sql } }
            ] }
        })
        .to_string()
    }

    #[test]
    fn strip_fence_removes_matching_sql_block() {
        let mut t =
            "Let me count them:\n```sql\nSELECT COUNT(*)\n  FROM customers\n```\n".to_string();
        strip_matching_fence(&mut t, "select count(*) from customers");
        assert!(!t.contains("SELECT"), "echoed fence should be gone: {t:?}");
        assert!(t.contains("Let me count them"));
    }

    #[test]
    fn strip_fence_keeps_non_matching_block() {
        let mut t = "```sql\nSELECT 1\n```".to_string();
        strip_matching_fence(&mut t, "SELECT 2");
        assert!(t.contains("SELECT 1"));
    }

    #[test]
    fn tool_use_dedups_echoed_sql_into_chip() {
        let mut turn = TurnState::default();
        for ev in parse_stream_line(&stream_text("Here's the query:\n```sql\nSELECT 1\n```")) {
            turn.apply(&ev);
        }
        for ev in parse_stream_line(&assistant_tool_use("mcp__schemaic__run_query", "SELECT 1")) {
            turn.apply(&ev);
        }
        let segs = turn.segments();
        // One prose seg (without the SQL) + one tool chip carrying the SQL.
        let tools: Vec<_> = segs
            .iter()
            .filter_map(|s| match s {
                Seg::Tool(tc) => Some(tc),
                _ => None,
            })
            .collect();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].sql.as_deref(), Some("SELECT 1"));
        assert_eq!(tools[0].short_name(), "run_query");
        let prose: String = segs
            .iter()
            .filter_map(|s| match s {
                Seg::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert!(prose.contains("Here's the query"));
        assert!(
            !prose.contains("SELECT 1"),
            "SQL should live only in the chip"
        );
    }

    #[test]
    fn tool_result_attaches_to_pending_call() {
        let mut turn = TurnState::default();
        for ev in parse_stream_line(&assistant_tool_use("run_query", "SELECT 1")) {
            turn.apply(&ev);
        }
        turn.apply(&StreamEvent::ToolResult {
            text: "| n |\n| 1 |".into(),
            is_error: false,
        });
        let Seg::Tool(tc) = &turn.segments()[0] else {
            panic!("expected a tool seg")
        };
        assert_eq!(tc.result.as_deref(), Some("| n |\n| 1 |"));
        assert!(!tc.is_error);
    }

    #[test]
    fn parses_turn_stats_from_result() {
        let line = serde_json::json!({
            "type": "result",
            "is_error": false,
            "total_cost_usd": 0.01234,
            "duration_ms": 1500u64,
            "usage": { "input_tokens": 1234u64, "output_tokens": 340u64 }
        })
        .to_string();
        let evs = parse_stream_line(&line);
        let StreamEvent::TurnDone { is_error, stats } = &evs[0] else {
            panic!("expected TurnDone")
        };
        assert!(!is_error);
        assert_eq!(stats.duration_ms, Some(1500));
        assert_eq!(stats.summary(), "1.5s  ·  ↑1.2k ↓340");
    }

    fn stream_text(s: &str) -> String {
        // A single text_delta stream event carrying `s`.
        serde_json::json!({
            "type": "stream_event",
            "event": {
                "type": "content_block_delta",
                "delta": { "type": "text_delta", "text": s }
            }
        })
        .to_string()
    }

    /// Position of `flag` in `args`, or None.
    fn pos(args: &[String], flag: &str) -> Option<usize> {
        args.iter().position(|a| a == flag)
    }

    #[test]
    fn session_args_include_streaming_flags_and_disallowed_last() {
        let a = build_session_args("", None, None, None, &[]);
        // Core streaming flags present.
        assert!(pos(&a, "-p").is_some());
        assert!(pos(&a, "--input-format").is_some());
        assert!(pos(&a, "--output-format").is_some());
        assert!(pos(&a, "--include-partial-messages").is_some());
        // No optional flags when their inputs are absent.
        assert!(pos(&a, "--model").is_none());
        assert!(pos(&a, "--effort").is_none());
        assert!(pos(&a, "--append-system-prompt").is_none());
        assert!(pos(&a, "--mcp-config").is_none());
        assert!(pos(&a, "--allowedTools").is_none());
        // The variadic --disallowedTools is last, followed only by tool names.
        let d = pos(&a, "--disallowedTools").expect("disallowedTools present");
        assert!(a[d + 1..].contains(&"Bash".to_string()));
        assert!(a[d + 1..].contains(&"WebSearch".to_string()));
        assert_eq!(a[d + 1..].len(), DISALLOWED_TOOLS.len());
    }

    #[test]
    fn session_args_thread_model_effort_and_system_prompt() {
        let a = build_session_args("ctx", Some("claude-opus-4-8"), Some("high"), None, &[]);
        let m = pos(&a, "--model").unwrap();
        assert_eq!(a[m + 1], "claude-opus-4-8");
        let e = pos(&a, "--effort").unwrap();
        assert_eq!(a[e + 1], "high");
        let s = pos(&a, "--append-system-prompt").unwrap();
        assert_eq!(a[s + 1], "ctx");
    }

    #[test]
    fn session_args_allowlist_mcp_tools_only_with_config() {
        // Tools without a config are NOT allow-listed (guarded on mcp_config_json).
        let a = build_session_args("", None, None, None, &["mcp__schemaic__run_query"]);
        assert!(pos(&a, "--allowedTools").is_none());
        // With a config, the tools follow --allowedTools before --disallowedTools.
        let a = build_session_args(
            "",
            None,
            None,
            Some("{\"mcpServers\":{}}"),
            &["mcp__schemaic__run_query", "mcp__schemaic__list_schema"],
        );
        let cfg = pos(&a, "--mcp-config").unwrap();
        assert_eq!(a[cfg + 1], "{\"mcpServers\":{}}");
        let al = pos(&a, "--allowedTools").unwrap();
        let dis = pos(&a, "--disallowedTools").unwrap();
        assert!(al < dis, "allowed before disallowed");
        assert!(a[al + 1..dis].contains(&"mcp__schemaic__run_query".to_string()));
        assert!(a[al + 1..dis].contains(&"mcp__schemaic__list_schema".to_string()));
    }

    #[test]
    fn inline_args_flags_in_order() {
        let a = inline_args("count rows", "SCHEMA", "claude-opus-4-8");
        assert_eq!(
            a,
            vec![
                "-p",
                "count rows",
                "--append-system-prompt",
                "SCHEMA",
                "--model",
                "claude-opus-4-8",
            ]
        );
    }

    #[test]
    fn user_message_line_is_json_with_trailing_newline() {
        let line = user_message_line("hello \"world\"");
        assert!(line.ends_with('\n'));
        let v: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(v["type"], "user");
        assert_eq!(v["message"]["role"], "user");
        assert_eq!(v["message"]["content"], "hello \"world\"");
    }

    #[test]
    fn parse_stream_line_ignores_blank_malformed_and_unknown() {
        assert!(parse_stream_line("").is_empty());
        assert!(parse_stream_line("   ").is_empty());
        assert!(parse_stream_line("{not json").is_empty());
        assert!(parse_stream_line(r#"{"type":"system"}"#).is_empty());
        // Well-formed JSON with no "type" field.
        assert!(parse_stream_line(r#"{"foo":1}"#).is_empty());
    }

    #[test]
    fn parse_stream_line_decodes_text_delta() {
        let evs = parse_stream_line(&stream_text("chunk"));
        assert_eq!(evs.len(), 1);
        let StreamEvent::TextDelta(t) = &evs[0] else {
            panic!("expected TextDelta")
        };
        assert_eq!(t, "chunk");
    }

    #[test]
    fn tool_result_text_handles_string_array_and_missing() {
        // String content.
        let line = serde_json::json!({
            "type": "user",
            "message": { "content": [
                { "type": "tool_result", "content": "plain text", "is_error": true }
            ] }
        })
        .to_string();
        let evs = parse_stream_line(&line);
        let StreamEvent::ToolResult { text, is_error } = &evs[0] else {
            panic!("expected ToolResult")
        };
        assert_eq!(text, "plain text");
        assert!(is_error);

        // Array-of-blocks content joins the text fields.
        let line = serde_json::json!({
            "type": "user",
            "message": { "content": [
                { "type": "tool_result", "content": [
                    { "type": "text", "text": "line1" },
                    { "type": "text", "text": "line2" }
                ] }
            ] }
        })
        .to_string();
        let evs = parse_stream_line(&line);
        let StreamEvent::ToolResult { text, is_error } = &evs[0] else {
            panic!("expected ToolResult")
        };
        assert_eq!(text, "line1\nline2");
        assert!(!is_error); // missing is_error defaults to false

        // Missing content → empty text.
        let line = serde_json::json!({
            "type": "user",
            "message": { "content": [ { "type": "tool_result" } ] }
        })
        .to_string();
        let evs = parse_stream_line(&line);
        let StreamEvent::ToolResult { text, .. } = &evs[0] else {
            panic!("expected ToolResult")
        };
        assert_eq!(text, "");
    }

    #[test]
    fn tool_use_captures_query_alias_for_sql() {
        // The `input.query` alias is captured when `input.sql` is absent.
        let line = serde_json::json!({
            "type": "assistant",
            "message": { "content": [
                { "type": "tool_use", "name": "run_query", "input": { "query": "SELECT 9" } }
            ] }
        })
        .to_string();
        let evs = parse_stream_line(&line);
        let StreamEvent::ToolUse { name, sql } = &evs[0] else {
            panic!("expected ToolUse")
        };
        assert_eq!(name, "run_query");
        assert_eq!(sql.as_deref(), Some("SELECT 9"));
    }
}
