//! Claude Code CLI integration for Schemaic's AI panel.
//!
//! We drive one long-lived `claude` process per conversation in streaming mode
//! (`-p --input-format stream-json --output-format stream-json --include-partial-messages`):
//! user turns are written to stdin as JSON lines and the response streams back
//! as JSONL events. This crate is pure — arg building, the stdin encoder, and
//! parsing/accumulating the event stream into a renderable transcript. The app
//! owns the subprocess and the async→UI marshalling.

use schemaic_core::transcript::{Seg, ToolCall, TurnStats};

/// A **backstop**, not the guard. `--tools ""` is what leaves the session with
/// no built-in tools; this list names the ones whose misuse would be worst, so
/// that a future CLI which stops honouring `--tools` does not silently hand the
/// SQL assistant a shell.
///
/// It is deliberately not a *complete* denylist, because a complete denylist is
/// not maintainable: the first ten names here were the whole of it, and the CLI
/// they run grew nineteen more they had never heard of — every one live in the
/// AI panel, and three of them (`Skill`, `ToolSearch`, `Monitor`) measured
/// *running inside a turn without raising a permission request at all*.
/// Enumerating what to refuse is the wrong shape; naming what to allow is
/// [`build_session_args`]'s job.
///
/// **It is a snapshot, and it has to name the worst of what it can see.** The
/// second group below is that measured nineteen. Listing only the first ten left
/// the backstop covering the shell and the filesystem while `Artifact` — which
/// publishes a page to the web — `SendMessage`, `RemoteTrigger` and `CronCreate`
/// walked past it, so the fallback did not fall back on anything that mattered.
/// A name the CLI does not know is harmless here; a name missing from it is not.
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
    // Measured live in the panel against the shipped CLI, none of them known to
    // the ten above. Ordered as the `system`/`init` event reported them.
    "Artifact",
    "CronCreate",
    "CronDelete",
    "CronList",
    "DesignSync",
    "EnterWorktree",
    "ExitWorktree",
    "ListAgents",
    "Monitor",
    "PushNotification",
    "RemoteTrigger",
    "ReportFindings",
    "ScheduleWakeup",
    "SendMessage",
    "Skill",
    "TaskOutput",
    "TaskStop",
    "ToolSearch",
    "Workflow",
];

/// Which of the sealing flags the detected `claude` understands.
///
/// The flags below are not universally old, and Schemaic spawns whatever binary
/// the user has: passing one an older CLI does not know is not a degraded
/// session but a dead one — it exits with `error: unknown option '--tools'`
/// before the first turn, and the AI panel is gone until the user upgrades. So
/// the flags are asked for rather than assumed.
///
/// **A capability, not a version.** The question is whether *this* binary
/// accepts the flag, which its own `--help` answers directly; a version number
/// only stands in for that answer and needs a table mapping releases to flags
/// that nothing here can keep true.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CliSeal {
    pub tools: bool,
    pub setting_sources: bool,
    pub strict_mcp_config: bool,
}

impl CliSeal {
    /// Every flag passed — a current CLI, and the answer for an *unreadable*
    /// probe. See [`seal_from_help`] for why that is the safe direction.
    pub const ALL: CliSeal = CliSeal {
        tools: true,
        setting_sources: true,
        strict_mcp_config: true,
    };

    /// No flag passed — what a CLI too old for any of them gets.
    pub const NONE: CliSeal = CliSeal {
        tools: false,
        setting_sources: false,
        strict_mcp_config: false,
    };
}

/// Read [`CliSeal`] out of a `claude --help`.
///
/// **An unreadable answer is not evidence of absence.** Empty or unrecognisable
/// help text yields [`CliSeal::ALL`], so a probe that failed for a transient
/// reason on a *working* CLI degrades to a spawn that may error loudly — never
/// to a session quietly running unsealed. The dangerous direction here is
/// "assume the flag is missing and carry on": that reopens the hole with nothing
/// on screen to say so, which is exactly the failure this whole change exists to
/// close. Only a help text we actually read, that actually lacks the flag, drops
/// it.
pub fn seal_from_help(help: &str) -> CliSeal {
    if help.trim().is_empty() {
        return CliSeal::ALL;
    }
    CliSeal {
        tools: mentions_flag(help, "--tools"),
        setting_sources: mentions_flag(help, "--setting-sources"),
        strict_mcp_config: mentions_flag(help, "--strict-mcp-config"),
    }
}

