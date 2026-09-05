//! One transcript vocabulary, three CLI dialects.
//!
//! Schemaic drives whichever agent CLI the user has installed, and each one
//! reports a turn in its own JSONL shape. The panel renders exactly one:
//! [`StreamEvent`] and the [`TurnState`](crate::TurnState) it feeds. So the
//! dialect stops here — every harness decodes into the same [`StreamEvent`]s,
//! and nothing downstream learns which CLI produced them.
//!
//! **They do not even agree on where the discriminator lives.** Claude and Codex
//! tag a line with `type`; Antigravity tags it with `event` and nests
//! the payload under a key of the same name. That is measured, not documented —
//! see the captured fixtures in the tests.
//!
//! **One dialect needs state, so decoding is a parser and not a function.**
//! Claude and Antigravity stream *deltas*: each line carries the text new since
//! the last, and a stateless `line -> events` mapping is exact. Codex
//! re-sends a message's text *cumulatively* as it grows, on every
//! `item.updated` for the same item id. Mapping that the stateless way appends
//! the whole message once per update, so a three-chunk reply renders as the
//! first word, then the first two, then all three, concatenated. [`Coalescer`]
//! is the fix and the first reason [`StreamParser`] owns a `&mut self`.
//!
//! **Tool calls are restated the same way, and that needs the same answer.**
//! Both per-turn dialects re-send a call while it runs — Codex on every
//! `item.updated` for the item, Antigravity on every `state: "ACTIVE"` for the
//! step — without marking the repeat. The panel pushes a chip per announcement
//! and attaches a result to the last pending one, so a restatement left an
//! earlier chip spinning forever. `StreamParser::seen_tools` is the second piece
//! of per-stream state, and it is keyed by whatever id that dialect gives the
//! call.
//!
//! **A side-effecting tool is surfaced, never dropped.** Codex can report
//! `command_execution` and `file_change` items. Under the sandbox this harness
//! is launched with they should not occur, and the temptation is to ignore them
//! as noise from a path we don't use. They are rendered as tool chips instead:
//! if the constraint we believe we set ever fails to hold, the user watches it
//! happen in the transcript rather than finding out from the filesystem.

use crate::StreamEvent;
use crate::harness::Harness;
use schemaic_core::transcript::TurnStats;
use std::collections::HashMap;

/// Emits only the part of a cumulative string not yet sent downstream.
///
/// Codex restates a message's full text as it grows. Feeding that
/// to a transcript that *appends* duplicates every prefix; this returns the new
/// suffix instead, and falls back to the whole string when the text is not an
/// extension of what came before (a rewritten message, or a new run reusing the
/// key), so a non-monotonic update loses nothing.
#[derive(Default)]
struct Coalescer {
    sent: HashMap<String, String>,
}

impl Coalescer {
    /// The unseen tail of `full` for `key`, or `None` when there is nothing new.
    fn advance(&mut self, key: &str, full: &str) -> Option<String> {
        let prev = self.sent.get(key);
        let out = match prev {
            Some(p) if full.starts_with(p.as_str()) => {
                if full.len() == p.len() {
                    return None;
                }
                full[p.len()..].to_string()
            }
            // Not an extension: the text was rewritten, so send it whole rather
            // than diffing two strings that share no prefix.
            _ => full.to_string(),
        };
        self.sent.insert(key.to_string(), full.to_string());
        if out.is_empty() { None } else { Some(out) }
    }

    /// Drop the accumulated text for `key` so a later run starts clean.
    fn clear(&mut self, key: &str) {
        self.sent.remove(key);
    }
}

/// Decodes one harness's JSONL into [`StreamEvent`]s.
///
/// Holds the per-message state described in the module docs. One parser per
/// turn for the one-process-per-turn harnesses, one per session for Claude;
/// either way it must not be shared between two concurrent streams, because the
/// coalescer is keyed by ids that are only unique within a stream.
pub struct StreamParser {
    harness: Harness,
    text: Coalescer,
    /// Tool calls already announced, by the id their dialect gives them.
    ///
    /// **A restatement is not a second call.** Both per-turn dialects re-send an
    /// in-progress tool call as it runs — Codex on every `item.updated` for the
    /// same item id, Antigravity on every `state: "ACTIVE"` for the same
    /// `step_index` — and neither marks the repeat. `TurnState::apply` pushes a
    /// chip for every `ToolUse` it is given and attaches a result to the *last*
    /// pending one, so a restated call left an earlier chip spinning for the rest
    /// of the transcript. This is the same problem [`Coalescer`] solves for prose
    /// and the same reason the parser is stateful.
    seen_tools: std::collections::HashSet<String>,
}

impl StreamParser {
    pub fn new(harness: Harness) -> Self {
        Self {
            harness,
            text: Coalescer::default(),
            seen_tools: std::collections::HashSet::new(),
        }
    }

    /// Whether this tool call is being announced for the first time.
    ///
    /// Ids are only unique within one stream, which is why the set lives on the
    /// parser and not anywhere longer-lived — the same reason the coalescer does.
    fn first_sight(&mut self, id: &str) -> bool {
        self.seen_tools.insert(id.to_string())
    }

    /// Which harness this parser decodes.
    pub fn harness(&self) -> Harness {
        self.harness
    }

    /// Decode one output line into zero or more events.
    pub fn push(&mut self, line: &str) -> Vec<StreamEvent> {
        let line = line.trim();
        if line.is_empty() {
            return Vec::new();
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            return Vec::new();
        };
        match self.harness {
            Harness::Claude => crate::parse_stream_line(line),
            Harness::Codex => self.push_codex(&v),
            Harness::Antigravity => self.push_antigravity(&v),
        }
    }

    /// `agy -p --output-format stream-json`.
    ///
    /// **Its discriminator is `event`, not `type`**, and each event's payload is
    /// nested under a key of the same name (`{"event":"init","init":{…}}`). Both
    /// differ from every other dialect here and from what its changelog implied;
    /// they are what the binary actually emitted.
    fn push_antigravity(&mut self, v: &serde_json::Value) -> Vec<StreamEvent> {
        let ev = v.get("event").and_then(|t| t.as_str()).unwrap_or("");
        match ev {
            // `conversation_id` sits at the top level, not inside `init`.
            "init" => v
                .get("conversation_id")
                .and_then(|s| s.as_str())
                .map(|id| vec![StreamEvent::SessionStarted { id: id.to_string() }])
                .unwrap_or_default(),
            "step_update" => {
                let s = v.get("step_update").unwrap_or(&serde_json::Value::Null);
                // True deltas: each carries only what is new, so they append and
                // need no coalescing. The terminal update of a step carries one
                // too (a trailing newline in the measured turn), so it is read
                // on every state rather than only while ACTIVE.
                match s.get("step_type").and_then(|t| t.as_str()).unwrap_or("") {
                    "agent_response" => s
                        .get("text_delta")
                        .and_then(|t| t.as_str())
                        .filter(|t| !t.is_empty())
                        .map(|t| vec![StreamEvent::TextDelta(t.to_string())])
                        .unwrap_or_default(),
                    // Our own prompt echoed back as a step.
                    "user_input" => Vec::new(),
                    "tool" => {
                        let evs = agy_tool_step(s);
                        // Every `ACTIVE` for one `step_index` is the same call
                        // being restated, so only the first announces a chip —
                        // see `StreamParser::seen_tools`. A step with no index
                        // is announced rather than swallowed: silently dropping
                        // a call is the worse failure of the two, and every
                        // measured step carried one.
                        match (evs.first(), s.get("step_index")) {
                            (Some(StreamEvent::ToolUse { .. }), Some(i))
                                if !self.first_sight(&i.to_string()) =>
                            {
                                Vec::new()
                            }
                            _ => evs,
                        }
                    }
                    _ => Vec::new(),
                }
            }
            "result" => {
                let r = v.get("result").unwrap_or(&serde_json::Value::Null);
                // **`status` alone is not the verdict.** A measured turn whose
                // only tool call was refused still reported
                // `"status":"SUCCESS"` with an empty `response`, recording the
                // refusal only in `denied_actions`. Reading `status` by itself
                // renders that as a successful, silent turn — the user asks a
                // question, nothing happens, and nothing says why.
                let denied = r
                    .get("denied_actions")
                    .and_then(|d| d.as_array())
                    .map(|a| !a.is_empty())
                    .unwrap_or(false);
                let failed = r.get("status").and_then(|s| s.as_str()) != Some("SUCCESS");
                let mut out = Vec::new();
                if denied {
                    out.push(StreamEvent::TextDelta(
                        "\nThis harness refused a tool this turn needed. Antigravity \
                         cannot ask for permission when it is not running \
                         interactively, so a tool it has no standing allow-rule for \
                         is denied.\n"
                            .to_string(),
                    ));
                }
                out.push(StreamEvent::TurnDone {
                    is_error: failed || denied,
                    stats: antigravity_stats(r),
                });
                out
            }
            _ => Vec::new(),
        }
    }

    /// `codex exec --json` — see `codex-rs/exec/src/exec_events.rs`.
    ///
    /// The item payload is `#[serde(flatten)]`ed into the item object, so an
    /// item's own `type` sits beside its `id` rather than under a wrapper.
    fn push_codex(&mut self, v: &serde_json::Value) -> Vec<StreamEvent> {
        let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match ty {
            "thread.started" => v
                .get("thread_id")
                .and_then(|t| t.as_str())
                .map(|id| vec![StreamEvent::SessionStarted { id: id.to_string() }])
                .unwrap_or_default(),
            "item.started" | "item.updated" | "item.completed" => {
                let Some(item) = v.get("item") else {
                    return Vec::new();
                };
                self.codex_item(item, ty == "item.completed")
            }
            "turn.completed" => vec![StreamEvent::TurnDone {
                is_error: false,
                stats: codex_stats(v),
            }],
            "turn.failed" => {
                let msg = v
                    .pointer("/error/message")
                    .and_then(|m| m.as_str())
                    .unwrap_or_default();
                let mut out = Vec::new();
                if !msg.is_empty() {
                    out.push(StreamEvent::TextDelta(format!("\n{msg}\n")));
                }
                out.push(StreamEvent::TurnDone {
                    is_error: true,
                    stats: TurnStats::default(),
                });
                out
            }
            // A fatal stream-level error. It ends the turn: nothing further
            // arrives, so a `TurnDone` has to come from here or the panel spins.
            "error" => {
                let msg = v
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or_default();
                let mut out = Vec::new();
                if !msg.is_empty() {
                    out.push(StreamEvent::TextDelta(format!("\n{msg}\n")));
                }
                out.push(StreamEvent::TurnDone {
                    is_error: true,
                    stats: TurnStats::default(),
                });
                out
            }
            _ => Vec::new(),
        }
    }