/// Does `help` list exactly this flag — not one that merely starts with it?
///
/// `--tools` is a prefix of nothing today, but `--allowed-tools` and
/// `--disallowedTools` are near enough that a bare `contains` is the kind of
/// match that starts passing for the wrong reason after a rename. The character
/// after the flag has to end it.
fn mentions_flag(help: &str, flag: &str) -> bool {
    help.match_indices(flag).any(|(i, _)| {
        help[i + flag.len()..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '-')
    })
}

/// The sealing flags this CLI accepts, in the order both arg builders emit them.
///
/// One definition, so the session and the one-shot paths cannot come to seal
/// themselves differently — the one-shot paths are the ones with no surface to
/// notice it if they did.
fn seal_args(seal: CliSeal) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();
    if seal.strict_mcp_config {
        a.push("--strict-mcp-config".into());
    }
    if seal.setting_sources {
        a.push("--setting-sources".into());
        a.push("user".into());
    }
    if seal.tools {
        // Empty built-in set. The MCP tools the app allow-lists are unaffected —
        // this empties the *built-in* set only.
        a.push("--tools".into());
        a.push(String::new());
    }
    a
}

/// Build the args for a persistent streaming session.
///
/// `mcp_config_json` (if set) is passed to `--mcp-config` and its tools are
/// allow-listed so the assistant can call them without an interactive prompt.
///
/// `seal` is what the detected CLI accepts — see [`CliSeal`]. A flag it does not
/// know is left out rather than killing the spawn; `DISALLOWED_TOOLS` is passed
/// unconditionally and is what stands in for `--tools` on a CLI too old for it,
/// which is the case that backstop exists for.
///
/// **The assistant is given no built-in tools.** `--tools ""` empties the
/// built-in set, leaving the session exactly the MCP tools the app allow-lists
/// and nothing else. This is the guard; [`DISALLOWED_TOOLS`] is only a backstop
/// behind it. The denylist alone was never the guard it looked like — measured
/// against the shipped CLI it left nineteen built-ins live, including `Artifact`
/// (which publishes a page to the web), `CronCreate`, `SendMessage` and
/// `Workflow`. An assistant that has just read a client's rows should not be one
/// tool call away from publishing them — and they did not ask first: driving the
/// old flag set with a permission route deliberately left unanswered, `Skill`,
/// `ToolSearch` and `Monitor` each executed inside the turn with no
/// `can_use_tool` request emitted at all. What justifies this flag is what those
/// tools can do, *not* the stall recorded in `TODO.md`, whose cause these
/// measurements do not explain and which remains unidentified.
///
/// **The session sees only the MCP server and the settings we hand it.**
/// `--strict-mcp-config` and `--setting-sources user` are not tuning; they close
/// the hole that `DISALLOWED_TOOLS` and the allow-list look like they already
/// close. Without the first, the user's own global MCP servers load into
/// Schemaic's SQL assistant — tools nobody here allow-listed, reachable by the
/// same unprompted route the built-ins took, and on a machine whose global
/// settings allow-list them they do not even pause. Without the second, the CLI
/// also reads `.claude/settings.json`
/// and its local sibling relative to the working directory, and this child is
/// spawned in [`std::env::temp_dir`] — world-writable on most systems, so
/// anything that drops a settings file there is granting permissions in
/// Schemaic's AI panel. `user` is kept because the user's own global settings are
/// their own choice; the two directory-relative sources are the ones nobody
/// chose.
pub fn build_session_args(
    system_context: &str,
    model: Option<&str>,
    effort: Option<&str>,
    mcp_config_json: Option<&str>,
    mcp_tools: &[&str],
    seal: CliSeal,
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
    // All three ahead of the variadic --allowedTools/--disallowedTools, which
    // would read a later flag's name as a tool name.
    a.extend(seal_args(seal));
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
///
/// **Sealed the same way [`build_session_args`] is, and for a sharper reason.**
/// Every caller — Ctrl+K, AI Fill, AI Seed — wants one string back and reads it
/// with a parser; none of them has a surface that could render a tool call, and
/// each discards everything but the value it parses out. So a tool call on this
/// path leaves no trace anywhere: not in a transcript, not in a chip, not in the
/// string the caller keeps. They need no tool and no server — the whole request
/// is in the prompt.
pub fn inline_args(intent: &str, system: &str, model: &str, seal: CliSeal) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "-p".into(),
        intent.into(),
        "--append-system-prompt".into(),
        system.into(),
        "--model".into(),
        model.into(),
    ];
    a.extend(seal_args(seal));
    a
}