    /// One `ThreadItem`. `completed` marks the terminal event for the item.
    fn codex_item(&mut self, item: &serde_json::Value, completed: bool) -> Vec<StreamEvent> {
        let id = item.get("id").and_then(|i| i.as_str()).unwrap_or("");
        let kind = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match kind {
            "agent_message" => {
                let full = item.get("text").and_then(|t| t.as_str()).unwrap_or("");
                let out = self
                    .text
                    .advance(id, full)
                    .map(|s| vec![StreamEvent::TextDelta(s)])
                    .unwrap_or_default();
                if completed {
                    self.text.clear(id);
                }
                out
            }
            // Reasoning summaries are not transcript prose — the panel has no
            // place for them and Claude's dialect never surfaces them either.
            "reasoning" => Vec::new(),
            "mcp_tool_call" => {
                let server = item.get("server").and_then(|s| s.as_str()).unwrap_or("");
                let tool = item.get("tool").and_then(|t| t.as_str()).unwrap_or("");
                // Rebuild the fully-qualified name the transcript and the
                // allow-list both speak: `mcp__<server>__<tool>`.
                let name = format!("mcp__{server}__{tool}");
                let sql = item
                    .pointer("/arguments/sql")
                    .or_else(|| item.pointer("/arguments/query"))
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string());
                if !completed {
                    // Only the first sighting: `item.updated` restates a call in
                    // progress, and a chip per restatement is a chip that never
                    // fills. See `StreamParser::seen_tools`.
                    return match self.first_sight(id) {
                        true => vec![StreamEvent::ToolUse { name, sql }],
                        false => Vec::new(),
                    };
                }
                let err = item
                    .pointer("/error/message")
                    .and_then(|m| m.as_str())
                    .map(|s| s.to_string());
                let is_error =
                    err.is_some() || item.get("status").and_then(|s| s.as_str()) == Some("failed");
                let text = err.unwrap_or_else(|| mcp_result_text(item));
                vec![StreamEvent::ToolResult { text, is_error }]
            }
            // Side effects. They should not happen under the constraint this
            // harness is launched with; if one does, it is shown rather than
            // swallowed. See the module docs.
            "command_execution" => {
                let cmd = item.get("command").and_then(|c| c.as_str()).unwrap_or("");
                if !completed {
                    return vec![StreamEvent::ToolUse {
                        name: "shell".to_string(),
                        sql: None,
                    }];
                }
                let code = item.get("exit_code").and_then(|c| c.as_i64());
                let status = item.get("status").and_then(|s| s.as_str()).unwrap_or("");
                vec![StreamEvent::ToolResult {
                    text: format!(
                        "{cmd}\n{}",
                        item.get("aggregated_output")
                            .and_then(|o| o.as_str())
                            .unwrap_or("")
                    ),
                    is_error: status == "failed" || code.is_some_and(|c| c != 0),
                }]
            }
            "file_change" => {
                if !completed {
                    return vec![StreamEvent::ToolUse {
                        name: "file_change".to_string(),
                        sql: None,
                    }];
                }
                let paths: Vec<String> = item
                    .get("changes")
                    .and_then(|c| c.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|c| c.get("path").and_then(|p| p.as_str()))
                            .map(|p| p.to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                vec![StreamEvent::ToolResult {
                    text: paths.join("\n"),
                    is_error: item.get("status").and_then(|s| s.as_str()) == Some("failed"),
                }]
            }
            "error" => {
                let msg = item.get("message").and_then(|m| m.as_str()).unwrap_or("");
                vec![StreamEvent::TextDelta(format!("\n{msg}\n"))]
            }
            _ => Vec::new(),
        }
    }
}