/// What the platform will accept as one spawned command line.
///
/// Windows' `CreateProcess` caps the **whole** command line at 32,767
/// characters; the headroom is for the executable path and the quoting the
/// runtime adds. Elsewhere the binding constraint is `MAX_ARG_STRLEN`, 128 KiB
/// for any single argument — and the system prompt is a single argument.
pub const fn arg_limit() -> usize {
    if cfg!(windows) { 30_000 } else { 120 * 1024 }
}

/// Why this command line can't be spawned, or `None` if it can.
///
/// Called before the spawn because the failure it prevents is unrecognisable
/// afterwards: the OS returns a generic error, and the app's handler says
/// *"Ensure Claude Code is installed"* — the one cause that is definitely not
/// the problem, sending the user to check an installation that is fine.
///
/// The message names the lever the user actually has. The system prompt carries
/// the schema outline, and the AI schema scope is a setting.
pub fn oversize_reason(args: &[String], limit: usize) -> Option<String> {
    // Roughly what the OS sees: the arguments plus a separator each.
    let total: usize = args.iter().map(|a| a.len() + 1).sum();
    let longest = args.iter().map(String::len).max().unwrap_or(0);
    if total <= limit && longest <= limit {
        return None;
    }
    Some(format!(
        "The context sent to Claude is too large for one command line \
         ({total} characters; this platform allows about {limit}). Narrow \
         Settings → AI → schema scope to the active database (or None), or \
         shorten the query in the editor."
    ))
}

/// Build a legible error message for a failed one-shot `claude` invocation.
///
/// The CLI writes some fatal errors — notably `Failed to authenticate: OAuth
/// session expired …` — to **stdout**, not stderr, and often with an empty
/// stderr. Surfacing stderr alone therefore yields a blank error, so prefer
/// stderr, fall back to stdout, and finally to the exit status. Pure so it's
/// unit-tested; both AI grid callbacks (fill / seed) use it.
pub fn cli_failure_message(code: Option<i32>, stdout: &str, stderr: &str) -> String {
    let err = stderr.trim();
    if !err.is_empty() {
        return err.to_string();
    }
    let out = stdout.trim();
    if !out.is_empty() {
        return out.to_string();
    }
    match code {
        Some(c) => format!("the claude CLI exited with status {c}"),
        None => "the claude CLI was terminated by a signal".to_string(),
    }
}

/// Encode a user turn as a `stream-json` stdin line (newline-terminated).
pub fn user_message_line(text: &str) -> String {
    let v = serde_json::json!({
        "type": "user",
        "message": { "role": "user", "content": text }
    });
    format!("{v}\n")
}

/// Encode an `interrupt` control request as a `stream-json` stdin line.
///
/// Ends the turn in flight without ending the process: the CLI answers with a
/// `control_response` and emits a `result`, then accepts the next message. That
/// is the difference between Stop and killing the child — verified against the
/// CLI, where every sampled interrupt ended the turn and left the session usable.
pub fn interrupt_line(request_id: &str) -> String {
    let v = serde_json::json!({
        "type": "control_request",
        "request_id": request_id,
        "request": { "subtype": "interrupt" }
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
        let a = build_session_args("", None, None, None, &[], CliSeal::ALL);
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
        let a = build_session_args(
            "ctx",
            Some("claude-opus-4-8"),
            Some("high"),
            None,
            &[],
            CliSeal::ALL,
        );
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
        let a = build_session_args(
            "",
            None,
            None,
            None,
            &["mcp__schemaic__run_query"],
            CliSeal::ALL,
        );
        assert!(pos(&a, "--allowedTools").is_none());
        // With a config, the tools follow --allowedTools before --disallowedTools.
        let a = build_session_args(
            "",
            None,
            None,
            Some("{\"mcpServers\":{}}"),
            &["mcp__schemaic__run_query", "mcp__schemaic__list_schema"],
            CliSeal::ALL,
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
    fn the_session_loads_no_mcp_server_and_no_config_dir_of_its_own_finding() {
        let a = build_session_args(
            "",
            None,
            None,
            Some("{\"mcpServers\":{}}"),
            &["mcp__schemaic__run_query"],
            CliSeal::ALL,
        );
        // Only the server we hand it: without this the user's own global MCP
        // servers load into the SQL assistant, and their tools are neither
        // allow-listed nor disallowed — the one class that prompts.
        assert!(pos(&a, "--strict-mcp-config").is_some());
        // Settings from the user, never from the CWD — which is the (often
        // world-writable) temp dir.
        let s = pos(&a, "--setting-sources").expect("setting-sources pinned");
        assert_eq!(a[s + 1], "user");
        // Both sit ahead of the two variadic tool lists, which would swallow
        // them as tool names.
        let al = pos(&a, "--allowedTools").unwrap();
        assert!(pos(&a, "--strict-mcp-config").unwrap() < al);
        assert!(s < al);
    }

    /// **Captured from `claude --help`, not written from memory** — the same rule
    /// the engine-error fixtures follow, and for the same reason: this parser is
    /// the thing that decides whether the sealing flags are passed at all, so a
    /// fixture shaped the way the help *ought* to look would prove nothing about
    /// the binary Schemaic actually spawns. The near-miss names are real: the CLI
    /// lists two spellings of each tool-list flag on one line, and mentions
    /// `--tools` inside another flag's prose.
    const HELP: &str = "\
  --allowedTools, --allowed-tools <tools...>
  --disallowedTools, --disallowed-tools <tools...>
                                        --tools names them, and ignores user,
  --setting-sources <sources>           Comma-separated list of setting sources
  --settings <file-or-json>             Path to a settings JSON file or a JSON
  --strict-mcp-config                   Only use MCP servers from --mcp-config,
  --tools <tools...>                    Specify the list of available tools from";

    #[test]
    fn a_current_cli_advertises_every_sealing_flag() {
        assert_eq!(seal_from_help(HELP), CliSeal::ALL);
    }

    /// The one that matters: `--allowed-tools` and `--disallowedTools` both end
    /// in the letters of `--tools`, and a `contains` would call a CLI that has
    /// neither `--tools` nor `--setting-sources` fully sealed — passing flags
    /// that kill the spawn.
    #[test]
    fn a_flag_is_not_found_inside_another_flags_name() {
        let old = "\
  --allowedTools, --allowed-tools <tools...>
  --disallowedTools, --disallowed-tools <tools...>
  --settings <file-or-json>             Path to a settings JSON file or a JSON
  --mcp-config <configs...>             Load MCP servers from a JSON file";
        assert_eq!(seal_from_help(old), CliSeal::NONE);
    }

    #[test]
    fn each_flag_is_read_on_its_own() {
        let only_strict = "  --strict-mcp-config    Only use MCP servers from --mcp-config";
        assert_eq!(
            seal_from_help(only_strict),
            CliSeal {
                tools: false,
                setting_sources: false,
                strict_mcp_config: true,
            }
        );
    }

    /// An unreadable probe must not be read as "the flags are absent" — that
    /// silently unseals a session on a CLI that was merely busy or noisy.
    #[test]
    fn an_unreadable_probe_seals_rather_than_opens() {
        for help in ["", "   \n\t "] {
            assert_eq!(seal_from_help(help), CliSeal::ALL, "{help:?}");
        }
    }

    /// The whole point of the probe: an old CLI gets a spawnable command line,
    /// and the backstop is what still stands between it and a shell.
    #[test]
    fn an_old_cli_gets_no_sealing_flags_but_keeps_the_backstop() {
        let a = build_session_args("", None, None, None, &[], CliSeal::NONE);
        for flag in ["--tools", "--setting-sources", "--strict-mcp-config"] {
            assert!(pos(&a, flag).is_none(), "{flag} must not be passed: {a:?}");
        }
        let d = pos(&a, "--disallowedTools").expect("the backstop still goes");
        assert!(a[d + 1..].contains(&"Artifact".to_string()));
        assert!(a[d + 1..].contains(&"Bash".to_string()));

        let inline = inline_args("i", "s", "m", CliSeal::NONE);
        assert!(!inline.iter().any(|x| x == "--tools"), "{inline:?}");
    }

    #[test]
    fn the_session_is_given_no_built_in_tools_at_all() {
        let a = build_session_args(
            "",
            None,
            None,
            Some("{\"mcpServers\":{}}"),
            &["mcp__schemaic__run_query"],
            CliSeal::ALL,
        );
        // `--tools ""` is the guard; the empty string is its whole value, so it
        // must be present *and* empty. A missing value would read the next flag
        // as a tool name and hand the session that tool.
        let t = pos(&a, "--tools").expect("--tools present");
        assert_eq!(a[t + 1], "", "--tools takes the empty list");
        // Ahead of the variadic lists, like the other prefix flags.
        assert!(t < pos(&a, "--allowedTools").unwrap());
        // The MCP allow-list is untouched: `--tools` empties the *built-in* set,
        // and the tools Schemaic serves are what the panel is for.
        let al = pos(&a, "--allowedTools").unwrap();
        let dis = pos(&a, "--disallowedTools").unwrap();
        assert!(a[al + 1..dis].contains(&"mcp__schemaic__run_query".to_string()));
    }

    #[test]
    fn cli_failure_prefers_stderr_then_stdout_then_status() {
        // stderr wins when present.
        assert_eq!(cli_failure_message(Some(1), "out", "boom"), "boom");
        // The real-world case: auth error on stdout, empty stderr → show stdout.
        assert_eq!(
            cli_failure_message(
                Some(1),
                "Failed to authenticate: OAuth session expired and could not be refreshed\n",
                "   "
            ),
            "Failed to authenticate: OAuth session expired and could not be refreshed"
        );
        // Both empty → fall back to the exit status (never a blank message).
        assert_eq!(
            cli_failure_message(Some(2), "", ""),
            "the claude CLI exited with status 2"
        );
        assert_eq!(
            cli_failure_message(None, "", ""),
            "the claude CLI was terminated by a signal"
        );
    }

    #[test]
    fn a_one_shot_generation_is_given_no_tools_and_no_servers() {
        // Ctrl+K, AI Fill and AI Seed all ask for one string back. They ran with
        // the full built-in set, the user's own MCP servers and every settings
        // file — and unlike the chat panel they have no surface that could show
        // a tool call, so anything the model reached for happened unseen.
        let a = inline_args("count rows", "SCHEMA", "claude-opus-4-8", CliSeal::ALL);
        let t = a.iter().position(|x| x == "--tools").expect("--tools");
        assert_eq!(a[t + 1], "");
        assert!(a.iter().any(|x| x == "--strict-mcp-config"));
        let s = a
            .iter()
            .position(|x| x == "--setting-sources")
            .expect("--setting-sources");
        assert_eq!(a[s + 1], "user");
    }

    #[test]
    fn inline_args_flags_in_order() {
        let a = inline_args("count rows", "SCHEMA", "claude-opus-4-8", CliSeal::ALL);
        assert_eq!(
            a,
            vec![
                "-p",
                "count rows",
                "--append-system-prompt",
                "SCHEMA",
                "--model",
                "claude-opus-4-8",
                // `seal_args`' order, shared with the session builder.
                "--strict-mcp-config",
                "--setting-sources",
                "user",
                "--tools",
                "",
            ]
        );
    }

    #[test]
    fn interrupt_line_is_a_control_request() {
        let line = interrupt_line("stop-7");
        assert!(line.ends_with('\n'));
        let v: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(v["type"], "control_request");
        assert_eq!(v["request_id"], "stop-7");
        assert_eq!(v["request"]["subtype"], "interrupt");
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

    #[test]
    fn an_ordinary_command_line_is_not_refused() {
        let args = build_session_args(
            "a system prompt",
            Some("haiku"),
            None,
            None,
            &[],
            CliSeal::ALL,
        );
        assert_eq!(oversize_reason(&args, arg_limit()), None);
    }

    #[test]
    fn an_oversize_prompt_is_refused_with_the_setting_that_fixes_it() {
        // Not "Ensure Claude Code is installed", which is what the OS error
        // becomes if this isn't caught, and which sends the user to check an
        // installation that is fine.
        let huge = "x".repeat(40_000);
        let args = build_session_args(&huge, None, None, None, &[], CliSeal::ALL);
        let why = oversize_reason(&args, 30_000).expect("must refuse");
        assert!(why.contains("schema scope"), "{why}");
        assert!(!why.contains("installed"), "{why}");
    }

    #[test]
    fn one_oversize_argument_is_refused_even_when_the_total_fits() {
        // The system prompt is a *single* argument, and that is what Linux caps
        // at `MAX_ARG_STRLEN` — a total-only check would pass and the spawn
        // would still fail.
        let args = vec!["-p".to_string(), "y".repeat(200)];
        assert!(oversize_reason(&args, 150).is_some());
    }
}