/// Text out of a Codex `mcp_tool_call` result (`content` is MCP content blocks).
fn mcp_result_text(item: &serde_json::Value) -> String {
    item.pointer("/result/content")
        .and_then(|c| c.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|i| i.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// Codex reports tokens on `turn.completed` and no wall time at all.
///
/// `duration_ms` stays `None` rather than being invented: the panel's live
/// counter already shows elapsed time while the turn runs, and a fabricated
/// total would silently disagree with it.
fn codex_stats(v: &serde_json::Value) -> TurnStats {
    let at = |k: &str| v.pointer(&format!("/usage/{k}")).and_then(|n| n.as_u64());
    TurnStats {
        duration_ms: None,
        input_tokens: at("input_tokens"),
        output_tokens: at("output_tokens"),
    }
}

/// One Antigravity `step_type: "tool"` update.
///
/// **The tool that matters is nested.** An MCP call arrives as the *built-in*
/// tool `call_mcp_tool`, with the real identity in
/// `tool_info.parameters.{ServerName,ToolName}` — so a chip built from
/// `tool_name` alone would label every database call "call_mcp_tool" and the
/// transcript could not tell `run_query` from `propose_table_change`. The
/// qualified name is rebuilt to match what the allow-list and the transcript
/// already speak, exactly as the Codex dialect does from its own `server`/`tool`.
///
/// `ACTIVE` opens the chip; `DONE` and `ERROR` close it. A built-in tool is
/// reported under its own name rather than dropped, for the reason in the module
/// docs — a measured turn ran `list_dir` and `view_file` with no permission
/// prompt at all, so those are exactly what the user needs to be able to see.
fn agy_tool_step(s: &serde_json::Value) -> Vec<StreamEvent> {
    let info = s.get("tool_info").unwrap_or(&serde_json::Value::Null);
    let raw = s
        .get("tool_name")
        .and_then(|t| t.as_str())
        .unwrap_or_default();
    let params = info.get("parameters").unwrap_or(&serde_json::Value::Null);

    let name = if raw == "call_mcp_tool" {
        let server = params
            .get("ServerName")
            .and_then(|x| x.as_str())
            .unwrap_or_default();
        let tool = params
            .get("ToolName")
            .and_then(|x| x.as_str())
            .unwrap_or_default();
        format!("mcp__{server}__{tool}")
    } else {
        raw.to_string()
    };

    match s.get("state").and_then(|x| x.as_str()).unwrap_or("") {
        "ACTIVE" => {
            // MCP arguments sit under `Arguments`; a built-in's are flat.
            let sql = params
                .pointer("/Arguments/sql")
                .or_else(|| params.pointer("/Arguments/query"))
                .or_else(|| params.pointer("/sql"))
                .and_then(|x| x.as_str())
                .map(|x| x.to_string());
            vec![StreamEvent::ToolUse { name, sql }]
        }
        "ERROR" => vec![StreamEvent::ToolResult {
            text: info
                .pointer("/error/message")
                .and_then(|m| m.as_str())
                .unwrap_or("the harness refused this tool")
                .to_string(),
            is_error: true,
        }],
        "DONE" => vec![StreamEvent::ToolResult {
            text: info
                .get("output")
                .and_then(|o| o.as_str())
                .unwrap_or_default()
                .to_string(),
            is_error: false,
        }],
        _ => Vec::new(),
    }
}

/// Antigravity reports wall time as **`duration_seconds`, a float** — every
/// other dialect here reports whole milliseconds or nothing.
///
/// Rounded rather than truncated, so a 2.9985 s turn reads as `3.0s` and not
/// `2.9s`; and clamped at zero because `as u64` on a negative float saturates to
/// 0 in Rust but the intent should not rest on that.
fn antigravity_stats(r: &serde_json::Value) -> TurnStats {
    let at = |k: &str| r.pointer(&format!("/usage/{k}")).and_then(|n| n.as_u64());
    TurnStats {
        duration_ms: r
            .get("duration_seconds")
            .and_then(|d| d.as_f64())
            .filter(|d| d.is_finite() && *d >= 0.0)
            .map(|d| (d * 1000.0).round() as u64),
        input_tokens: at("input_tokens"),
        output_tokens: at("output_tokens"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemaic_core::transcript::Seg;

    fn drive(h: Harness, lines: &[&str]) -> Vec<StreamEvent> {
        let mut p = StreamParser::new(h);
        lines.iter().flat_map(|l| p.push(l)).collect()
    }

    fn text_of(evs: &[StreamEvent]) -> String {
        evs.iter()
            .filter_map(|e| match e {
                StreamEvent::TextDelta(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn harness_keys_round_trip_and_reject_the_unknown() {
        for h in Harness::ALL {
            assert_eq!(Harness::from_key(h.key()), Some(h), "{}", h.key());
        }
        // Not coerced to a default — the caller is told instead.
        assert_eq!(Harness::from_key("gpt"), None);
        assert_eq!(Harness::from_key(""), None);
    }

    #[test]
    fn every_harness_ignores_blank_and_malformed_lines() {
        for h in Harness::ALL {
            let out = drive(h, &["", "   ", "{not json", "[]", "{}"]);
            assert!(out.is_empty(), "{:?} produced {:?}", h, out.len());
        }
    }

    // ---- Codex ------------------------------------------------------------

    #[test]
    fn codex_cumulative_message_updates_emit_only_the_new_suffix() {
        // The bug this exists for: `item.updated` restates the whole message,
        // so appending each one renders "Hi" + "Hi there" + "Hi there!".
        let out = drive(
            Harness::Codex,
            &[
                r#"{"type":"item.started","item":{"id":"m1","type":"agent_message","text":"Hi"}}"#,
                r#"{"type":"item.updated","item":{"id":"m1","type":"agent_message","text":"Hi there"}}"#,
                r#"{"type":"item.completed","item":{"id":"m1","type":"agent_message","text":"Hi there!"}}"#,
            ],
        );
        assert_eq!(text_of(&out), "Hi there!");
    }

    #[test]
    fn codex_a_rewritten_message_is_sent_whole_rather_than_diffed() {
        let out = drive(
            Harness::Codex,
            &[
                r#"{"type":"item.started","item":{"id":"m1","type":"agent_message","text":"abc"}}"#,
                r#"{"type":"item.completed","item":{"id":"m1","type":"agent_message","text":"xyz"}}"#,
            ],
        );
        assert_eq!(text_of(&out), "abcxyz");
    }

    #[test]
    fn codex_two_messages_do_not_share_a_coalescer_slot() {
        let out = drive(
            Harness::Codex,
            &[
                r#"{"type":"item.completed","item":{"id":"m1","type":"agent_message","text":"one"}}"#,
                r#"{"type":"item.completed","item":{"id":"m2","type":"agent_message","text":"two"}}"#,
            ],
        );
        assert_eq!(text_of(&out), "onetwo");
    }

    #[test]
    fn codex_mcp_tool_call_rebuilds_the_qualified_name_and_captures_sql() {
        let out = drive(
            Harness::Codex,
            &[
                r#"{"type":"item.started","item":{"id":"t1","type":"mcp_tool_call","server":"schemaic","tool":"run_query","arguments":{"sql":"SELECT 1"},"status":"in_progress"}}"#,
            ],
        );
        match &out[..] {
            [StreamEvent::ToolUse { name, sql }] => {
                assert_eq!(name, "mcp__schemaic__run_query");
                assert_eq!(sql.as_deref(), Some("SELECT 1"));
            }
            other => panic!("{other:?}"),
        }
    }

    /// **One call, one chip, however many times the CLI restates it.**
    /// `codex_item` treats every non-`item.completed` event as "in progress", and
    /// `command_execution` is streamed with `item.updated` as its output grows —
    /// so a second `ToolUse` reached `TurnState::apply`, which pushes a segment
    /// unconditionally. The result attaches to the *last* pending chip, leaving
    /// the first spinning for the rest of the transcript.
    #[test]
    fn a_restated_tool_call_does_not_add_a_second_chip() {
        let out = drive(
            Harness::Codex,
            &[
                r#"{"type":"item.started","item":{"id":"t1","type":"mcp_tool_call","server":"schemaic","tool":"run_query","arguments":{"sql":"SELECT 1"},"status":"in_progress"}}"#,
                r#"{"type":"item.updated","item":{"id":"t1","type":"mcp_tool_call","server":"schemaic","tool":"run_query","arguments":{"sql":"SELECT 1"},"status":"in_progress"}}"#,
                r#"{"type":"item.updated","item":{"id":"t1","type":"mcp_tool_call","server":"schemaic","tool":"run_query","arguments":{"sql":"SELECT 1"},"status":"in_progress"}}"#,
                r#"{"type":"item.completed","item":{"id":"t1","type":"mcp_tool_call","server":"schemaic","tool":"run_query","status":"completed","result":{"content":[{"text":"1 row"}]}}}"#,
            ],
        );
        let uses = out
            .iter()
            .filter(|e| matches!(e, StreamEvent::ToolUse { .. }))
            .count();
        assert_eq!(uses, 1, "one call announced {uses} times: {out:?}");
        // And the call is still announced *before* its result, so the chip the
        // result attaches to exists.
        assert!(
            matches!(out.first(), Some(StreamEvent::ToolUse { .. })),
            "{out:?}"
        );
        assert!(
            matches!(out.last(), Some(StreamEvent::ToolResult { text, .. }) if text == "1 row"),
            "{out:?}"
        );
    }

    /// The same rule on the other per-turn dialect, whose id is `step_index`.
    #[test]
    fn a_restated_agy_tool_step_does_not_add_a_second_chip() {
        let out = drive(
            Harness::Antigravity,
            &[
                r#"{"event":"step_update","step_update":{"step_index":4,"state":"ACTIVE","step_type":"tool","tool_name":"call_mcp_tool","tool_info":{"name":"call_mcp_tool","parameters":{"Arguments":{},"ServerName":"schemaic","ToolName":"list_schema"}}}}"#,
                r#"{"event":"step_update","step_update":{"step_index":4,"state":"ACTIVE","step_type":"tool","tool_name":"call_mcp_tool","tool_info":{"name":"call_mcp_tool","parameters":{"Arguments":{},"ServerName":"schemaic","ToolName":"list_schema"}}}}"#,
                r###"{"event":"step_update","step_update":{"step_index":4,"state":"DONE","step_type":"tool","tool_name":"call_mcp_tool","tool_info":{"name":"call_mcp_tool","parameters":{"Arguments":{},"ServerName":"schemaic","ToolName":"list_schema"},"output":"ok"}}}"###,
            ],
        );
        let uses = out
            .iter()
            .filter(|e| matches!(e, StreamEvent::ToolUse { .. }))
            .count();
        assert_eq!(uses, 1, "{out:?}");
        // Two different steps are two different calls.
        let two = drive(
            Harness::Antigravity,
            &[
                r#"{"event":"step_update","step_update":{"step_index":4,"state":"ACTIVE","step_type":"tool","tool_name":"call_mcp_tool","tool_info":{"name":"call_mcp_tool","parameters":{"ServerName":"schemaic","ToolName":"list_schema"}}}}"#,
                r#"{"event":"step_update","step_update":{"step_index":6,"state":"ACTIVE","step_type":"tool","tool_name":"call_mcp_tool","tool_info":{"name":"call_mcp_tool","parameters":{"ServerName":"schemaic","ToolName":"run_query"}}}}"#,
            ],
        );
        assert_eq!(
            two.iter()
                .filter(|e| matches!(e, StreamEvent::ToolUse { .. }))
                .count(),
            2,
            "{two:?}"
        );
    }

    /// Two *different* calls still get two chips — the dedupe is per id, not a
    /// blanket "only the first tool call in a turn".
    #[test]
    fn two_distinct_tool_calls_still_get_a_chip_each() {
        let out = drive(
            Harness::Codex,
            &[
                r#"{"type":"item.started","item":{"id":"t1","type":"mcp_tool_call","server":"schemaic","tool":"list_schema","arguments":{}}}"#,
                r#"{"type":"item.completed","item":{"id":"t1","type":"mcp_tool_call","server":"schemaic","tool":"list_schema","result":{"content":[{"text":"ok"}]}}}"#,
                r#"{"type":"item.started","item":{"id":"t2","type":"mcp_tool_call","server":"schemaic","tool":"run_query","arguments":{"sql":"SELECT 1"}}}"#,
                r#"{"type":"item.completed","item":{"id":"t2","type":"mcp_tool_call","server":"schemaic","tool":"run_query","result":{"content":[{"text":"1 row"}]}}}"#,
            ],
        );
        let uses = out
            .iter()
            .filter(|e| matches!(e, StreamEvent::ToolUse { .. }))
            .count();
        assert_eq!(uses, 2, "{out:?}");
    }

    #[test]
    fn codex_mcp_tool_result_reads_content_blocks_and_flags_failure() {
        let ok = drive(
            Harness::Codex,
            &[
                r#"{"type":"item.completed","item":{"id":"t1","type":"mcp_tool_call","server":"schemaic","tool":"run_query","arguments":{},"result":{"content":[{"type":"text","text":"1 row"}]},"status":"completed"}}"#,
            ],
        );
        match &ok[..] {
            [StreamEvent::ToolResult { text, is_error }] => {
                assert_eq!(text, "1 row");
                assert!(!is_error);
            }
            other => panic!("{other:?}"),
        }

        let bad = drive(
            Harness::Codex,
            &[
                r#"{"type":"item.completed","item":{"id":"t1","type":"mcp_tool_call","server":"schemaic","tool":"run_query","arguments":{},"error":{"message":"nope"},"status":"failed"}}"#,
            ],
        );
        match &bad[..] {
            [StreamEvent::ToolResult { text, is_error }] => {
                assert_eq!(text, "nope");
                assert!(is_error);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn codex_a_side_effecting_item_is_shown_not_swallowed() {
        // The constraint should prevent these. If it ever doesn't, the user
        // sees it in the transcript instead of on their disk.
        let shell = drive(
            Harness::Codex,
            &[
                r#"{"type":"item.started","item":{"id":"c1","type":"command_execution","command":"rm -rf /","aggregated_output":"","status":"in_progress"}}"#,
            ],
        );
        assert!(
            matches!(&shell[..], [StreamEvent::ToolUse { .. }]),
            "{shell:?}"
        );

        let edit = drive(
            Harness::Codex,
            &[
                r#"{"type":"item.completed","item":{"id":"f1","type":"file_change","changes":[{"path":"/etc/passwd","kind":"update"}],"status":"completed"}}"#,
            ],
        );
        match &edit[..] {
            [StreamEvent::ToolResult { text, .. }] => assert!(text.contains("/etc/passwd")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn codex_reasoning_never_reaches_the_transcript() {
        let out = drive(
            Harness::Codex,
            &[
                r#"{"type":"item.completed","item":{"id":"r1","type":"reasoning","text":"thinking hard"}}"#,
            ],
        );
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn codex_turn_completed_carries_usage_and_no_invented_duration() {
        let out = drive(
            Harness::Codex,
            &[
                r#"{"type":"turn.completed","usage":{"input_tokens":10,"cached_input_tokens":2,"output_tokens":5,"reasoning_output_tokens":1}}"#,
            ],
        );
        match &out[..] {
            [StreamEvent::TurnDone { is_error, stats }] => {
                assert!(!is_error);
                assert_eq!(stats.input_tokens, Some(10));
                assert_eq!(stats.output_tokens, Some(5));
                assert_eq!(stats.duration_ms, None);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn codex_a_failed_turn_and_a_stream_error_both_end_the_turn() {
        for line in [
            r#"{"type":"turn.failed","error":{"message":"boom"}}"#,
            r#"{"type":"error","message":"boom"}"#,
        ] {
            let out = drive(Harness::Codex, &[line]);
            assert!(text_of(&out).contains("boom"), "{out:?}");
            assert!(
                matches!(
                    out.last(),
                    Some(StreamEvent::TurnDone { is_error: true, .. })
                ),
                "{line} -> {out:?}"
            );
        }
    }

    #[test]
    fn codex_thread_started_surfaces_the_id_a_resume_needs() {
        let out = drive(
            Harness::Codex,
            &[r#"{"type":"thread.started","thread_id":"th_42"}"#],
        );
        match &out[..] {
            [StreamEvent::SessionStarted { id }] => assert_eq!(id, "th_42"),
            other => panic!("{other:?}"),
        }
    }

    /// **Captured verbatim** from `codex exec --json` on the installed binary,
    /// for the prompt "Reply with exactly the word ok".
    ///
    /// Everything else about this dialect was inferred from Codex's own
    /// `exec_events.rs` serde attributes; this is the one case where the bytes
    /// are the bytes. Keep it byte-for-byte — the moment somebody "tidies" the
    /// ids or the usage keys it stops being evidence and becomes another
    /// hand-written fixture agreeing with the code that produced it.
    const CODEX_REAL_TURN: &[&str] = &[
        r#"{"type":"thread.started","thread_id":"01a07261-62e9-7073-a8bb-405b9c4a16e5"}"#,
        r#"{"type":"turn.started"}"#,
        r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"ok"}}"#,
        r#"{"type":"turn.completed","usage":{"input_tokens":13146,"cached_input_tokens":9984,"cache_write_input_tokens":0,"output_tokens":5,"reasoning_output_tokens":0}}"#,
    ];

    #[test]
    fn a_real_codex_turn_decodes_end_to_end() {
        let out = drive(Harness::Codex, CODEX_REAL_TURN);

        // The id a resumed turn needs, the prose, and the close — in order, with
        // `turn.started` contributing nothing.
        match &out[..] {
            [
                StreamEvent::SessionStarted { id },
                StreamEvent::TextDelta(t),
                StreamEvent::TurnDone { is_error, stats },
            ] => {
                assert_eq!(id, "01a07261-62e9-7073-a8bb-405b9c4a16e5");
                assert_eq!(t, "ok");
                assert!(!is_error);
                assert_eq!(stats.input_tokens, Some(13146));
                assert_eq!(stats.output_tokens, Some(5));
                assert_eq!(stats.duration_ms, None);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_real_codex_turn_renders_as_one_text_segment() {
        // Through `TurnState` too, so the composition with the accumulator is
        // covered and not just the parser in isolation.
        let mut p = StreamParser::new(Harness::Codex);
        let mut st = crate::TurnState::default();
        for line in CODEX_REAL_TURN {
            for ev in p.push(line) {
                st.apply(&ev);
            }
        }
        assert_eq!(st.segments(), vec![Seg::Text("ok".to_string())]);
    }

    /// **Captured verbatim** from `codex exec --json` driving the real Schemaic
    /// MCP server against a SQLite database, with the `-c` override and the
    /// per-tool `approval_mode="approve"` in place. The `agent_message` before
    /// the call is elided; the rest is byte-for-byte.
    const CODEX_REAL_TOOL_CYCLE: &[&str] = &[
        r#"{"type":"item.started","item":{"id":"item_1","type":"mcp_tool_call","server":"schemaic","tool":"list_schema","arguments":{},"result":null,"error":null,"status":"in_progress"}}"#,
        // `r###` — the server answers in markdown and its heading contains `"##`.
        r###"{"type":"item.completed","item":{"id":"item_1","type":"mcp_tool_call","server":"schemaic","tool":"list_schema","arguments":{},"result":{"content":[{"type":"text","text":"## main (2 tables)\n- orders\n- widgets\n"}],"structured_content":null},"error":null,"status":"completed"}}"###,
        r#"{"type":"item.completed","item":{"id":"item_2","type":"agent_message","text":"orders  \nwidgets"}}"#,
        r#"{"type":"turn.completed","usage":{"input_tokens":41065,"cached_input_tokens":33024,"cache_write_input_tokens":0,"output_tokens":128,"reasoning_output_tokens":14}}"#,
    ];

    #[test]
    fn a_real_codex_tool_cycle_fills_its_chip_and_answers() {
        let out = drive(Harness::Codex, CODEX_REAL_TOOL_CYCLE);
        match out.first() {
            Some(StreamEvent::ToolUse { name, .. }) => {
                assert_eq!(name, "mcp__schemaic__list_schema")
            }
            other => panic!("{other:?}"),
        }
        assert!(
            out.iter().any(|e| matches!(
                e,
                StreamEvent::ToolResult { text, is_error: false } if text.contains("widgets")
            )),
            "the server's answer never reached the chip: {out:?}"
        );
        assert_eq!(text_of(&out), "orders  \nwidgets");
    }

    /// The refusal shape, captured before the approval was configured. Codex
    /// reports it on the item's `error`, and `status` is `failed`.
    #[test]
    fn a_real_codex_tool_refusal_is_flagged_on_the_chip() {
        let out = drive(
            Harness::Codex,
            &[
                r#"{"type":"item.completed","item":{"id":"item_1","type":"mcp_tool_call","server":"schemaic","tool":"list_schema","arguments":{},"result":null,"error":{"message":"MCP tool call requires approval, but approval policy is never"},"status":"failed"}}"#,
            ],
        );
        match &out[..] {
            [StreamEvent::ToolResult { text, is_error }] => {
                assert!(*is_error);
                assert!(text.contains("requires approval"), "{text}");
            }
            other => panic!("{other:?}"),
        }
    }

    // ---- Antigravity ------------------------------------------------------

    /// **Captured verbatim** from `agy -p --output-format stream-json --sandbox`
    /// on the installed binary, for the prompt "Reply with exactly the word ok".
    /// The `init.tools` array (56 entries) is elided; the rest is byte-for-byte.
    const AGY_REAL_TURN: &[&str] = &[
        r#"{"event":"init","conversation_id":"d0994762-e805-4c81-9298-3787bc9df864","init":{"cwd":"C:\\tmp","tools":["run_command","write_to_file"],"permission_mode":"request-review"}}"#,
        r#"{"event":"step_update","step_update":{"conversation_id":"d0994762-e805-4c81-9298-3787bc9df864","step_index":0,"state":"DONE","step_type":"user_input"}}"#,
        r#"{"event":"step_update","step_update":{"conversation_id":"d0994762-e805-4c81-9298-3787bc9df864","step_index":1,"state":"ACTIVE","step_type":"agent_response","text_delta":"ok"}}"#,
        r#"{"event":"step_update","step_update":{"conversation_id":"d0994762-e805-4c81-9298-3787bc9df864","step_index":1,"state":"DONE","step_type":"agent_response","text_delta":"\n","duration_seconds":2.0066245,"usage":{"input_tokens":14006,"output_tokens":84,"thinking_tokens":83,"cache_read_tokens":0,"total_tokens":14090}}}"#,
        r#"{"event":"result","result":{"conversation_id":"d0994762-e805-4c81-9298-3787bc9df864","status":"SUCCESS","response":"ok\n","duration_seconds":2.0374978,"num_turns":1,"usage":{"input_tokens":14006,"output_tokens":84,"thinking_tokens":83,"cache_read_tokens":0,"total_tokens":14090}}}"#,
    ];

    #[test]
    fn a_real_antigravity_turn_decodes_end_to_end() {
        let out = drive(Harness::Antigravity, AGY_REAL_TURN);
        // The id, both text deltas (the DONE step carries the trailing newline),
        // and the close. The echoed `user_input` step contributes nothing.
        assert_eq!(text_of(&out), "ok\n");
        match out.first() {
            Some(StreamEvent::SessionStarted { id }) => {
                assert_eq!(id, "d0994762-e805-4c81-9298-3787bc9df864");
            }
            other => panic!("{other:?}"),
        }
        match out.last() {
            Some(StreamEvent::TurnDone { is_error, stats }) => {
                assert!(!is_error);
                assert_eq!(stats.input_tokens, Some(14006));
                assert_eq!(stats.output_tokens, Some(84));
                // 2.0374978s → 2037ms, rounded from a float.
                assert_eq!(stats.duration_ms, Some(2037));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_real_antigravity_turn_renders_as_one_text_segment() {
        let mut p = StreamParser::new(Harness::Antigravity);
        let mut st = crate::TurnState::default();
        for line in AGY_REAL_TURN {
            for ev in p.push(line) {
                st.apply(&ev);
            }
        }
        // `TurnState::segments` trims the prose, so the trailing newline goes.
        assert_eq!(st.segments(), vec![Seg::Text("ok".to_string())]);
    }

    /// **Captured verbatim** from the turn that first exercised an MCP tool call
    /// on the installed `agy`, with the `schemaic` server registered. The prompt
    /// asked it to list the tables; the call was refused because headless mode
    /// cannot prompt for permission.
    const AGY_REAL_DENIED_TOOL: &[&str] = &[
        r#"{"event":"step_update","step_update":{"conversation_id":"3861f769","step_index":6,"state":"ACTIVE","step_type":"tool","tool_name":"call_mcp_tool","tool_info":{"name":"call_mcp_tool","parameters":{"Arguments":{},"ServerName":"schemaic","ToolName":"list_schema"}}}}"#,
        r#"{"event":"step_update","step_update":{"conversation_id":"3861f769","step_index":6,"state":"ERROR","step_type":"tool","tool_name":"call_mcp_tool","duration_seconds":0.0185249,"tool_info":{"name":"call_mcp_tool","parameters":{"Arguments":{},"ServerName":"schemaic","ToolName":"list_schema"},"error":{"type":"TOOL_ERROR","message":"permission check failed for mcp \"schemaic/list_schema\": user denied permission for mcp(schemaic/list_schema)"}}}}"#,
        r#"{"event":"result","result":{"conversation_id":"3861f769","status":"SUCCESS","response":"","duration_seconds":4.5478546,"num_turns":1,"usage":{"input_tokens":33792,"output_tokens":554,"total_tokens":34346},"denied_actions":[{"action":"mcp","display_name":"CallMcpTool"}]}}"#,
    ];

    #[test]
    fn a_real_denied_antigravity_tool_call_names_the_tool_and_fails_the_turn() {
        let out = drive(Harness::Antigravity, AGY_REAL_DENIED_TOOL);

        // The chip must name the database tool, not the built-in wrapper.
        match out.first() {
            Some(StreamEvent::ToolUse { name, .. }) => {
                assert_eq!(name, "mcp__schemaic__list_schema");
            }
            other => panic!("{other:?}"),
        }
        // The refusal reaches the chip rather than vanishing.
        assert!(
            out.iter().any(|e| matches!(
                e,
                StreamEvent::ToolResult { text, is_error: true } if text.contains("denied permission")
            )),
            "{out:?}"
        );
        // **The turn is an error even though `status` says SUCCESS.**
        match out.last() {
            Some(StreamEvent::TurnDone { is_error, .. }) => assert!(
                *is_error,
                "a turn whose only tool call was refused is not a success"
            ),
            other => panic!("{other:?}"),
        }
        // And the user is told why, since the response body was empty.
        assert!(text_of(&out).contains("permission"), "{out:?}");
    }

    /// **Captured verbatim** from the first turn that completed a database tool
    /// call end to end: `agy` → our MCP server → SQLite → answer, with the four
    /// `mcp(schemaic/…)` allow-rules in place and `run_command` never granted.
    ///
    /// The counterpart to [`AGY_REAL_DENIED_TOOL`]: same harness, same prompt,
    /// the permission being the only difference. Keep both — one pins the happy
    /// path, the other pins a refusal that reports `"status":"SUCCESS"`.
    const AGY_REAL_TOOL_CYCLE: &[&str] = &[
        r#"{"event":"step_update","step_update":{"step_index":4,"state":"ACTIVE","step_type":"tool","tool_name":"call_mcp_tool","tool_info":{"name":"call_mcp_tool","parameters":{"Arguments":{},"ServerName":"schemaic","ToolName":"list_schema"}}}}"#,
        // `r###` because the captured output contains the exact sequence `"##`
        // — the server answers in markdown and its `## main` heading, preceded
        // by the JSON quote, closes both an `r#"` and an `r##"` literal early.
        r###"{"event":"step_update","step_update":{"step_index":4,"state":"DONE","step_type":"tool","tool_name":"call_mcp_tool","duration_seconds":0.1246735,"tool_info":{"name":"call_mcp_tool","parameters":{"Arguments":{},"ServerName":"schemaic","ToolName":"list_schema"},"output":"## main (2 tables)\n- orders\n- widgets\n"}}}"###,
        r#"{"event":"step_update","step_update":{"step_index":5,"state":"DONE","step_type":"agent_response","text_delta":"orders\nwidgets\n","duration_seconds":1.7201625,"usage":{"input_tokens":15786,"output_tokens":104}}}"#,
        r#"{"event":"result","result":{"status":"SUCCESS","response":"orders\nwidgets\n","duration_seconds":4.6566974,"num_turns":1,"usage":{"input_tokens":38040,"output_tokens":597,"total_tokens":38637}}}"#,
    ];

    #[test]
    fn a_real_completed_antigravity_tool_call_fills_its_chip_and_succeeds() {
        let out = drive(Harness::Antigravity, AGY_REAL_TOOL_CYCLE);

        match out.first() {
            Some(StreamEvent::ToolUse { name, .. }) => {
                assert_eq!(name, "mcp__schemaic__list_schema")
            }
            other => panic!("{other:?}"),
        }
        assert!(
            out.iter().any(|e| matches!(
                e,
                StreamEvent::ToolResult { text, is_error: false } if text.contains("widgets")
            )),
            "the server's answer never reached the chip: {out:?}"
        );
        assert_eq!(text_of(&out), "orders\nwidgets\n");
        match out.last() {
            Some(StreamEvent::TurnDone { is_error, stats }) => {
                assert!(!is_error, "no denial this time");
                assert_eq!(stats.duration_ms, Some(4657));
                assert_eq!(stats.output_tokens, Some(597));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_real_completed_tool_call_renders_as_a_filled_chip_then_prose() {
        // Through `TurnState`, so the chip/result pairing is covered and not
        // just the events in isolation.
        let mut p = StreamParser::new(Harness::Antigravity);
        let mut st = crate::TurnState::default();
        for line in AGY_REAL_TOOL_CYCLE {
            for ev in p.push(line) {
                st.apply(&ev);
            }
        }
        let segs = st.segments();
        match &segs[..] {
            [Seg::Tool(tc), Seg::Text(t)] => {
                assert_eq!(tc.name, "mcp__schemaic__list_schema");
                assert!(!tc.is_error);
                assert!(
                    tc.result.as_deref().unwrap_or_default().contains("orders"),
                    "{tc:?}"
                );
                assert_eq!(t, "orders\nwidgets");
            }
            other => panic!("{other:?}"),
        }
    }

    /// **Captured verbatim.** Antigravity split one two-word answer across two
    /// `text_delta`s mid-word — `"orders\nwidge"` then `"ts\n"` — which is the
    /// evidence that these are partial chunks and not cumulative restatements.
    /// Run through the coalescer they would render as "orders\nwidgets\n" only by
    /// accident; appended, they are exact.
    #[test]
    fn real_antigravity_deltas_are_partial_chunks_and_append() {
        let out = drive(
            Harness::Antigravity,
            &[
                r#"{"event":"step_update","step_update":{"step_index":5,"state":"ACTIVE","step_type":"agent_response","text_delta":"orders\nwidge"}}"#,
                r#"{"event":"step_update","step_update":{"step_index":5,"state":"DONE","step_type":"agent_response","text_delta":"ts\n","duration_seconds":1.385}}"#,
            ],
        );
        assert_eq!(text_of(&out), "orders\nwidgets\n");
    }

    #[test]
    fn an_antigravity_builtin_tool_is_reported_under_its_own_name() {
        // Measured: these ran with no permission prompt, so they are precisely
        // what the transcript must not hide.
        let out = drive(
            Harness::Antigravity,
            &[
                r#"{"event":"step_update","step_update":{"step_index":2,"state":"ACTIVE","step_type":"tool","tool_name":"list_dir","tool_info":{"name":"list_dir","parameters":{"DirectoryPath":"C:\\x"}}}}"#,
            ],
        );
        match &out[..] {
            [StreamEvent::ToolUse { name, sql }] => {
                assert_eq!(name, "list_dir");
                assert!(sql.is_none());
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_antigravity_mcp_query_captures_its_sql() {
        let out = drive(
            Harness::Antigravity,
            &[
                r#"{"event":"step_update","step_update":{"step_index":3,"state":"ACTIVE","step_type":"tool","tool_name":"call_mcp_tool","tool_info":{"name":"call_mcp_tool","parameters":{"Arguments":{"sql":"SELECT 1"},"ServerName":"schemaic","ToolName":"run_query"}}}}"#,
            ],
        );
        match &out[..] {
            [StreamEvent::ToolUse { name, sql }] => {
                assert_eq!(name, "mcp__schemaic__run_query");
                assert_eq!(sql.as_deref(), Some("SELECT 1"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_antigravity_failure_is_any_status_but_success() {
        let out = drive(
            Harness::Antigravity,
            &[r#"{"event":"result","result":{"status":"ERROR","duration_seconds":0.5}}"#],
        );
        assert!(
            matches!(&out[..], [StreamEvent::TurnDone { is_error: true, .. }]),
            "{out:?}"
        );
    }

    #[test]
    fn an_antigravity_event_is_keyed_on_event_not_type() {
        // The trap: every other dialect here uses `type`. A line shaped like one
        // of those must decode to nothing rather than being half-read.
        let out = drive(
            Harness::Antigravity,
            &[r#"{"type":"result","result":{"status":"SUCCESS"}}"#],
        );
        assert!(out.is_empty(), "{out:?}");
    }

    // ---- Claude (delegation) ----------------------------------------------

    #[test]
    fn claude_still_decodes_through_the_parser() {
        let out = drive(
            Harness::Claude,
            &[
                r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"hello"}}}"#,
            ],
        );
        assert_eq!(text_of(&out), "hello");
    }
}
