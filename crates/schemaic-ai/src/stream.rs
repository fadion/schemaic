//! One transcript vocabulary, five CLI dialects.
//!
//! Schemaic drives whichever agent CLI the user has installed, and each one
//! reports a turn in its own JSONL shape. The panel renders exactly one:
//! [`StreamEvent`] and the [`TurnState`](crate::TurnState) it feeds. So the
//! dialect stops here — every harness decodes into the same [`StreamEvent`]s,
//! and nothing downstream learns which CLI produced them.
//!
//! **They do not even agree on where the discriminator lives.** Claude, Codex,
//! OpenCode and Copilot tag a line with `type` (Copilot nesting the rest under
//! `data`); Antigravity tags it with `event` and nests the payload under a key
//! of the same name. That is measured, not documented — see the captured
//! fixtures in the tests.
//!
//! **One of them never says a turn is over.** Claude, Codex, Antigravity and
//! Copilot each emit a terminal event — Copilot's being the one line with no
//! `data`, after several `assistant.turn_end`s that are not it. OpenCode's
//! printer simply stops writing when the
//! session goes idle, so the close is inferred from `step_finish.reason` —
//! `tool-calls` means another step follows, anything else ends the turn. That
//! inference is load-bearing in both directions: end early and the answer is
//! truncated, never end and the app closes the turn on process exit and reports
//! a working turn as "ended unexpectedly".
//!
//! **And one of them streams nothing.** Claude and Antigravity send deltas,
//! Codex sends cumulative restatements, Copilot sends deltas *and then* the
//! whole message again, and OpenCode sends whole finished parts
//! — its printer emits a text part only once `time.end` is set. There is no
//! partial text to decode, which is why [`Harness::streams_deltas`] is false for
//! it and no amount of coalescing would change that.
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
//! Codex and Antigravity both re-send a call while it runs — Codex on every
//! `item.updated` for the item, Antigravity on every `state: "ACTIVE"` for the
//! step — without marking the repeat. The panel pushes a chip per announcement
//! and attaches a result to the last pending one, so a restatement left an
//! earlier chip spinning forever. `StreamParser::seen_tools` is the second piece
//! of per-turn state, and it is keyed by whatever id that dialect gives the
//! call — which is why [`StreamParser::push`] clears it at a turn boundary. It
//! was *per stream* until a stream could hold more than one Antigravity turn,
//! and those step ids start again from zero on each.
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
///
/// **Measured on codex-cli 0.153.4: it does not restate.** Three real
/// `codex exec --json` turns — 394, 439 and 209 output tokens — produced **no
/// `item.updated` line at all**, and no `item.started` either; every
/// `agent_message` arrived as a single `item.completed` carrying the whole
/// answer. So on that build `advance` is called once per message, with no prior
/// text to compare against, and the Θ(N·K) this once looked like is Θ(N) with
/// K = 1.
///
/// **Kept anyway, and that is a decision rather than an oversight.** The
/// alternative to coalescing is a transcript that renders every prefix again the
/// day a build starts streaming — a data-loss-shaped bug arriving from a CLI
/// upgrade, with nothing here to catch it, and the streaming shape is one Codex
/// documents. What the measurement changes is the *cost* of keeping it (one
/// hash lookup per message) and the standing of the tests that drive it: they
/// exercise a shape this build never emits, which
/// `the_coalescer_emits_only_what_is_new` now says out loud rather than
/// implying it is a capture.
#[derive(Default)]
struct Coalescer {
    sent: HashMap<String, String>,
}

impl Coalescer {
    /// The unseen tail of `full` for `key`, or `None` when there is nothing new.
    ///
    /// **One allocation per update, not three.** It built the tail, then
    /// allocated a fresh `String` for `full` *and* a fresh `String` for `key` on
    /// every call, replacing an entry that was almost always already there. The
    /// accumulated text is extended in place instead, and the key is only
    /// allocated when it is genuinely new.
    ///
    /// The remaining Θ(N·K) is the `starts_with` compare, which is the price of
    /// detecting a rewrite. **K is 1 on codex-cli 0.153.4** — that build never
    /// restates, so this is called once per message against no prior text (the
    /// measurement is on [`Coalescer`]). Tuning it further would be optimising a
    /// path that does not execute.
    fn advance(&mut self, key: &str, full: &str) -> Option<String> {
        match self.sent.get_mut(key) {
            Some(p) if full.starts_with(p.as_str()) => {
                if full.len() == p.len() {
                    return None;
                }
                let out = full[p.len()..].to_string();
                p.push_str(&out);
                Some(out)
            }
            // Not an extension: the text was rewritten, so send it whole rather
            // than diffing two strings that share no prefix.
            Some(p) => {
                p.clear();
                p.push_str(full);
                (!full.is_empty()).then(|| full.to_string())
            }
            None => {
                self.sent.insert(key.to_string(), full.to_string());
                (!full.is_empty()).then(|| full.to_string())
            }
        }
    }

    /// Record a true delta for `key` as sent, and hand it back.
    ///
    /// **For the dialect that sends both shapes.** Copilot streams
    /// `assistant.message_delta`s and then restates the whole message in one
    /// `assistant.message`; the restatement is fed to [`Coalescer::advance`],
    /// which can only answer "nothing new" if the deltas were recorded here
    /// first.
    fn extend(&mut self, key: &str, delta: &str) -> Option<String> {
        if delta.is_empty() {
            return None;
        }
        self.sent
            .entry(key.to_string())
            .or_default()
            .push_str(delta);
        Some(delta.to_string())
    }

    /// Reconcile a **restatement** of `key` with the deltas [`Coalescer::extend`]
    /// already sent — the tail it adds, or `None`.
    ///
    /// **Not [`Coalescer::advance`]**, whose non-extension arm is Codex's: a
    /// cumulative restatement that is not a prefix really is new text. For the
    /// dialects that stream deltas *and then* restate the message (Copilot,
    /// Cursor), the restatement is a copy of text already on screen, so one that
    /// is shorter, trimmed or normalised adds nothing — sending it whole printed
    /// the answer twice. With no deltas first it is the only copy, and is sent.
    fn settle(&mut self, key: &str, full: &str) -> Option<String> {
        match self.sent.get(key) {
            Some(p) if !full.starts_with(p.as_str()) => None,
            _ => self.advance(key, full),
        }
    }

    /// Drop the accumulated text for `key` so a later run starts clean.
    fn clear(&mut self, key: &str) {
        self.sent.remove(key);
    }
}

/// Decodes one harness's JSONL into [`StreamEvent`]s.
///
/// Holds the per-message state described in the module docs. One parser per
/// turn for the harnesses spawned per turn, one per session for the two that
/// hold a process — Claude and Antigravity. Either way it must not be shared
/// between two concurrent streams: the coalescer and the tool set are keyed by
/// ids unique only within a *turn*, which is why [`StreamParser::push`] clears
/// them at every turn boundary rather than only at construction.
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
    /// The one dialect that reports usage per *step* and never says a turn is
    /// over. See [`StreamParser::push_opencode`].
    oc: OpenCodeTurn,
    /// What the last line was, so the caller need not parse it again to find
    /// out. See [`StreamParser::last_line`].
    last_line: LineKind,
}

/// What one line of a CLI's stdout turned out to be.
///
/// The distinction that matters is [`LineKind::Plain`]: a non-blank line that is
/// not JSON is a diagnostic the CLI printed as prose — an expired OAuth session,
/// a missing model — and it is the only explanation the user will get, so it is
/// kept and shown when the turn ends badly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineKind {
    /// Nothing was pushed yet, or the line was blank.
    #[default]
    Blank,
    /// Valid JSON, whether or not this dialect had anything to say about it.
    Json,
    /// Not JSON: prose the CLI printed.
    Plain,
}

/// What an OpenCode turn accumulates across its steps.
///
/// Every other harness hands the parser a finished turn's numbers in one event.
/// This one reports them per step and stops writing when it is done, so the
/// running totals — and the wall clock, which it reports as nothing at all — are
/// assembled here.
#[derive(Default)]
struct OpenCodeTurn {
    /// `SessionStarted` is emitted once, off whichever event arrives first.
    session_announced: bool,
    first_ts: Option<u64>,
    last_ts: Option<u64>,
    input: u64,
    output: u64,
    /// Distinguishes "no tokens reported" from "zero tokens", so a footer shows
    /// nothing rather than a confident `0 in / 0 out`.
    saw_tokens: bool,
}

impl OpenCodeTurn {
    fn stats(&self) -> TurnStats {
        TurnStats {
            // Nothing in the stream states a duration, but every event is
            // timestamped in milliseconds, so the turn's own span is exact.
            duration_ms: match (self.first_ts, self.last_ts) {
                (Some(a), Some(b)) if b >= a => Some(b - a),
                _ => None,
            },
            input_tokens: self.saw_tokens.then_some(self.input),
            output_tokens: self.saw_tokens.then_some(self.output),
        }
    }
}

/// `schemaic_run_query` → `mcp__schemaic__run_query`; anything else unchanged.
///
/// **OpenCode flattens the server into the tool name**, where Codex keeps them
/// as separate fields its decoder rejoins. So our own server's tools arrive
/// prefixed and everything else — the built-ins, and any server a future config
/// adds — arrives bare. Rewriting only our prefix keeps the transcript and the
/// allow-list speaking the one qualified form every other harness produces,
/// while a built-in still shows under its own name rather than being dressed up
/// as an MCP call.
fn opencode_tool_name(raw: &str) -> String {
    let prefix = format!("{}_", crate::harness::MCP_SERVER);
    match raw.strip_prefix(&prefix) {
        Some(bare) if !bare.is_empty() => {
            format!("mcp__{}__{bare}", crate::harness::MCP_SERVER)
        }
        _ => raw.to_string(),
    }
}

/// The qualified name of one Copilot tool call.
///
/// **Rebuilt from the fields, not unflattened from the name.** Copilot reports
/// `toolName: "schemaic-list_schema"` *and* `mcpServerName`/`mcpToolName`
/// beside it, so this is Codex's rebuild rather than OpenCode's prefix-strip:
/// splitting `schemaic-list_schema` on its dash would misread any server whose
/// own name has one. A built-in carries no server field and keeps its own name,
/// for the module docs' reason — it should never run here, and if it does the
/// user should see it under the name it really has.
fn copilot_tool_name(d: &serde_json::Value) -> String {
    let s = |k: &str| d.get(k).and_then(|x| x.as_str()).unwrap_or("");
    match (s("mcpServerName"), s("mcpToolName")) {
        (server, tool) if !server.is_empty() && !tool.is_empty() => {
            format!("mcp__{server}__{tool}")
        }
        // A completion carries neither field; its chip still needs a label.
        _ => match s("toolName") {
            "" => "tool".to_string(),
            name => name.to_string(),
        },
    }
}

/// The text of a Copilot `tool.execution_complete`.
///
/// `result.content` is the flattened text; `result.contents` is the MCP blocks
/// it was flattened from, read only when the flat form is missing.
///
/// **Only successes were captured.** `error.message` is read first on the
/// assumption that a failure names itself the way Codex's does, and nothing
/// depends on it being right: `success: false` is what marks the chip as an
/// error, and a failure shaped some other way shows an empty result rather than
/// a wrong one.
fn copilot_result_text(d: &serde_json::Value) -> String {
    if let Some(m) = d.pointer("/error/message").and_then(|m| m.as_str()) {
        return m.to_string();
    }
    if let Some(t) = d.pointer("/result/content").and_then(|c| c.as_str()) {
        return t.to_string();
    }
    d.pointer("/result/contents")
        .and_then(|c| c.as_array())
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// The chip name for one Cursor tool call.
///
/// An MCP call is rebuilt from `args.providerIdentifier` (or
/// `serverIdentifier`) and `args.toolName`, not split out of `args.name`
/// (`schemaic-run_query`), for Copilot's reason: a server name with a dash
/// would split wrong. A built-in is its kind with the `ToolCall` suffix off —
/// `read`, `grep`, `shell`, `edit`. A refused MCP completion carries no `args`
/// at all (measured), so a call first seen that way is labelled `mcp` rather
/// than guessed.
fn cursor_tool_name(kind: &str, body: &serde_json::Value) -> String {
    if kind == "mcpToolCall" {
        let a = |k: &str| body.pointer(&format!("/args/{k}")).and_then(|x| x.as_str());
        if let (Some(server), Some(tool)) = (
            a("providerIdentifier").or_else(|| a("serverIdentifier")),
            a("toolName"),
        ) && !server.is_empty()
            && !tool.is_empty()
        {
            return format!("mcp__{server}__{tool}");
        }
    }
    kind.strip_suffix("ToolCall").unwrap_or(kind).to_string()
}

/// The text and the verdict of a Cursor tool call's `result`.
///
/// `success` is the only key that is not a refusal. Its text is `content`: a
/// string for the built-ins, an array of `{"text": {"text": …}}` blocks for an
/// MCP call (measured — the text is nested one level deeper than MCP's own
/// shape), and an MCP result can still say `isError`. Every other key names a
/// refusal, and its text is whichever reason field it carries.
///
/// **`success` is looked up, not taken as the first key.** The workspace's
/// `serde_json` has no `preserve_order`, so a map iterates alphabetically, and
/// any sibling sorting before `success` (`isBackground`, `metadata`) turned a
/// successful call into a refusal named after that sibling.
fn cursor_result(r: Option<&serde_json::Value>) -> (String, bool) {
    let Some(o) = r.and_then(|r| r.as_object()) else {
        return (String::new(), false);
    };
    let Some((key, val)) = o.get_key_value("success").or_else(|| o.iter().next()) else {
        return (String::new(), false);
    };
    if key != "success" {
        let why = ["reason", "error", "errorMessage", "clientVisibleError"]
            .iter()
            .filter_map(|k| val.get(*k).and_then(|x| x.as_str()))
            .find(|s| !s.is_empty())
            .unwrap_or(key.as_str());
        return (why.to_string(), true);
    }
    let text = match val.get("content") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|b| match b.get("text") {
                Some(serde_json::Value::String(s)) => Some(s.as_str()),
                Some(t) => t.get("text").and_then(|x| x.as_str()),
                None => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };
    let is_error = val.get("isError").and_then(|e| e.as_bool()) == Some(true);
    (text, is_error)
}

/// A Cursor turn's numbers, from its `result`.
///
/// **Cached input is input.** `usage` partitions the prompt: a resumed second
/// turn reported 287 `inputTokens` beside 24,960 `cacheReadTokens` — 287 alone
/// cannot be a prompt carrying the first turn's context — so the footer adds
/// the two, as OpenCode's does. `cacheWriteTokens` is left out, for OpenCode's
/// reason and with less evidence: it was 0 in every measured turn, so whether it
/// is a part of the prompt or a count within one is unknown.
fn cursor_stats(v: &serde_json::Value) -> TurnStats {
    let at = |k: &str| v.pointer(&format!("/usage/{k}")).and_then(|n| n.as_u64());
    let input = match (at("inputTokens"), at("cacheReadTokens")) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
    };
    TurnStats {
        duration_ms: v.get("duration_ms").and_then(|d| d.as_u64()),
        input_tokens: input,
        output_tokens: at("outputTokens"),
    }
}

impl StreamParser {
    pub fn new(harness: Harness) -> Self {
        Self {
            harness,
            text: Coalescer::default(),
            seen_tools: std::collections::HashSet::new(),
            oc: OpenCodeTurn::default(),
            last_line: LineKind::Blank,
        }
    }

    /// Whether this tool call is being announced for the first time.
    ///
    /// **Unique within one *turn*, which is a narrower promise than the stream.**
    /// The set lived on the parser because ids mean nothing outside it — the same
    /// reason the coalescer does — and while Claude was the only harness whose
    /// stream held more than one turn, "per stream" and "per turn" were the same
    /// scope; Claude's ids are unique for the life of the process either way.
    /// Antigravity numbers its steps from zero on each turn, so on a persistent
    /// one the second turn reuses the first's ids. [`StreamParser::push`] clears
    /// this at every turn boundary for that reason.
    ///
    /// **An item with no id of its own is announced rather than swallowed.**
    /// Codex's `id` is read as `unwrap_or("")`, so every id-less item in a turn
    /// shared one key — the second `run_query` ran and left no chip, no SQL and
    /// no result behind it. The Antigravity arm answers the identical question
    /// the other way and says why: silently dropping a call is the worse failure
    /// of the two, and `push`'s own comment calls it "the one failure this whole
    /// dialect is decoded carefully to avoid".
    ///
    /// Both keys such an item produces are listed, since the completion half is
    /// keyed `"{id}\0done"` and an empty id makes that `"\0done"` rather than
    /// empty. Neither is reachable for a real id.
    fn first_sight(&mut self, id: &str) -> bool {
        if id.is_empty() || id == "\u{0}done" {
            return true;
        }
        self.seen_tools.insert(id.to_string())
    }

    /// One Codex side-effect item — a shell command, a file change — turned into
    /// the same guarded chip-open / chip-close pair `mcp_tool_call` gets.
    ///
    /// **Both guards, because these two arms had neither.** `mcp_tool_call` was
    /// given `first_sight` on each half and its siblings were not, so:
    ///
    /// - A *streamed* step emitted an unconditional `ToolUse` on every
    ///   `!completed` line. Codex restates an item on `item.updated`, so four
    ///   lines for one shell command opened four chips, three of which never
    ///   receive a result and spin for the rest of the turn.
    /// - A step whose only line is `item.completed` — nothing opened it — emitted
    ///   a `ToolResult` with no chip to land in, and `TurnState::apply` attaches
    ///   a loose result to *the most recent tool call still awaiting one*. That
    ///   staples `ls` output, or the contents of a file, onto whatever
    ///   `run_query` chip happened to be open, and the user reads it as the
    ///   answer to their query.
    ///
    /// **The completed-only shape is the measured normal case, not an edge
    /// case.** Three real `codex exec --json` turns on codex-cli 0.153.4 carried
    /// no `item.started` at all — every item arrived bare, as one
    /// `item.completed`. So the second failure above was not the unlikely half
    /// of this finding; it was the half that fires every time an item this arm
    /// handles occurs. `command_execution` itself stayed uncaptured — that
    /// environment refuses shell execution before an item is created — so the
    /// guard is written to be right for both shapes rather than for the one that
    /// was observed.
    ///
    /// The done-key is the same `id + NUL + "done"` shape, so one id can announce
    /// once and resolve once.
    fn side_effect(
        &mut self,
        id: &str,
        completed: bool,
        name: &str,
        text: impl FnOnce() -> String,
        is_error: bool,
    ) -> Vec<StreamEvent> {
        let mut out = Vec::new();
        if self.first_sight(id) {
            out.push(StreamEvent::ToolUse {
                name: name.to_string(),
                sql: None,
                id: Some(id.to_string()),
            });
        }
        if completed && self.first_sight(&format!("{id}\u{0}done")) {
            out.push(StreamEvent::ToolResult {
                text: text(),
                is_error,
                id: Some(id.to_string()),
            });
        }
        out
    }

    /// Which harness this parser decodes.
    pub fn harness(&self) -> Harness {
        self.harness
    }

    /// What the line last handed to [`StreamParser::push`] was.
    ///
    /// **So the caller does not have to parse it again to find out.** Both
    /// session tasks need to tell "no events because this is not JSON" — a fatal
    /// error the CLI printed as prose, which is the only diagnostic there will
    /// be — from "no events because this is JSON I ignore". They answered it by
    /// running `serde_json::from_str` on the line a second time, after `push`
    /// had already done so, for every line of every turn.
    pub fn last_line(&self) -> LineKind {
        self.last_line
    }

    /// Decode one output line into zero or more events.
    pub fn push(&mut self, line: &str) -> Vec<StreamEvent> {
        // **A BOM survives `trim`.** U+FEFF is not `White_Space`, so a byte-order
        // mark on the first line — which a Windows console redirect or a shim
        // that re-encodes a pipe can prepend — made `from_str` fail, and the
        // line was filed as prose. On the two dialects that carry the session id
        // on their opening line that costs the whole conversation's continuity,
        // not one event: `resume` never learns the id, so every later turn opens
        // a fresh conversation.
        let line = line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() {
            self.last_line = LineKind::Blank;
            return Vec::new();
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            self.last_line = LineKind::Plain;
            return Vec::new();
        };
        self.last_line = LineKind::Json;
        let out = match self.harness {
            Harness::Claude => crate::parse_stream_value(&v),
            Harness::Codex => self.push_codex(&v),
            Harness::Antigravity => self.push_antigravity(&v),
            Harness::OpenCode => self.push_opencode(&v),
            Harness::Copilot => self.push_copilot(&v),
            Harness::Cursor => self.push_cursor(&v),
        };
        // **A turn boundary clears the per-turn state, and it has to now that a
        // stream can hold more than one turn.** `seen_tools` is keyed by whatever
        // id the dialect gives a call, and those ids are unique only *within a
        // turn* — Antigravity numbers its steps from zero on each one. That was
        // invisible while every multi-turn stream was Claude's, whose ids are
        // unique for the life of the process; on a persistent Antigravity the
        // second turn's first tool call carries `step_index` 0 again, the set
        // already holds it, and the chip announcing it is dropped. A tool call
        // that ran with nothing on screen to say so is the one failure this
        // whole dialect is decoded carefully to avoid.
        if out
            .iter()
            .any(|e| matches!(e, StreamEvent::TurnDone { .. }))
        {
            self.seen_tools.clear();
            self.text = Coalescer::default();
            self.oc = OpenCodeTurn::default();
        }
        out
    }

    /// `opencode run --format json`.
    ///
    /// **The dialect with no ending.** Every other harness here says when a turn
    /// is over — `turn.completed`, `result`, a final `stream-json` message. This
    /// printer just stops writing when the session goes idle, so the close has to
    /// be *inferred*, and the only thing carrying that information is
    /// `step_finish.reason`: `tool-calls` means the model is going round again,
    /// anything else means it has stopped. Getting it wrong is visible in both
    /// directions — end early and the answer is cut off mid-turn, never end and
    /// the app closes the turn on process exit and reports a working turn as
    /// "ended unexpectedly".
    ///
    /// An absent `reason` is read as terminal. It has never been observed absent
    /// (the AI SDK behind it always sets a finish reason), and of the two ways to
    /// be wrong about a value that does not occur, this one degrades to a
    /// truncated turn only if a *tool* step ever omits it, while the other would
    /// stamp an error on every ordinary turn.
    ///
    /// **Tokens are per step, not per turn**, so they accumulate here rather than
    /// being read off the last event. The measured two-step turn reported 2497
    /// input and then 637; a footer showing only the second understates the turn
    /// by a factor of four.
    fn push_opencode(&mut self, v: &serde_json::Value) -> Vec<StreamEvent> {
        let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let mut out = Vec::new();

        // The session id arrives on the top level of *every* event rather than in
        // an opening one of its own, so it is taken from whichever comes first
        // and only once. It is what `--session` resumes the next turn with.
        if let Some(id) = v.get("sessionID").and_then(|s| s.as_str())
            && !id.is_empty()
            && !self.oc.session_announced
        {
            self.oc.session_announced = true;
            out.push(StreamEvent::SessionStarted { id: id.to_string() });
        }
        if let Some(ts) = v.get("timestamp").and_then(|t| t.as_u64()) {
            self.oc.first_ts.get_or_insert(ts);
            self.oc.last_ts = Some(ts);
        }

        let part = v.get("part").unwrap_or(&serde_json::Value::Null);
        match ty {
            // Nothing to render: it opens a step, and a step is not a turn.
            "step_start" => {}
            "text" => {
                // Whole parts, never deltas — see `Harness::streams_deltas`. The
                // printer only emits one once `time.end` is set and dedupes by
                // part id before it gets here, so there is nothing to coalesce.
                if let Some(t) = part.get("text").and_then(|t| t.as_str())
                    && !t.is_empty()
                {
                    out.push(StreamEvent::TextDelta(t.to_string()));
                }
            }
            "tool_use" => out.extend(self.opencode_tool(part)),
            "step_finish" => {
                let at = |k: &str| {
                    part.pointer(&format!("/tokens/{k}"))
                        .and_then(|n| n.as_u64())
                };
                // **Four fields, not two.** The footer summed `input` and
                // `output` alone, which drops every cached input token — and the
                // captured fixture's own `total` proves it: it reconciles
                // exactly, in both of its steps, as
                // `input + output + reasoning + cache.read`. A turn that really
                // spent 5,181 tokens rendered `↑3.1k ↓12`, understating the
                // input by 39%, and the gap widens as cache reads grow. Cached
                // input is billed input; it is not free, and it is not nothing.
                //
                // `cache.write` is deliberately absent: it is not part of the
                // fixture's `total`, so adding it would overstate the turn by
                // the same reasoning that understating it was wrong.
                for k in ["input", "reasoning", "cache/read"] {
                    if let Some(n) = at(k) {
                        self.oc.input += n;
                        self.oc.saw_tokens = true;
                    }
                }
                if let Some(o) = at("output") {
                    self.oc.output += o;
                    self.oc.saw_tokens = true;
                }
                let reason = part.get("reason").and_then(|r| r.as_str());
                if reason != Some("tool-calls") {
                    // **A turn cut short is not a clean turn.** Only the exact
                    // string `"error"` used to be flagged, so a `reason` of
                    // `"length"` — the model hit its output cap — or
                    // `"content-filter"` filed a truncated or withheld answer as
                    // a success, with nothing on screen to say the last sentence
                    // was not the end of one. The Antigravity arm applies the
                    // opposite rule (anything but `SUCCESS` is a failure) to the
                    // same question.
                    //
                    // Named rather than inverted, and that is the paragraph
                    // above's reasoning applied a second time: an unrecognised
                    // reason must not stamp an error on an ordinary turn, so
                    // only the ones whose meaning is known are flagged.
                    if let Some(note) = opencode_cutoff_note(reason) {
                        out.push(StreamEvent::TextDelta(note.to_string()));
                    }
                    out.push(StreamEvent::TurnDone {
                        is_error: opencode_is_failure(reason),
                        stats: self.oc.stats(),
                    });
                }
            }
            // `session.error`, the printer's only failure event. Nothing follows
            // it, so the turn is closed here or not at all.
            "error" => {
                let e = v.get("error").unwrap_or(&serde_json::Value::Null);
                let msg = e
                    .pointer("/data/message")
                    .and_then(|m| m.as_str())
                    .or_else(|| e.get("name").and_then(|n| n.as_str()))
                    .unwrap_or_default();
                if !msg.is_empty() {
                    out.push(StreamEvent::TextDelta(format!("\n{msg}\n")));
                }
                out.push(StreamEvent::TurnDone {
                    is_error: true,
                    stats: self.oc.stats(),
                });
            }
            // `reasoning` among them: it only appears under `--thinking`, which
            // is not passed, and the panel has no place for it — same call the
            // Codex dialect makes for its `reasoning` items.
            _ => {}
        }
        out
    }

    /// One `tool_use` part.
    ///
    /// **The call and its result arrive together**, unlike both other per-turn
    /// dialects: a measured call was reported once, already `completed`, with its
    /// `input` and `output` in the same event. So this emits the chip and fills
    /// it from one line. The `running` status is handled anyway — the state
    /// machine has one — and `seen_tools` keeps a call announced once if a build
    /// ever does restate it.
    fn opencode_tool(&mut self, part: &serde_json::Value) -> Vec<StreamEvent> {
        let raw = part
            .get("tool")
            .and_then(|t| t.as_str())
            .unwrap_or_default();
        let state = part.get("state").unwrap_or(&serde_json::Value::Null);
        let status = state.get("status").and_then(|s| s.as_str()).unwrap_or("");
        // `callID` is the dialect's own id for the call; the part id changes
        // between restatements where the call id does not.
        let id = part
            .get("callID")
            .and_then(|c| c.as_str())
            .unwrap_or(raw)
            .to_string();

        let mut out = Vec::new();
        if self.first_sight(&id) {
            let sql = state
                .pointer("/input/sql")
                .or_else(|| state.pointer("/input/query"))
                .and_then(|s| s.as_str())
                .map(|s| s.to_string());
            out.push(StreamEvent::ToolUse {
                name: opencode_tool_name(raw),
                sql,
                id: Some(id.clone()),
            });
        }
        match status {
            // Still running: the chip stays open, and the result arrives on a
            // later event for the same `callID`.
            "running" | "pending" | "" => {}
            _ => {
                let is_error = status == "error";
                let text = state
                    .get("error")
                    .and_then(|e| e.as_str())
                    .or_else(|| state.get("output").and_then(|o| o.as_str()))
                    .unwrap_or_default()
                    .to_string();
                out.push(StreamEvent::ToolResult {
                    text,
                    is_error,
                    id: Some(id.clone()),
                });
            }
        }
        out
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
                        let evs = agy_tool_step(s, s.get("step_index").map(|i| i.to_string()));
                        // Every `ACTIVE` for one `step_index` is the same call
                        // being restated, so only the first announces a chip —
                        // see `StreamParser::seen_tools`. A step with no index
                        // is announced rather than swallowed: silently dropping
                        // a call is the worse failure of the two, and every
                        // measured step carried one.
                        //
                        // **And the result half is guarded too**, keyed apart
                        // the way `codex_item`'s is, for the reason that arm's
                        // own doc gives: a second `ToolResult` for one call is
                        // attached by `TurnState::apply` to *the most recent
                        // tool call still awaiting one*. Measured: step 0
                        // opens, step 1 opens, step 0 completes twice — and
                        // step 1, which has not finished and may never, was
                        // shown as complete carrying step 0's output. The
                        // dedupe matched only on a `ToolUse` first event, so it
                        // followed the shape the captured turns happen to
                        // restate rather than the rule `seen_tools` states for
                        // both per-turn dialects.
                        match (evs.first(), s.get("step_index")) {
                            (Some(StreamEvent::ToolUse { .. }), Some(i))
                                if !self.first_sight(&i.to_string()) =>
                            {
                                Vec::new()
                            }
                            (Some(StreamEvent::ToolResult { .. }), Some(i))
                                if !self.first_sight(&format!("{i}\u{0}done")) =>
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
                // **A denial is not the turn failing.** `is_error` paints the
                // whole bubble as a plain-text error — see
                // `opencode_is_failure` — and a refused *side* tool can sit
                // beside an answer that is complete. The note above is what
                // says the refusal happened; the status is what says the turn
                // did not produce one.
                out.push(StreamEvent::TurnDone {
                    is_error: failed,
                    stats: antigravity_stats(r),
                });
                out
            }
            _ => Vec::new(),
        }
    }

    /// `copilot --output-format json` (GitHub Copilot CLI).
    ///
    /// **Its ending is the one event that is shaped differently.** Every event
    /// nests its payload under `data`; `result` carries `sessionId` and
    /// `exitCode` at the top level instead, and it is the only turn-completion
    /// event there is. `assistant.turn_end` is not one: it closes a single model
    /// call, and a measured turn with two tool calls emitted three.
    ///
    /// **The session id arrives last, not first**, so it is emitted beside the
    /// `TurnDone` rather than at the start of the turn — ahead of it in the same
    /// batch, so the app has kept it before the turn is closed. A turn stopped
    /// before `result` leaves no id behind, and the next one opens a fresh
    /// conversation; `turn_system` then sends the schema again, which is the
    /// right thing for a conversation that has never seen it.
    ///
    /// **Text arrives twice**: `assistant.message_delta`s, then the whole
    /// message again in `assistant.message`. Both are fed through the coalescer
    /// under the message's id, so the restatement prints only what the deltas
    /// never sent — nothing, when streaming is on, and the whole message when it
    /// is not.
    fn push_copilot(&mut self, v: &serde_json::Value) -> Vec<StreamEvent> {
        let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let d = v.get("data").unwrap_or(&serde_json::Value::Null);
        let s = |k: &str| d.get(k).and_then(|x| x.as_str()).unwrap_or("");
        match ty {
            "assistant.message_delta" => self
                .text
                .extend(s("messageId"), s("deltaContent"))
                .map(|t| vec![StreamEvent::TextDelta(t)])
                .unwrap_or_default(),
            "assistant.message" => {
                let id = s("messageId");
                let out = self
                    .text
                    .settle(id, s("content"))
                    .map(|t| vec![StreamEvent::TextDelta(t)])
                    .unwrap_or_default();
                self.text.clear(id);
                out
            }
            "tool.execution_start" => {
                let id = s("toolCallId").to_string();
                if !self.first_sight(&id) {
                    return Vec::new();
                }
                let sql = d
                    .pointer("/arguments/sql")
                    .or_else(|| d.pointer("/arguments/query"))
                    .and_then(|x| x.as_str())
                    .map(|x| x.to_string());
                vec![StreamEvent::ToolUse {
                    name: copilot_tool_name(d),
                    sql,
                    id: Some(id),
                }]
            }
            "tool.execution_complete" => {
                let id = s("toolCallId").to_string();
                let mut out = Vec::new();
                // **A completion nothing announced opens its own chip.** Every
                // captured call had its `execution_start`, but a result with no
                // chip to land in is attached by `TurnState::apply` to *the most
                // recent call still awaiting one* — another call's chip — which
                // is the defect `side_effect` records for Codex. The completion
                // carries no tool name, so the chip is labelled by what it has.
                if self.first_sight(&id) {
                    out.push(StreamEvent::ToolUse {
                        name: copilot_tool_name(d),
                        sql: None,
                        id: Some(id.clone()),
                    });
                }
                // Keyed apart from the announcement, as every per-turn dialect
                // here keys it: one id announces once and resolves once.
                if self.first_sight(&format!("{id}\u{0}done")) {
                    let ok = d.get("success").and_then(|x| x.as_bool()) == Some(true);
                    out.push(StreamEvent::ToolResult {
                        text: copilot_result_text(d),
                        is_error: !ok,
                        id: Some(id),
                    });
                }
                out
            }
            "result" => {
                let mut out = Vec::new();
                if let Some(id) = v.get("sessionId").and_then(|x| x.as_str())
                    && !id.is_empty()
                {
                    out.push(StreamEvent::SessionStarted { id: id.to_string() });
                }
                // An absent code is not a failure: nothing has ever omitted it,
                // and reading its absence as one would paint an ordinary answer
                // as an error — `opencode_is_failure`'s reasoning.
                let code = v.get("exitCode").and_then(|x| x.as_i64()).unwrap_or(0);
                out.push(StreamEvent::TurnDone {
                    is_error: code != 0,
                    // **No numbers, rather than wrong ones.** `usage` carries
                    // `sessionDurationMs`, `totalApiDurationMs` and
                    // `premiumRequests`, and all three are *per session*:
                    // measured across a resumed pair, the second turn reported
                    // 6,500 ms against the first's 1,992 and two premium
                    // requests against one. No field in `result` is this
                    // turn's, and the panel's own live counter already shows
                    // its elapsed time.
                    stats: TurnStats::default(),
                });
                out
            }
            // `session.*` (MCP status, the disabled-tool list, usage
            // checkpoints), `assistant.turn_start`/`turn_end`,
            // `assistant.reasoning`, `tool_call_delta`, `user.message`: none is
            // transcript. Reasoning is dropped for the reason Codex's is.
            _ => Vec::new(),
        }
    }

    /// `cursor-agent -p --output-format stream-json --stream-partial-output`.
    ///
    /// **Three shapes of `assistant`, told apart by their fields.** Measured on
    /// the 2026-09-23 build, one turn with a tool call in the middle wrote:
    ///
    /// - deltas — `timestamp_ms` and no `model_call_id` — one per few tokens;
    /// - before the tool call, the segment so far **restated whole**, carrying
    ///   `timestamp_ms` *and* `model_call_id`;
    /// - at the end, the last segment restated whole with **no** `timestamp_ms`.
    ///
    /// So "timestamped means delta" would print the pre-tool segment twice. A
    /// delta is `timestamp_ms` without `model_call_id`; anything else is a
    /// restatement, fed through the coalescer against what that segment's
    /// deltas already sent, so it prints only an unseen tail — nothing when
    /// streaming, the whole segment when not. A tool call starts a new segment,
    /// which is what makes each restatement comparable to its own deltas and
    /// not the turn's.
    ///
    /// **The session id comes first**, on `system`/`init`, as Codex's does; the
    /// next turn resumes with it.
    fn push_cursor(&mut self, v: &serde_json::Value) -> Vec<StreamEvent> {
        const SEG: &str = "cursor-segment";
        let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match ty {
            "system" => match v.get("subtype").and_then(|s| s.as_str()) {
                Some("init") => v
                    .get("session_id")
                    .and_then(|s| s.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|id| vec![StreamEvent::SessionStarted { id: id.to_string() }])
                    .unwrap_or_default(),
                _ => Vec::new(),
            },
            "assistant" => {
                let text: String = v
                    .pointer("/message/content")
                    .and_then(|c| c.as_array())
                    .map(|blocks| {
                        blocks
                            .iter()
                            .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                            .collect()
                    })
                    .unwrap_or_default();
                let delta = v.get("timestamp_ms").is_some() && v.get("model_call_id").is_none();
                let out = match delta {
                    true => self.text.extend(SEG, &text),
                    false => {
                        let tail = self.text.settle(SEG, &text);
                        self.text.clear(SEG);
                        tail
                    }
                };
                out.map(|t| vec![StreamEvent::TextDelta(t)])
                    .unwrap_or_default()
            }
            "tool_call" => {
                self.text.clear(SEG);
                self.cursor_tool(v)
            }
            "result" => {
                let failed = v.get("is_error").and_then(|e| e.as_bool()) == Some(true)
                    || v.get("subtype")
                        .and_then(|s| s.as_str())
                        .is_some_and(|s| s != "success");
                vec![StreamEvent::TurnDone {
                    is_error: failed,
                    stats: cursor_stats(v),
                }]
            }
            // `thinking` (reasoning, dropped as every dialect's is) and `user`
            // (our prompt echoed back).
            _ => Vec::new(),
        }
    }

    /// One `tool_call` event, `started` or `completed`.
    ///
    /// **The kind is a key, not a field**: the payload sits under
    /// `tool_call.<kind>ToolCall` — `mcpToolCall`, `readToolCall`,
    /// `shellToolCall` — and the verdict under its `result` as a key too:
    /// `success`, or one of several refusals (`rejected`, `permissionDenied`,
    /// `writePermissionDenied`, `readPermissionDenied`, `error`, all measured).
    ///
    /// `getMcpToolsToolCall` is dropped: it is the CLI looking up which MCP
    /// tools exist, which the model does before nearly every call, and a chip
    /// for it would sit beside every real one saying nothing. Every other
    /// built-in is shown under its own name, for the module docs' reason —
    /// this harness's readers run unprompted, so they are exactly what the user
    /// needs to see, and a refused writer is shown refused.
    ///
    /// Both halves guarded, keyed by `call_id`, and a completion with no start
    /// still opens its chip — the rule `side_effect` states for Codex.
    fn cursor_tool(&mut self, v: &serde_json::Value) -> Vec<StreamEvent> {
        let Some(tc) = v.get("tool_call").and_then(|t| t.as_object()) else {
            return Vec::new();
        };
        let Some((kind, body)) = tc.iter().find(|(k, _)| k.ends_with("ToolCall")) else {
            return Vec::new();
        };
        if kind == "getMcpToolsToolCall" {
            return Vec::new();
        }
        let id = v
            .get("call_id")
            .and_then(|c| c.as_str())
            .unwrap_or_default()
            .to_string();
        let completed = v.get("subtype").and_then(|s| s.as_str()) == Some("completed");
        let mut out = Vec::new();
        if self.first_sight(&id) {
            let sql = body
                .pointer("/args/args/sql")
                .or_else(|| body.pointer("/args/args/query"))
                .and_then(|s| s.as_str())
                .map(|s| s.to_string());
            out.push(StreamEvent::ToolUse {
                name: cursor_tool_name(kind, body),
                sql,
                id: Some(id.clone()),
            });
        }
        if completed && self.first_sight(&format!("{id}\u{0}done")) {
            let (text, is_error) = cursor_result(body.get("result"));
            out.push(StreamEvent::ToolResult {
                text,
                is_error,
                id: Some(id),
            });
        }
        out
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
                // **Both halves are guarded, and the result half was not.**
                // `first_sight` kept a restated call from opening a second chip,
                // but the completion path emitted a `ToolResult` unconditionally
                // — so a call whose only event is `item.completed` produced a
                // result with no chip to land in, and `TurnState::apply` attaches
                // a loose result to *the most recent tool call still awaiting
                // one*. That stamps a database answer onto whatever chip happened
                // to be open (a `command_execution`, say), or drops it entirely
                // when none is, and the user sees no record of a query that ran.
                // A restated `completed` had the mirror problem: a second
                // `ToolResult` for one call.
                let mut out = Vec::new();
                if self.first_sight(id) {
                    out.push(StreamEvent::ToolUse {
                        name,
                        sql,
                        id: Some(id.to_string()),
                    });
                }
                if !completed {
                    return out;
                }
                // Keyed apart from the call itself: one id has to be able to
                // announce once *and* resolve once.
                //
                // The separator is a NUL because Codex mints these ids and none
                // it has produced contains one — *not* because a NUL is
                // impossible in a JSON string, which it is not: `\u0000` is
                // legal and `serde_json` decodes it, so an id of `t1`+NUL+`done`
                // would collide with item `t1`'s done-key. Nothing outside that
                // CLI chooses an id, so this is a statement about the source and
                // not about the encoding.
                if self.first_sight(&format!("{id}\u{0}done")) {
                    let err = item
                        .pointer("/error/message")
                        .and_then(|m| m.as_str())
                        .map(|s| s.to_string());
                    let is_error = err.is_some()
                        || item.get("status").and_then(|s| s.as_str()) == Some("failed");
                    let text = err.unwrap_or_else(|| mcp_result_text(item));
                    out.push(StreamEvent::ToolResult {
                        text,
                        is_error,
                        id: Some(id.to_string()),
                    });
                }
                out
            }
            // Side effects. They should not happen under the constraint this
            // harness is launched with; if one does, it is shown rather than
            // swallowed. See the module docs.
            "command_execution" => {
                let cmd = item.get("command").and_then(|c| c.as_str()).unwrap_or("");
                let code = item.get("exit_code").and_then(|c| c.as_i64());
                let status = item.get("status").and_then(|s| s.as_str()).unwrap_or("");
                self.side_effect(
                    id,
                    completed,
                    "shell",
                    || {
                        format!(
                            "{cmd}\n{}",
                            item.get("aggregated_output")
                                .and_then(|o| o.as_str())
                                .unwrap_or("")
                        )
                    },
                    status == "failed" || code.is_some_and(|c| c != 0),
                )
            }
            "file_change" => {
                let failed = item.get("status").and_then(|s| s.as_str()) == Some("failed");
                self.side_effect(
                    id,
                    completed,
                    "file_change",
                    || {
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
                        paths.join("\n")
                    },
                    failed,
                )
            }
            "error" => {
                let msg = item.get("message").and_then(|m| m.as_str()).unwrap_or("");
                vec![StreamEvent::TextDelta(format!("\n{msg}\n"))]
            }
            _ => Vec::new(),
        }
    }
}

/// Did an OpenCode step finish with **nothing to show**? Pure, so the rule has a
/// test that does not have to drive a whole turn.
///
/// **`is_error` is a rendering decision, not a diagnosis.** Its only consumer
/// sets `Role::Error`, and an error turn renders every `Seg::Text` as plain
/// `text()` in `theme::error()` and never calls `render_markdown` — headings and
/// tables become raw `#`/`|`, and fenced SQL loses the Insert/Run/Propose bar.
/// That is right for what it was built for (a spawn failure, a `turn.failed`
/// with nothing but a message) and wrong for a turn that carries an answer.
///
/// So `length` and `content-filter` are **not** failures here, though they are
/// certainly not clean endings either: the answer on screen is real prose that
/// simply stops, and what says so is [`opencode_cutoff_note`], whose own doc
/// promises *"The answer stays on screen either way"*. It did stay — unreadable,
/// with the advisory sentence itself rendering as its literal `_underscores_`,
/// because nothing parsed it any more.
fn opencode_is_failure(reason: Option<&str>) -> bool {
    matches!(reason, Some("error"))
}

/// What to tell the user when a turn stopped for a reason that is not the model
/// having finished, or `None` when it is.
///
/// The answer stays on screen either way — this is the sentence that says the
/// last one was not the end of it.
fn opencode_cutoff_note(reason: Option<&str>) -> Option<&'static str> {
    match reason {
        Some("length") => Some(
            "\n\n_The answer stops here because the model reached its output limit — \
             it is not finished._\n",
        ),
        Some("content-filter") => {
            Some("\n\n_The rest of the answer was withheld by a content filter._\n")
        }
        _ => None,
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
///
/// **`cached_input_tokens` and `reasoning_output_tokens` are deliberately not
/// added, and this is the opposite call from the one OpenCode's arm makes.**
/// That is worth writing down because the two look alike and the wrong
/// inference is one line away: OpenCode's footer really did drop its cache
/// reads, and its own `total` field proved the four parts sum to it, so
/// `push_opencode` adds them. Codex publishes no `total`, and its numbers say
/// the fields *nest* rather than partition — measured across three turns on
/// codex-cli 0.153.4, `cached_input_tokens` was 9,984 of 12,595 input, 34,048
/// of 38,134, and 22,016 of 25,058: always a fraction of the input beside it,
/// never a sibling of it. Adding them would double-count the cache on every
/// turn, overstating a Codex conversation by roughly the amount the OpenCode
/// fix stopped understating one.
///
/// Not proven, and it does not need to be to justify the direction: with no
/// `total` to reconcile against, the conservative reading is the one that
/// cannot invent tokens the user never spent. A `total` appearing in a future
/// build settles it.
///
/// (`CODEX_REAL_TURN` reports `cached_input_tokens: 9984` as well, against a
/// different `input_tokens` and a five-token answer. Coincidence rather than a
/// transcription slip: 9,984 is 78 × 128, and these land on cache-block
/// boundaries — worth saying so once rather than having the next reader check.)
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
fn agy_tool_step(s: &serde_json::Value, id: Option<String>) -> Vec<StreamEvent> {
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
            vec![StreamEvent::ToolUse { name, sql, id }]
        }
        "ERROR" => vec![StreamEvent::ToolResult {
            id,
            text: info
                .pointer("/error/message")
                .and_then(|m| m.as_str())
                .unwrap_or("the harness refused this tool")
                .to_string(),
            is_error: true,
        }],
        "DONE" => vec![StreamEvent::ToolResult {
            id,
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

    /// **Every branch of the accumulator, driven directly** — and every input
    /// here is **hand-written, because the shape does not occur.** Measured on
    /// codex-cli 0.153.4, three real turns emitted no `item.updated` at all (see
    /// [`Coalescer`]), so this drives a contract Codex documents rather than one
    /// this build exercises. That is worth stating on the test rather than
    /// leaving a reader to assume these lines were captured: if a future build
    /// starts streaming, *this* is the specification it will be held to, and
    /// nobody should discover only then that it was written from the docs.
    #[test]
    fn the_coalescer_emits_only_what_is_new() {
        let mut c = Coalescer::default();
        // First sight: the whole thing.
        assert_eq!(c.advance("a", "Hel"), Some("Hel".to_string()));
        // An extension: the tail only.
        assert_eq!(c.advance("a", "Hello"), Some("lo".to_string()));
        // Restated unchanged: nothing.
        assert_eq!(c.advance("a", "Hello"), None);
        // …and the accumulated text is still right after an in-place extension,
        // which is what the next tail is measured against.
        assert_eq!(c.advance("a", "Hello there"), Some(" there".to_string()));

        // **Not an extension: sent whole.** A rewritten message shares no
        // prefix, and diffing two such strings would drop the difference
        // silently. The stored text has to become the *new* one, or the next
        // update is measured against text that is no longer on screen.
        assert_eq!(c.advance("a", "Goodbye"), Some("Goodbye".to_string()));
        assert_eq!(c.advance("a", "Goodbye now"), Some(" now".to_string()));

        // Keys are independent — Codex numbers items per turn.
        assert_eq!(c.advance("b", "Hel"), Some("Hel".to_string()));
        assert_eq!(c.advance("a", "Goodbye now"), None);

        // An empty update is nothing, first sight or not.
        assert_eq!(c.advance("c", ""), None);
        assert_eq!(c.advance("c", ""), None);
        assert_eq!(c.advance("c", "x"), Some("x".to_string()));

        // Cleared, a key starts over rather than treating the old text as a
        // prefix — which is what the turn boundary relies on.
        c.clear("a");
        assert_eq!(
            c.advance("a", "Goodbye now"),
            Some("Goodbye now".to_string())
        );
    }

    #[test]
    fn every_harness_ignores_blank_and_malformed_lines() {
        for h in Harness::ALL {
            let out = drive(
                h,
                &[
                    "",
                    "   ",
                    "\t\r\n",
                    "{not json",
                    "[]",
                    "{}",
                    "null",
                    "0",
                    r#""a string""#,
                    r#"{"type":null}"#,
                    r#"{"type":123}"#,
                    r#"{"type":{"nested":"object"}}"#,
                    r#"{"type":"item.completed"}"#,
                    r#"{"type":"item.completed","item":null}"#,
                    r#"{"type":"item.completed","item":[]}"#,
                    r#"{"type":"a type nothing decodes"}"#,
                ],
            );
            assert!(out.is_empty(), "{:?} produced {:?}", h, out);
        }
    }

    /// The one deliberate exception, recorded so it is not read as a leak in the
    /// test above: an OpenCode `step_finish` whose `reason` cannot be read ends
    /// the turn. `push_opencode`'s doc argues for that direction — the printer
    /// never says a turn is over, so of the two ways to be wrong about a missing
    /// reason, this one truncates a turn only if a *tool* step ever omits it,
    /// while the other stamps an error on every ordinary turn.
    #[test]
    fn an_opencode_step_finish_with_no_readable_reason_ends_the_turn() {
        let out = drive(
            Harness::OpenCode,
            &[r#"{"type":"step_finish","part":"not an object"}"#],
        );
        assert!(
            matches!(
                &out[..],
                [StreamEvent::TurnDone {
                    is_error: false,
                    ..
                }]
            ),
            "{out:?}"
        );
    }

    /// **`trim` does not remove a byte-order mark**, because U+FEFF is not
    /// `White_Space`. A BOM on the first line — a Windows console redirect, a
    /// shim that re-encodes a pipe — therefore made `from_str` fail and the line
    /// was filed as prose. On the two dialects that carry the session id on
    /// their opening line that costs the **whole conversation's continuity**,
    /// not one event: `resume` never learns the id, so every later turn opens a
    /// fresh conversation with no memory of the last.
    #[test]
    fn a_byte_order_mark_does_not_swallow_the_session_id() {
        let line = "\u{feff}".to_string() + r#"{"type":"thread.started","thread_id":"th_1"}"#;
        let out = drive(Harness::Codex, &[&line]);
        assert!(
            matches!(&out[..], [StreamEvent::SessionStarted { id }] if id == "th_1"),
            "{out:?}"
        );
        // …and the line is not filed as a plain-text diagnostic either, which is
        // the other half: the app keeps those and shows them when a turn fails.
        let mut p = StreamParser::new(Harness::Codex);
        p.push(&line);
        assert_eq!(p.last_line(), LineKind::Json);
    }

    /// **A deeply nested line is refused, not a stack overflow.** `serde_json`
    /// caps recursion at 128 levels by default, which is the only thing standing
    /// between a hostile or corrupt line and this parser's stack — and a
    /// dependency default is not a guarantee until something asks for it. All
    /// four CLIs write their own JSON to a pipe, so reachability is low; the
    /// cost of pinning it is one test.
    #[test]
    fn a_deeply_nested_line_is_refused_rather_than_overflowing_the_stack() {
        for depth in [127usize, 200, 5_000] {
            let line = format!("{}{}", "[".repeat(depth), "]".repeat(depth));
            for h in Harness::ALL {
                let mut p = StreamParser::new(h);
                // No events either way — a JSON array is not a line any dialect
                // decodes — but the shallow one parses and the deep one does
                // not, which is the distinction being pinned.
                assert!(p.push(&line).is_empty(), "{h:?} at depth {depth}");
                let want = match depth <= 127 {
                    true => LineKind::Json,
                    false => LineKind::Plain,
                };
                assert_eq!(p.last_line(), want, "{h:?} at depth {depth}");
            }
        }
    }

    /// The three answers `last_line` exists to keep apart, so the app does not
    /// have to re-parse the line to tell them apart.
    #[test]
    fn a_line_reports_what_it_was_without_being_parsed_again() {
        let mut p = StreamParser::new(Harness::Codex);
        p.push("");
        assert_eq!(p.last_line(), LineKind::Blank);
        p.push("   ");
        assert_eq!(p.last_line(), LineKind::Blank);
        // Prose the CLI printed: an expired OAuth session, a missing model. It
        // is the only explanation there will be, so the app keeps it.
        p.push("error: could not authenticate");
        assert_eq!(p.last_line(), LineKind::Plain);
        // JSON this dialect has nothing to say about is **not** prose, and
        // filing it as such put protocol noise in the failure message.
        p.push(r#"{"type":"reasoning_summary"}"#);
        assert_eq!(p.last_line(), LineKind::Json);
        assert!(p.push(r#"{"type":"reasoning_summary"}"#).is_empty());
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
            [StreamEvent::ToolUse { name, sql, .. }] => {
                assert_eq!(name, "mcp__schemaic__run_query");
                assert_eq!(sql.as_deref(), Some("SELECT 1"));
            }
            other => panic!("{other:?}"),
        }
    }

    /// **One call, one chip, however many times the CLI restates it.**
    /// `codex_item` treats every non-`item.completed` event as "in progress", so
    /// a second `ToolUse` reached `TurnState::apply`, which pushes a segment
    /// unconditionally. The result attaches to the *last* pending chip, leaving
    /// the first spinning for the rest of the transcript.
    ///
    /// This drives `mcp_tool_call`, which is what it names. Its docstring used
    /// to reason about `command_execution` — the arm that is actually streamed
    /// with `item.updated` — while the body exercised this one, so the arm the
    /// argument was about had no test at all and shipped without the guard. That
    /// case is `codex_a_restated_shell_step_opens_one_chip_and_closes_it`.
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

    /// **The seam the per-turn reset exists for, and it needs two turns to
    /// show.** `seen_tools` is keyed by `step_index`, and Antigravity numbers
    /// its steps from zero on *each* turn — so on a persistent session the
    /// second turn's first tool call arrives with an id the set already holds,
    /// and its chip is dropped. Every other tool test here drives a single turn,
    /// which is exactly why the bug was invisible: one parser, one turn, no
    /// collision. This one feeds two turns through one parser, as a live session
    /// does.
    #[test]
    fn a_second_turn_reuses_step_ids_and_still_gets_its_chips() {
        let tool = |idx: u32, name: &str| {
            format!(
                r#"{{"event":"step_update","step_update":{{"step_index":{idx},"state":"ACTIVE","step_type":"tool","tool_name":"call_mcp_tool","tool_info":{{"name":"call_mcp_tool","parameters":{{"ServerName":"schemaic","ToolName":"{name}"}}}}}}}}"#
            )
        };
        let result = r#"{"event":"result","result":{"conversation_id":"c1","status":"SUCCESS","response":"ok\n","duration_seconds":1.0,"num_turns":1,"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}}"#;
        let mut p = StreamParser::new(Harness::Antigravity);
        let mut chips = 0usize;
        // Turn one: one call at step 0, then the turn ends.
        for l in [tool(0, "list_schema").as_str(), result] {
            chips += p
                .push(l)
                .iter()
                .filter(|e| matches!(e, StreamEvent::ToolUse { .. }))
                .count();
        }
        assert_eq!(chips, 1, "the first turn's chip went missing");
        // Turn two: a *different* call that happens to be step 0 again.
        let second = p.push(&tool(0, "describe_table"));
        assert_eq!(
            second
                .iter()
                .filter(|e| matches!(e, StreamEvent::ToolUse { .. }))
                .count(),
            1,
            "the second turn's tool ran with no chip to show for it: {second:?}"
        );
        // …and the restatement rule still holds *within* that second turn.
        let restated = p.push(&tool(0, "describe_table"));
        assert!(
            !restated
                .iter()
                .any(|e| matches!(e, StreamEvent::ToolUse { .. })),
            "a restatement inside one turn announced itself twice: {restated:?}"
        );
    }

    /// The same rule on the other dialect that restates a call, whose id is
    /// `step_index` — held *within* one turn, which is the half the reset above
    /// must not undo.
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
        // A call whose only event is `item.completed` opens its own chip before
        // filling it. It used to emit the result alone, which `TurnState::apply`
        // then attached to whichever *other* call was still pending — or dropped
        // when none was.
        match &ok[..] {
            [
                StreamEvent::ToolUse { name, .. },
                StreamEvent::ToolResult { text, is_error, .. },
            ] => {
                assert_eq!(name, "mcp__schemaic__run_query");
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
            [
                StreamEvent::ToolUse { .. },
                StreamEvent::ToolResult { text, is_error, .. },
            ] => {
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

        // **A completed-only item opens its own chip before it fills it.** It
        // used to emit a bare `ToolResult`, and `TurnState::apply` attaches a
        // loose result to the most recent tool call still awaiting one — so this
        // file change was stapled onto whatever `run_query` chip happened to be
        // open, and the user read `/etc/passwd` as the answer to their query.
        // The composition is the finding, so the assertion is the pair.
        let edit = drive(
            Harness::Codex,
            &[
                r#"{"type":"item.completed","item":{"id":"f1","type":"file_change","changes":[{"path":"/etc/passwd","kind":"update"}],"status":"completed"}}"#,
            ],
        );
        match &edit[..] {
            [
                StreamEvent::ToolUse { name, .. },
                StreamEvent::ToolResult { text, .. },
            ] => {
                assert_eq!(name, "file_change");
                assert!(text.contains("/etc/passwd"));
            }
            other => panic!("{other:?}"),
        }
    }

    /// **The guard the sibling arm had and these two did not.** Codex restates
    /// an item on every `item.updated`, so one shell command arrived as four
    /// lines and opened four chips — three of which never receive a result and
    /// spin for the rest of the turn. `mcp_tool_call` was given `first_sight` on
    /// both halves; `command_execution` and `file_change` were given neither.
    ///
    /// Driven through `TurnState` as well as the parser, because a chip that
    /// never closes is a rendering fact and the event list alone does not show
    /// it.
    #[test]
    fn codex_a_restated_shell_step_opens_one_chip_and_closes_it() {
        let out = drive(
            Harness::Codex,
            &[
                r#"{"type":"item.started","item":{"id":"c1","type":"command_execution","command":"ls","aggregated_output":"","status":"in_progress"}}"#,
                r#"{"type":"item.updated","item":{"id":"c1","type":"command_execution","command":"ls","aggregated_output":"a","status":"in_progress"}}"#,
                r#"{"type":"item.updated","item":{"id":"c1","type":"command_execution","command":"ls","aggregated_output":"a\nb","status":"in_progress"}}"#,
                r#"{"type":"item.completed","item":{"id":"c1","type":"command_execution","command":"ls","aggregated_output":"a\nb","exit_code":0,"status":"completed"}}"#,
            ],
        );
        let opens = out
            .iter()
            .filter(|e| matches!(e, StreamEvent::ToolUse { .. }))
            .count();
        let closes = out
            .iter()
            .filter(|e| matches!(e, StreamEvent::ToolResult { .. }))
            .count();
        assert_eq!(opens, 1, "one shell step opened {opens} chips: {out:?}");
        assert_eq!(closes, 1, "{out:?}");

        // The composition: every chip the turn opened is answered.
        let mut turn = crate::TurnState::default();
        for e in &out {
            turn.apply(e);
        }
        let segs = turn.segments();
        let pending = segs
            .iter()
            .filter(|s| {
                matches!(
                    s,
                    schemaic_core::transcript::Seg::Tool(t) if t.result.is_none()
                )
            })
            .count();
        assert_eq!(pending, 0, "a chip is still spinning: {segs:?}");
    }

    /// **Two id-less items are two calls, not one.** `codex_item` reads the id
    /// as `unwrap_or("")`, so every item in a turn that carries none shared the
    /// single `first_sight` key `""` (and `"\0done"`): the second `run_query`
    /// ran and left no trace at all — no chip, no SQL, no result. The sibling
    /// Antigravity arm answers the identical question the other way and says so
    /// in writing: "a step with no index is announced rather than swallowed:
    /// silently dropping a call is the worse failure of the two."
    ///
    /// Defensive rather than observed: no captured Codex turn omits `id`. The
    /// point is the direction the default fails in.
    #[test]
    fn codex_two_items_with_no_id_are_two_calls() {
        let out = drive(
            Harness::Codex,
            &[
                r#"{"type":"item.completed","item":{"type":"mcp_tool_call","server":"schemaic","tool":"run_query","arguments":{"sql":"A"},"result":"RA","status":"completed"}}"#,
                r#"{"type":"item.completed","item":{"type":"mcp_tool_call","server":"schemaic","tool":"run_query","arguments":{"sql":"B"},"result":"RB","status":"completed"}}"#,
            ],
        );
        let sqls: Vec<&str> = out
            .iter()
            .filter_map(|e| match e {
                StreamEvent::ToolUse { sql, .. } => sql.as_deref(),
                _ => None,
            })
            .collect();
        assert_eq!(sqls, ["A", "B"], "{out:?}");
        let results = out
            .iter()
            .filter(|e| matches!(e, StreamEvent::ToolResult { .. }))
            .count();
        assert_eq!(results, 2, "{out:?}");
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
                StreamEvent::ToolResult { text, is_error: false, .. } if text.contains("widgets")
            )),
            "the server's answer never reached the chip: {out:?}"
        );
        assert_eq!(text_of(&out), "orders  \nwidgets");

        // The composition, which `docs/architecture.md` claimed of all six
        // verbatim captures and was true of four. A parser that decodes
        // perfectly into a `TurnState` that renders nothing is a turn the user
        // reads as empty.
        let mut st = crate::TurnState::default();
        for ev in &out {
            st.apply(ev);
        }
        let segs = st.segments();
        let chip = segs
            .iter()
            .find_map(|s| match s {
                Seg::Tool(t) => Some(t),
                _ => None,
            })
            .expect("a tool chip");
        assert_eq!(chip.name, "mcp__schemaic__list_schema");
        assert!(!chip.is_error);
        assert!(
            chip.result
                .as_deref()
                .is_some_and(|r| r.contains("widgets")),
            "the chip is still spinning: {chip:?}"
        );
        assert!(
            segs.contains(&Seg::Text("orders  \nwidgets".to_string())),
            "{segs:?}"
        );
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
        // The refusal arrives as one `item.completed` with no `item.started`
        // before it, so the chip is opened here too — a refused call the user
        // cannot see is the failure this whole arm exists to surface.
        match &out[..] {
            [
                StreamEvent::ToolUse { name, .. },
                StreamEvent::ToolResult { text, is_error, .. },
            ] => {
                assert_eq!(name, "mcp__schemaic__list_schema");
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
    fn a_real_denied_antigravity_tool_call_names_the_tool_and_says_so_in_prose() {
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
                StreamEvent::ToolResult { text, is_error: true, .. } if text.contains("denied permission")
            )),
            "{out:?}"
        );
        // **The refusal is told in prose, not by repainting the turn.**
        // `status` says SUCCESS and the tool was refused all the same, so the
        // note is what carries it — and `is_error` stays false because a refused
        // *side* tool can sit beside an answer that is complete, and `is_error`
        // would render that whole answer as plain red text. See
        // `opencode_is_failure`.
        match out.last() {
            Some(StreamEvent::TurnDone { is_error, .. }) => assert!(
                !*is_error,
                "a denied side tool must not repaint the answer beside it"
            ),
            other => panic!("{other:?}"),
        }
        // And the user is told why, since the response body was empty.
        assert!(text_of(&out).contains("permission"), "{out:?}");

        // **Through `TurnState` as well as the parser**, which
        // `docs/architecture.md` claimed of all six verbatim captures and was
        // true of four. This is the fixture the whole "SUCCESS with a refusal"
        // argument rests on: the response body is empty, so the *only* thing the
        // user reads is the prose this parser synthesises — and a `TurnState`
        // regression that swallowed it would restore the silent success with the
        // parser test still green.
        let mut st = crate::TurnState::default();
        for ev in &out {
            st.apply(ev);
        }
        let segs = st.segments();
        let rendered: String = segs
            .iter()
            .filter_map(|s| match s {
                Seg::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            rendered.contains("permission"),
            "the turn renders as an empty success: {segs:?}"
        );
        // The chip is there, named for the database tool, and carries the
        // refusal rather than spinning.
        let chip = segs
            .iter()
            .find_map(|s| match s {
                Seg::Tool(t) => Some(t),
                _ => None,
            })
            .expect("a tool chip");
        assert_eq!(chip.name, "mcp__schemaic__list_schema");
        assert!(chip.is_error, "{chip:?}");
        assert!(
            chip.result.as_deref().is_some_and(|r| r.contains("denied")),
            "{chip:?}"
        );
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
                StreamEvent::ToolResult { text, is_error: false, .. } if text.contains("widgets")
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

    /// **A restated `DONE` resolves one chip, not the nearest open one.**
    ///
    /// The dedupe matched only on a `ToolUse` first event, so it suppressed a
    /// repeated *announcement* and let a repeated *result* through — and
    /// `TurnState::apply` attaches a loose result to the most recent tool call
    /// still awaiting one. Measured: step 0 opens, step 1 opens, step 0
    /// completes twice, and step 1 — which has not finished and may never — is
    /// shown as complete carrying step 0's output.
    ///
    /// Both Codex arms in this file already guard their result half and say
    /// why, keyed the same way; `seen_tools`' own doc states the rule for both
    /// per-turn dialects. The guard followed the shape the captured turns
    /// happen to restate rather than the rule.
    #[test]
    fn an_antigravity_step_restating_done_does_not_resolve_another_steps_chip() {
        let mut p = StreamParser::new(Harness::Antigravity);
        let mut st = crate::TurnState::default();
        for line in [
            r#"{"event":"step_update","step_update":{"step_index":0,"state":"ACTIVE","step_type":"tool","tool_name":"call_mcp_tool","tool_info":{"name":"call_mcp_tool","parameters":{"Arguments":{"sql":"S"},"ServerName":"schemaic","ToolName":"run_query"}}}}"#,
            r#"{"event":"step_update","step_update":{"step_index":1,"state":"ACTIVE","step_type":"tool","tool_name":"read_file","tool_info":{"name":"read_file","parameters":{}}}}"#,
            r#"{"event":"step_update","step_update":{"step_index":0,"state":"DONE","step_type":"tool","tool_name":"call_mcp_tool","tool_info":{"name":"call_mcp_tool","parameters":{"Arguments":{"sql":"S"},"ServerName":"schemaic","ToolName":"run_query"},"output":"OUT0"}}}"#,
            r#"{"event":"step_update","step_update":{"step_index":0,"state":"DONE","step_type":"tool","tool_name":"call_mcp_tool","tool_info":{"name":"call_mcp_tool","parameters":{"Arguments":{"sql":"S"},"ServerName":"schemaic","ToolName":"run_query"},"output":"OUT0-again"}}}"#,
        ] {
            for ev in p.push(line) {
                st.apply(&ev);
            }
        }
        let segs = st.segments();
        match &segs[..] {
            [Seg::Tool(first), Seg::Tool(second)] => {
                assert_eq!(first.result.as_deref(), Some("OUT0"));
                assert_eq!(
                    second.result, None,
                    "step 1 has not finished, and is wearing step 0's output"
                );
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
            [StreamEvent::ToolUse { name, sql, .. }] => {
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
            [StreamEvent::ToolUse { name, sql, .. }] => {
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

    // ---- OpenCode ----------------------------------------------------------

    /// **Captured verbatim** from `opencode run --pure --agent schemaic
    /// --format json`, driving a tool call and then answering. Byte-for-byte
    /// apart from the line breaks this array imposes: the ids, the per-step
    /// `tokens` objects and the `reason` values are exactly what the binary
    /// wrote.
    ///
    /// It is the fixture that carries the two facts this dialect turns on — a
    /// `step_finish` whose `reason` is `tool-calls` is *not* the end of the
    /// turn, and the one whose reason is `stop` is, because nothing resembling
    /// `turn.completed` is ever emitted.
    const OC_REAL_TOOL_CYCLE: &[&str] = &[
        r#"{"type":"step_start","timestamp":1788647017491,"sessionID":"ses_f8c547cecffep40rcGQbMKGwOv","part":{"id":"prt_073ab8c0d001tZ8IFpFLsXlWR1","messageID":"msg_073ab87250011eYtXhMHciOVKE","sessionID":"ses_f8c547cecffep40rcGQbMKGwOv","type":"step-start"}}"#,
        r#"{"type":"tool_use","timestamp":1788647017703,"sessionID":"ses_f8c547cecffep40rcGQbMKGwOv","part":{"type":"tool","tool":"glob","callID":"call_7d7b5533c49a40c6a5d4feca","state":{"status":"completed","input":{"pattern":"*.json"},"output":"only.json","metadata":{"count":1,"truncated":false},"time":{"start":1788647017603,"end":1788647017683}},"id":"prt_073ab8c62001vDU6f5IalTNarS","sessionID":"ses_f8c547cecffep40rcGQbMKGwOv","messageID":"msg_073ab87250011eYtXhMHciOVKE"}}"#,
        r#"{"type":"step_finish","timestamp":1788647017703,"sessionID":"ses_f8c547cecffep40rcGQbMKGwOv","part":{"id":"prt_073ab8cd7001Z3Tclx7O3xs4hk","reason":"tool-calls","messageID":"msg_073ab87250011eYtXhMHciOVKE","sessionID":"ses_f8c547cecffep40rcGQbMKGwOv","type":"step-finish","tokens":{"total":2536,"input":2497,"output":10,"reasoning":29,"cache":{"write":0,"read":0}},"cost":0}}"#,
        r#"{"type":"step_start","timestamp":1788647018629,"sessionID":"ses_f8c547cecffep40rcGQbMKGwOv","part":{"id":"prt_073ab907f001lV8Cg2emVjlGnQ","messageID":"msg_073ab8cdd001Gm7WtJOOb7DqA4","sessionID":"ses_f8c547cecffep40rcGQbMKGwOv","type":"step-start"}}"#,
        r#"{"type":"text","timestamp":1788647018724,"sessionID":"ses_f8c547cecffep40rcGQbMKGwOv","part":{"id":"prt_073ab90c1001ClXrpFkcoD8FeO","messageID":"msg_073ab8cdd001Gm7WtJOOb7DqA4","sessionID":"ses_f8c547cecffep40rcGQbMKGwOv","type":"text","text":"DONE","time":{"start":1788647018689,"end":1788647018716}}}"#,
        r#"{"type":"step_finish","timestamp":1788647018724,"sessionID":"ses_f8c547cecffep40rcGQbMKGwOv","part":{"id":"prt_073ab90dd001jG5FDOUzUeQQiA","reason":"stop","messageID":"msg_073ab8cdd001Gm7WtJOOb7DqA4","sessionID":"ses_f8c547cecffep40rcGQbMKGwOv","type":"step-finish","tokens":{"total":2645,"input":637,"output":2,"reasoning":22,"cache":{"write":0,"read":1984}},"cost":0}}"#,
    ];

    #[test]
    fn a_real_opencode_tool_cycle_decodes_end_to_end() {
        let out = drive(Harness::OpenCode, OC_REAL_TOOL_CYCLE);
        match &out[..] {
            [
                StreamEvent::SessionStarted { id },
                StreamEvent::ToolUse { name, .. },
                StreamEvent::ToolResult { text, is_error, .. },
                StreamEvent::TextDelta(t),
                StreamEvent::TurnDone { is_error: e2, .. },
            ] => {
                assert_eq!(id, "ses_f8c547cecffep40rcGQbMKGwOv");
                assert_eq!(name, "glob");
                assert_eq!(text, "only.json");
                assert!(!is_error);
                assert_eq!(t, "DONE");
                assert!(!e2);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_opencode_turn_ends_only_on_the_terminal_step() {
        // The whole dialect turns on this: `tool-calls` means another step is
        // coming, and closing the turn there would cut the answer off before it
        // was written. Exactly one `TurnDone`, and it is the last event.
        let out = drive(Harness::OpenCode, OC_REAL_TOOL_CYCLE);
        let dones = out
            .iter()
            .filter(|e| matches!(e, StreamEvent::TurnDone { .. }))
            .count();
        assert_eq!(dones, 1, "{out:?}");
        assert!(
            matches!(out.last(), Some(StreamEvent::TurnDone { .. })),
            "{out:?}"
        );
    }

    /// Per-*step* counts, not a turn total: the fixture's two steps report input
    /// 2497 then 637, and a footer showing only the last would tell the user the
    /// turn cost a quarter of what it did.
    ///
    /// **And every field the step bills for, which used to be two of four.** The
    /// footer summed `input` and `output` alone and dropped `reasoning` and
    /// every cached input token, so this same fixture rendered `↑3.1k ↓12` for a
    /// turn that really spent 5,181 — the input understated by 39%. The
    /// arithmetic is settled by the fixture itself and needs no claim about
    /// OpenCode's source: each step's own `total` reconciles exactly as
    /// `input + output + reasoning + cache.read`, at 2536 and at 2645. That
    /// identity is asserted here, so a step whose fields stop adding up fails
    /// rather than quietly shifting the footer.
    #[test]
    fn an_opencode_turn_sums_tokens_across_its_steps() {
        // Step one: 2497 + 10 + 29 + 0 = 2536, its reported `total`.
        assert_eq!(2497 + 10 + 29, 2536);
        // Step two: 637 + 2 + 22 + 1984 = 2645, its reported `total`.
        assert_eq!(637 + 2 + 22 + 1984, 2645);

        let out = drive(Harness::OpenCode, OC_REAL_TOOL_CYCLE);
        match out.last() {
            Some(StreamEvent::TurnDone { stats, .. }) => {
                assert_eq!(stats.input_tokens, Some(2497 + 29 + 637 + 22 + 1984));
                assert_eq!(stats.output_tokens, Some(10 + 2));
                // The two halves account for both steps' `total` between them,
                // which is the property the footer is claiming to show.
                assert_eq!(
                    stats.input_tokens.unwrap_or(0) + stats.output_tokens.unwrap_or(0),
                    2536 + 2645
                );
                // Wall clock across the turn, from the event timestamps.
                assert_eq!(stats.duration_ms, Some(1788647018724 - 1788647017491));
            }
            other => panic!("{other:?}"),
        }
    }

    /// **A truncated answer is not a clean success, and it is not an error
    /// either.** `reason: "length"` means the model hit its output cap
    /// mid-sentence and `"content-filter"` means the rest was withheld; both
    /// used to close the turn with `is_error: false` and nothing on screen to
    /// say the last sentence was not the end of one, which is what the note
    /// fixed.
    ///
    /// Flagging them `is_error` as well went too far: that is the *rendering*
    /// switch, and it repainted the whole answer — headings and tables as raw
    /// `#`/`|`, code blocks stripped of their action bar, and the advisory
    /// sentence itself showing its literal `_underscores_`. The note says it;
    /// `is_error` is for a turn with nothing to show.
    #[test]
    fn an_opencode_turn_cut_off_by_the_token_cap_says_so_without_repainting_it() {
        for (reason, must_mention) in [("length", "output limit"), ("content-filter", "withheld")] {
            let line = format!(
                r#"{{"type":"step_finish","timestamp":1,"sessionID":"s1","part":{{"reason":"{reason}","tokens":{{"input":5,"output":5}}}}}}"#
            );
            let out = drive(Harness::OpenCode, &[&line]);
            assert!(
                out.iter().any(|e| matches!(
                    e,
                    StreamEvent::TextDelta(t) if t.contains(must_mention)
                )),
                "nothing told the user the answer was cut off: {out:?}"
            );
            assert!(
                matches!(
                    out.last(),
                    Some(StreamEvent::TurnDone {
                        is_error: false,
                        ..
                    })
                ),
                "{reason} repainted an answer that is real prose: {out:?}"
            );
        }
        // …and an ordinary end is still an ordinary end, in both spellings.
        for reason in [Some("stop"), None] {
            assert!(!opencode_is_failure(reason));
            assert_eq!(opencode_cutoff_note(reason), None);
        }
        // An unrecognised reason must not stamp an error on a working turn —
        // the same reasoning `push_opencode`'s doc gives for reading an absent
        // reason as terminal.
        assert!(!opencode_is_failure(Some("other")));
        // The one that really has nothing to show.
        assert!(opencode_is_failure(Some("error")));
        assert_eq!(
            opencode_cutoff_note(Some("error")),
            None,
            "an error turn's own message is what it says; there is no answer to \
             annotate"
        );
    }

    /// **The composition, across all three links.** The decoder's `is_error`,
    /// `Role::settled` and `Role::carries_an_answer` lived in three crates with
    /// nothing joining them, and each was defensible alone —
    /// `opencode_is_failure`'s own doc argued correctly that a cut-off turn is
    /// not a clean one. Together they turned a complete, formatted reply into
    /// red monochrome plain text: headings and tables as raw `#`/`|`, fenced SQL
    /// stripped of its Insert / Run / Propose bar, and the advisory sentence
    /// about the cut-off rendering its own literal `_underscores_`.
    #[test]
    fn an_answer_cut_off_at_the_output_cap_still_renders_as_markdown() {
        for reason in ["length", "content-filter"] {
            let line = format!(
                r#"{{"type":"step_finish","timestamp":1,"sessionID":"s1","part":{{"reason":"{reason}","tokens":{{"input":5,"output":5}}}}}}"#
            );
            let out = drive(Harness::OpenCode, &[&line]);
            let is_error = out
                .iter()
                .find_map(|e| match e {
                    StreamEvent::TurnDone { is_error, .. } => Some(*is_error),
                    _ => None,
                })
                .expect("the step closes the turn");
            assert!(
                schemaic_core::transcript::Role::Assistant
                    .settled(is_error)
                    .carries_an_answer(),
                "{reason}: the answer is real prose and would lose its formatting"
            );
            // And the sentence that says so is in the prose, where it will be
            // rendered as the markdown it is written in.
            assert!(text_of(&out).contains('_'), "{out:?}");
        }
    }

    /// And a turn that really has nothing to show still goes the other way —
    /// without this the fix would pass by never marking anything an error.
    #[test]
    fn a_turn_with_nothing_to_show_is_still_rendered_as_an_error() {
        use schemaic_core::transcript::Role;
        let out = drive(
            Harness::OpenCode,
            &[r#"{"type":"step_finish","timestamp":1,"sessionID":"s1","part":{"reason":"error"}}"#],
        );
        let is_error = out
            .iter()
            .find_map(|e| match e {
                StreamEvent::TurnDone { is_error, .. } => Some(*is_error),
                _ => None,
            })
            .expect("the step closes the turn");
        assert!(!Role::Assistant.settled(is_error).carries_an_answer());
        // A turn already settled as an error is not un-settled by a later
        // snapshot carrying `is_error: false`.
        assert_eq!(Role::Error.settled(false), Role::Error);
    }

    #[test]
    fn a_real_opencode_turn_renders_as_one_text_segment() {
        // The composition, not the parser alone — the seam the fix campaign
        // found thirteen tests missing.
        let mut p = StreamParser::new(Harness::OpenCode);
        let mut st = crate::TurnState::default();
        for line in OC_REAL_TOOL_CYCLE {
            for ev in p.push(line) {
                st.apply(&ev);
            }
        }
        let segs = st.segments();
        assert!(segs.contains(&Seg::Text("DONE".to_string())), "{segs:?}");
    }

    #[test]
    fn an_opencode_session_error_ends_the_turn_and_says_why() {
        // `session.error` is the printer's only failure event, and nothing
        // follows it — so a `TurnDone` has to come from here or the panel spins
        // until the process exits and blames the exit status instead.
        let out = drive(
            Harness::OpenCode,
            &[
                r#"{"type":"error","error":{"name":"ProviderAuthError","data":{"message":"no credentials"}}}"#,
            ],
        );
        assert!(text_of(&out).contains("no credentials"), "{out:?}");
        assert!(
            matches!(
                out.last(),
                Some(StreamEvent::TurnDone { is_error: true, .. })
            ),
            "{out:?}"
        );
    }

    #[test]
    fn an_opencode_error_falls_back_to_its_name() {
        // `data.message` is optional in the printer; `name` is not.
        let out = drive(
            Harness::OpenCode,
            &[r#"{"type":"error","error":{"name":"UnknownError"}}"#],
        );
        assert!(text_of(&out).contains("UnknownError"), "{out:?}");
    }

    #[test]
    fn an_opencode_tool_still_pending_opens_a_chip_without_closing_it() {
        // Every measured call arrived already `completed`, but the state machine
        // has a `running` status and a chip that fills from a result we never
        // received would be wrong in the other direction.
        let out = drive(
            Harness::OpenCode,
            &[
                r#"{"type":"tool_use","timestamp":1,"sessionID":"s","part":{"type":"tool","tool":"grep","callID":"c1","state":{"status":"running","input":{}}}}"#,
            ],
        );
        // The id leads, as it does on every OpenCode event; what matters is that
        // the chip opens and nothing closes it.
        assert!(
            matches!(
                &out[..],
                [
                    StreamEvent::SessionStarted { .. },
                    StreamEvent::ToolUse { .. }
                ]
            ),
            "{out:?}"
        );
    }

    #[test]
    fn an_opencode_restated_tool_call_announces_once() {
        // Same guard the other two per-turn dialects need, keyed on `callID`.
        let running = r#"{"type":"tool_use","timestamp":1,"sessionID":"s","part":{"type":"tool","tool":"grep","callID":"c1","state":{"status":"running","input":{}}}}"#;
        let done = r#"{"type":"tool_use","timestamp":2,"sessionID":"s","part":{"type":"tool","tool":"grep","callID":"c1","state":{"status":"completed","output":"hits"}}}"#;
        let out = drive(Harness::OpenCode, &[running, done]);
        let uses = out
            .iter()
            .filter(|e| matches!(e, StreamEvent::ToolUse { .. }))
            .count();
        assert_eq!(uses, 1, "{out:?}");
        assert!(
            out.iter()
                .any(|e| matches!(e, StreamEvent::ToolResult { text, .. } if text == "hits")),
            "{out:?}"
        );
    }

    #[test]
    fn an_opencode_mcp_tool_keeps_the_qualified_name_the_allowlist_speaks() {
        // Our own server's tools arrive prefixed by OpenCode itself; the chip
        // and the allow-list both speak `mcp__schemaic__run_query`.
        let out = drive(
            Harness::OpenCode,
            &[
                r#"{"type":"tool_use","timestamp":1,"sessionID":"s","part":{"type":"tool","tool":"schemaic_run_query","callID":"c9","state":{"status":"completed","input":{"sql":"select 1"},"output":"1"}}}"#,
            ],
        );
        match out
            .iter()
            .find(|e| matches!(e, StreamEvent::ToolUse { .. }))
        {
            Some(StreamEvent::ToolUse { name, sql, .. }) => {
                assert_eq!(name, "mcp__schemaic__run_query");
                assert_eq!(sql.as_deref(), Some("select 1"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_opencode_event_is_keyed_on_type_and_ignores_antigravity_shapes() {
        // The mirror of the Antigravity trap above: a line tagged the *other*
        // dialect's way must decode to nothing rather than being half-read.
        let out = drive(
            Harness::OpenCode,
            &[r#"{"event":"result","result":{"status":"SUCCESS"}}"#],
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

#[cfg(test)]
mod copilot_tests {
    use super::*;

    fn drive(lines: &[&str]) -> Vec<StreamEvent> {
        let mut p = StreamParser::new(Harness::Copilot);
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

    /// A real turn — GitHub Copilot CLI 1.0.88, `--output-format json`, told to
    /// call `list_schema` and then `run_query` — with the fields nothing reads
    /// (`id`, `parentId`, `timestamp`, `interactionId`, `apiCallId`, the tool
    /// schemas) cut, and every line's shape otherwise as it was written. The
    /// `session.*` lines and the three `assistant.turn_end`s are kept because
    /// the decoder has to walk past them: the first `turn_end` arrives before
    /// any answer exists.
    const COPILOT_REAL_TURN: &[&str] = &[
        r#"{"type":"session.mcp_server_status_changed","data":{"serverName":"schemaic","status":"connected"},"ephemeral":true}"#,
        r#"{"type":"session.info","data":{"infoType":"configuration","message":"Disabled tools: create, edit, glob, grep, powershell, view, web_fetch"},"ephemeral":true}"#,
        r#"{"type":"session.mcp_servers_loaded","data":{"servers":[{"name":"github-mcp-server","status":"disabled","source":"builtin"},{"name":"schemaic","status":"connected"}]},"ephemeral":true}"#,
        r#"{"type":"user.message","data":{"content":"Call list_schema, then run_query with sql 'select 1'.","turnId":"0"}}"#,
        r#"{"type":"assistant.turn_start","data":{"turnId":"0"}}"#,
        r#"{"type":"assistant.tool_call_delta","data":{"toolCallId":"call_A","toolName":"schemaic-list_schema","toolType":"function","inputDelta":"{}"},"ephemeral":true}"#,
        r#"{"type":"assistant.message","data":{"messageId":"m0","content":"","toolRequests":[{"toolCallId":"call_A","name":"schemaic-list_schema","arguments":{},"type":"function","mcpServerName":"schemaic","mcpToolName":"list_schema"}],"turnId":"0","phase":"final_answer"}}"#,
        r#"{"type":"tool.execution_start","data":{"toolCallId":"call_A","toolName":"schemaic-list_schema","arguments":{},"turnId":"0","mcpServerName":"schemaic","mcpToolName":"list_schema"}}"#,
        r#"{"type":"tool.execution_complete","data":{"toolCallId":"call_A","turnId":"0","success":true,"result":{"content":"tables: customers(id, name)","detailedContent":"tables: customers(id, name)","contents":[{"type":"text","text":"tables: customers(id, name)"}]}}}"#,
        r#"{"type":"assistant.turn_end","data":{"turnId":"0"}}"#,
        r#"{"type":"assistant.turn_start","data":{"turnId":"1"}}"#,
        r#"{"type":"assistant.message","data":{"messageId":"m1","content":"","toolRequests":[{"toolCallId":"call_B","name":"schemaic-run_query","arguments":{"sql":"select 1"},"mcpServerName":"schemaic","mcpToolName":"run_query"}],"turnId":"1"}}"#,
        r#"{"type":"tool.execution_start","data":{"toolCallId":"call_B","toolName":"schemaic-run_query","arguments":{"sql":"select 1"},"turnId":"1","mcpServerName":"schemaic","mcpToolName":"run_query"}}"#,
        r#"{"type":"tool.execution_complete","data":{"toolCallId":"call_B","turnId":"1","success":true,"result":{"content":"id | name\n1 | ZEBRA","contents":[{"type":"text","text":"id | name\n1 | ZEBRA"}]}}}"#,
        r#"{"type":"assistant.turn_end","data":{"turnId":"1"}}"#,
        r#"{"type":"assistant.turn_start","data":{"turnId":"2"}}"#,
        r#"{"type":"assistant.message_start","data":{"messageId":"m2","phase":"final_answer"},"ephemeral":true}"#,
        r#"{"type":"assistant.message_delta","data":{"messageId":"m2","deltaContent":"Both "},"ephemeral":true}"#,
        r#"{"type":"assistant.message_delta","data":{"messageId":"m2","deltaContent":"returned."},"ephemeral":true}"#,
        r#"{"type":"assistant.message","data":{"messageId":"m2","content":"Both returned.","toolRequests":[],"turnId":"2","phase":"final_answer"}}"#,
        r#"{"type":"assistant.turn_end","data":{"turnId":"2"}}"#,
        r#"{"type":"session.usage_checkpoint","data":{"totalPremiumRequests":1},"ephemeral":true}"#,
        r#"{"type":"assistant.idle","data":{},"ephemeral":true}"#,
        r#"{"type":"result","timestamp":"2026-09-26T00:07:09.000Z","sessionId":"04765e2f-d530-4495-847b-c339fd860b4a","exitCode":0,"usage":{"premiumRequests":1,"totalApiDurationMs":929,"sessionDurationMs":1992}}"#,
    ];

    fn uses(out: &[StreamEvent]) -> Vec<(String, Option<String>, Option<String>)> {
        out.iter()
            .filter_map(|e| match e {
                StreamEvent::ToolUse { name, sql, id } => {
                    Some((name.clone(), sql.clone(), id.clone()))
                }
                _ => None,
            })
            .collect()
    }

    fn results(out: &[StreamEvent]) -> Vec<(String, bool, Option<String>)> {
        out.iter()
            .filter_map(|e| match e {
                StreamEvent::ToolResult { text, is_error, id } => {
                    Some((text.clone(), *is_error, id.clone()))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_real_copilot_tool_cycle_fills_two_chips_and_answers() {
        let out = drive(COPILOT_REAL_TURN);
        assert_eq!(
            uses(&out),
            vec![
                (
                    "mcp__schemaic__list_schema".to_string(),
                    None,
                    Some("call_A".to_string())
                ),
                (
                    "mcp__schemaic__run_query".to_string(),
                    Some("select 1".to_string()),
                    Some("call_B".to_string())
                ),
            ]
        );
        let r = results(&out);
        assert_eq!(r.len(), 2, "{r:?}");
        assert_eq!(
            r[0],
            (
                "tables: customers(id, name)".to_string(),
                false,
                Some("call_A".to_string())
            )
        );
        assert_eq!(r[1].0, "id | name\n1 | ZEBRA");
        // The answer once — the deltas, and the restating `assistant.message`
        // adding nothing to them.
        assert_eq!(text_of(&out), "Both returned.");
    }

    /// **The composition**, which the other dialects' captures are held to: a
    /// parser that decodes perfectly into a `TurnState` that renders nothing is a
    /// turn the user reads as empty. Two chips, each resolved with its own
    /// query's answer — not the second's answer stapled to the first — and the
    /// prose once, after them.
    #[test]
    fn a_real_copilot_turn_renders_two_filled_chips_then_the_answer() {
        let mut st = crate::TurnState::default();
        for ev in &drive(COPILOT_REAL_TURN) {
            st.apply(ev);
        }
        let segs = st.segments();
        let chips: Vec<_> = segs
            .iter()
            .filter_map(|s| match s {
                schemaic_core::transcript::Seg::Tool(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(chips.len(), 2, "{segs:?}");
        assert_eq!(chips[0].name, "mcp__schemaic__list_schema");
        assert_eq!(
            chips[0].result.as_deref(),
            Some("tables: customers(id, name)")
        );
        assert_eq!(chips[1].name, "mcp__schemaic__run_query");
        assert_eq!(chips[1].result.as_deref(), Some("id | name\n1 | ZEBRA"));
        assert!(chips.iter().all(|c| !c.is_error), "{chips:?}");
        assert_eq!(
            segs.last(),
            Some(&schemaic_core::transcript::Seg::Text(
                "Both returned.".to_string()
            )),
            "{segs:?}"
        );
    }

    /// **`assistant.turn_end` is not the end of the turn.** It closes one model
    /// call; the measured turn emitted three. Ending on the first would close
    /// the panel's turn before either query ran, and every event after it would
    /// land in a turn the app considers finished.
    #[test]
    fn only_result_ends_a_copilot_turn() {
        let mut p = StreamParser::new(Harness::Copilot);
        let mut done_at = Vec::new();
        for (i, l) in COPILOT_REAL_TURN.iter().enumerate() {
            if p.push(l)
                .iter()
                .any(|e| matches!(e, StreamEvent::TurnDone { .. }))
            {
                done_at.push(i);
            }
        }
        assert_eq!(done_at, vec![COPILOT_REAL_TURN.len() - 1]);
    }

    /// The id arrives on the last line, and the app keeps it only if it sees it
    /// before the turn closes — `ai.rs` scans a batch for `SessionStarted` and
    /// then hands the batch to a pump that stops at `TurnDone`.
    #[test]
    fn the_session_id_is_emitted_ahead_of_the_turn_closing() {
        let out = drive(COPILOT_REAL_TURN);
        let started = out
            .iter()
            .position(|e| {
                matches!(e, StreamEvent::SessionStarted { id }
                    if id == "04765e2f-d530-4495-847b-c339fd860b4a")
            })
            .expect("the session id");
        let done = out
            .iter()
            .position(|e| matches!(e, StreamEvent::TurnDone { .. }))
            .expect("the turn closes");
        assert!(started < done, "{out:?}");
    }

    /// **No numbers rather than per-session ones.** `usage` is cumulative over
    /// the session — a resumed second turn reported 6,500 ms and two premium
    /// requests against the first's 1,992 and one — so a footer reading it
    /// would grow with every question.
    #[test]
    fn a_copilot_turn_reports_no_session_wide_numbers_as_its_own() {
        let out = drive(COPILOT_REAL_TURN);
        let (stats, is_error) = out
            .iter()
            .find_map(|e| match e {
                StreamEvent::TurnDone { stats, is_error } => Some((*stats, *is_error)),
                _ => None,
            })
            .expect("TurnDone");
        assert!(!is_error);
        assert_eq!(stats.duration_ms, None);
        assert_eq!(stats.input_tokens, None);
        assert_eq!(stats.output_tokens, None);
    }

    #[test]
    fn a_nonzero_exit_code_is_a_failed_turn() {
        let out = drive(&[r#"{"type":"result","sessionId":"s","exitCode":1}"#]);
        assert!(
            out.iter()
                .any(|e| matches!(e, StreamEvent::TurnDone { is_error: true, .. })),
            "{out:?}"
        );
    }

    /// **Streaming off must not mean silence.** The restatement carries the
    /// whole message; with no deltas before it, it is the only copy there is.
    #[test]
    fn a_message_with_no_deltas_is_printed_from_its_restatement() {
        let out = drive(&[
            r#"{"type":"assistant.message","data":{"messageId":"m9","content":"Just this.","toolRequests":[]}}"#,
        ]);
        assert_eq!(text_of(&out), "Just this.");
    }

    /// And a restatement that carries more than the deltas did prints only the
    /// part never sent — not the whole again.
    #[test]
    fn a_restatement_longer_than_its_deltas_adds_only_the_tail() {
        let out = drive(&[
            r#"{"type":"assistant.message_delta","data":{"messageId":"m","deltaContent":"Hel"}}"#,
            r#"{"type":"assistant.message","data":{"messageId":"m","content":"Hello"}}"#,
        ]);
        assert_eq!(text_of(&out), "Hello");
    }

    /// **A restatement that is not an extension of its deltas is a copy of
    /// text already printed**, not a rewrite. `advance`'s "rewritten" arm is
    /// Codex's, whose restatements are cumulative; here it sent the whole
    /// message again, so a trimmed newline printed the answer twice.
    #[test]
    fn a_restatement_shorter_than_its_deltas_adds_nothing() {
        let out = drive(&[
            r#"{"type":"assistant.message_delta","data":{"messageId":"m","deltaContent":"Done.\n"}}"#,
            r#"{"type":"assistant.message","data":{"messageId":"m","content":"Done."}}"#,
        ]);
        assert_eq!(text_of(&out), "Done.\n");
    }

    #[test]
    fn a_failed_tool_call_is_flagged_on_its_chip() {
        let out = drive(&[
            r#"{"type":"tool.execution_start","data":{"toolCallId":"c","toolName":"schemaic-run_query","arguments":{"sql":"select x"},"mcpServerName":"schemaic","mcpToolName":"run_query"}}"#,
            r#"{"type":"tool.execution_complete","data":{"toolCallId":"c","success":false,"error":{"message":"Unknown column 'x'"}}}"#,
        ]);
        assert_eq!(
            results(&out),
            vec![(
                "Unknown column 'x'".to_string(),
                true,
                Some("c".to_string())
            )]
        );
    }

    /// **A completion with no start fills its own chip, not another's.** With
    /// one call still running, a bare completion for a second id used to emit a
    /// loose result, which `TurnState` attaches to the running call's chip.
    #[test]
    fn a_completion_nothing_announced_does_not_resolve_another_call() {
        let mut st = crate::TurnState::default();
        for ev in &drive(&[
            r#"{"type":"tool.execution_start","data":{"toolCallId":"a","toolName":"schemaic-run_query","arguments":{"sql":"select 1"},"mcpServerName":"schemaic","mcpToolName":"run_query"}}"#,
            r#"{"type":"tool.execution_complete","data":{"toolCallId":"b","success":true,"result":{"content":"b's answer"}}}"#,
        ]) {
            st.apply(ev);
        }
        let chips: Vec<_> = st
            .segments()
            .iter()
            .filter_map(|s| match s {
                schemaic_core::transcript::Seg::Tool(t) => Some(t.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(chips.len(), 2, "{chips:?}");
        assert_eq!(chips[0].name, "mcp__schemaic__run_query");
        assert_eq!(chips[0].result, None, "a's chip took b's result");
        assert_eq!(chips[1].result.as_deref(), Some("b's answer"));
    }

    /// A restated start or completion is one chip, resolved once — the rule
    /// `seen_tools` states for every per-turn dialect.
    #[test]
    fn a_restated_copilot_tool_call_announces_and_resolves_once() {
        let start = r#"{"type":"tool.execution_start","data":{"toolCallId":"c","toolName":"schemaic-list_schema","mcpServerName":"schemaic","mcpToolName":"list_schema"}}"#;
        let done = r#"{"type":"tool.execution_complete","data":{"toolCallId":"c","success":true,"result":{"content":"t"}}}"#;
        let out = drive(&[start, start, done, done]);
        assert_eq!((uses(&out).len(), results(&out).len()), (1, 1), "{out:?}");
    }

    /// A built-in has no server fields and keeps the name it really has — it
    /// should never run under the seal, and if it does the user sees it as
    /// what it is rather than dressed as a database call.
    #[test]
    fn a_builtin_tool_is_reported_under_its_own_name() {
        let out = drive(&[
            r#"{"type":"tool.execution_start","data":{"toolCallId":"c","toolName":"powershell","arguments":{"command":"dir"}}}"#,
        ]);
        assert_eq!(uses(&out)[0].0, "powershell");
    }
}

#[cfg(test)]
mod cursor_tests {
    use super::*;
    use schemaic_core::transcript::Seg;

    fn drive(lines: &[&str]) -> Vec<StreamEvent> {
        let mut p = StreamParser::new(Harness::Cursor);
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

    /// A real turn — Cursor CLI 2026.09.23, `-p --output-format stream-json
    /// --stream-partial-output`, against Schemaic's own `--mcp-serve` over a
    /// SQLite endpoint — **trimmed**: most of the one-token deltas are merged
    /// into three per segment, the `getMcpTools` result is cut to its opening,
    /// and `hookAdditionalContexts`, `startedAtMs`, `request_id` and the
    /// session id on every line after the first are dropped. What is kept is
    /// every field the decoder reads or tells shapes apart by, on every line:
    /// in particular the pre-tool restatement's `timestamp_ms` *and*
    /// `model_call_id`, and the final restatement's lack of both.
    const CURSOR_REAL_TURN: &[&str] = &[
        r#"{"type":"system","subtype":"init","apiKeySource":"login","cwd":"C:\\cfg\\cursor\\pid-1","session_id":"470de677-c5df-45ab-9d87-190a6fb230f3","model":"Auto","permissionMode":"default"}"#,
        r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"What is the name of the customer with id 1?"}]}}"#,
        r#"{"type":"thinking","subtype":"delta","text":"Looking up","timestamp_ms":1790388081000}"#,
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"I'll look up"}]},"timestamp_ms":1790388082000}"#,
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":" the customer"}]},"timestamp_ms":1790388082010}"#,
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":" with id 1."}]},"timestamp_ms":1790388082020}"#,
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"I'll look up the customer with id 1."}]},"model_call_id":"92b929ea-0-kjrn","timestamp_ms":1790388082030}"#,
        r#"{"type":"tool_call","subtype":"started","call_id":"call-0\nfc_0","tool_call":{"getMcpToolsToolCall":{"args":{"server":"schemaic"}}},"model_call_id":"92b929ea-0-kjrn","timestamp_ms":1790388082305}"#,
        r#"{"type":"tool_call","subtype":"completed","call_id":"call-0\nfc_0","tool_call":{"getMcpToolsToolCall":{"args":{"server":"schemaic"},"result":{"success":{"content":"{\"mode\":\"namespace\",\"namespace\":\"schemaic\"}"}}}},"model_call_id":"92b929ea-0-kjrn","timestamp_ms":1790388082400}"#,
        r#"{"type":"tool_call","subtype":"started","call_id":"call-1\nfc_1","tool_call":{"mcpToolCall":{"args":{"name":"schemaic-run_query","args":{"sql":"SELECT name FROM customers WHERE id = 1"},"providerIdentifier":"schemaic","toolName":"run_query","smartModeApprovalOnly":false,"skipApproval":false,"serverIdentifier":"schemaic"},"description":"Look up the name of customer id 1"}},"model_call_id":"92b929ea-1-8ah8","timestamp_ms":1790388083914}"#,
        r#"{"type":"tool_call","subtype":"completed","call_id":"call-1\nfc_1","tool_call":{"mcpToolCall":{"args":{"name":"schemaic-run_query","args":{"sql":"SELECT name FROM customers WHERE id = 1"},"providerIdentifier":"schemaic","toolName":"run_query","serverIdentifier":"schemaic"},"result":{"success":{"content":[{"text":{"text":"| name |\n| --- |\n| ZEBRA-9 |\n\n(1 rows)"}}],"isError":false}},"description":"Look up the name of customer id 1"}},"model_call_id":"92b929ea-1-8ah8","timestamp_ms":1790388083966}"#,
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"The customer"}]},"timestamp_ms":1790388084100}"#,
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":" with id 1 is named"}]},"timestamp_ms":1790388084110}"#,
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":" **ZEBRA-9**."}]},"timestamp_ms":1790388084120}"#,
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"The customer with id 1 is named **ZEBRA-9**."}]}}"#,
        r#"{"type":"result","subtype":"success","duration_ms":6413,"duration_api_ms":6413,"is_error":false,"result":"I'll look up the customer with id 1.The customer with id 1 is named **ZEBRA-9**.","usage":{"inputTokens":9747,"outputTokens":215,"cacheReadTokens":26240,"cacheWriteTokens":0}}"#,
    ];

    /// **Each segment once**, though each is written twice: deltas, then a
    /// restatement — the pre-tool one timestamped like a delta. Printing by
    /// "has `timestamp_ms`" alone doubles the first sentence.
    #[test]
    fn a_real_cursor_turn_prints_each_segment_once() {
        let out = drive(CURSOR_REAL_TURN);
        assert_eq!(
            text_of(&out),
            "I'll look up the customer with id 1.The customer with id 1 is named **ZEBRA-9**."
        );
    }

    /// A final restatement that trims what the deltas sent is the same text,
    /// already on screen — not a second copy to print.
    #[test]
    fn a_trimmed_final_restatement_is_not_printed_again() {
        let out = drive(&[
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Done."}]},"timestamp_ms":1}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"\n"}]},"timestamp_ms":2}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Done."}]}}"#,
        ]);
        assert_eq!(text_of(&out), "Done.\n");
    }

    /// `serde_json`'s map here is a `BTreeMap`, so "the first key" is the
    /// alphabetically first — and any sibling sorting before `success` made a
    /// successful call a refused chip named after that sibling.
    #[test]
    fn a_success_beside_another_key_is_still_a_success() {
        let r = serde_json::json!({"success": {"content": "x"}, "isBackground": false});
        assert_eq!(cursor_result(Some(&r)), ("x".to_string(), false));
    }

    /// The composition: one chip — the catalogue lookup is not one — filled
    /// with the server's answer, between the two segments of prose.
    #[test]
    fn a_real_cursor_turn_renders_prose_a_filled_chip_and_prose() {
        let mut st = crate::TurnState::default();
        for ev in &drive(CURSOR_REAL_TURN) {
            st.apply(ev);
        }
        let segs = st.segments();
        let chips: Vec<_> = segs
            .iter()
            .filter_map(|s| match s {
                Seg::Tool(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(chips.len(), 1, "{segs:?}");
        assert_eq!(chips[0].name, "mcp__schemaic__run_query");
        assert_eq!(
            chips[0].sql.as_deref(),
            Some("SELECT name FROM customers WHERE id = 1")
        );
        assert_eq!(
            chips[0].result.as_deref(),
            Some("| name |\n| --- |\n| ZEBRA-9 |\n\n(1 rows)")
        );
        assert!(!chips[0].is_error);
        assert_eq!(
            segs.first(),
            Some(&Seg::Text(
                "I'll look up the customer with id 1.".to_string()
            ))
        );
        assert_eq!(
            segs.last(),
            Some(&Seg::Text(
                "The customer with id 1 is named **ZEBRA-9**.".to_string()
            ))
        );
    }

    #[test]
    fn the_session_id_arrives_first_and_the_turn_ends_on_result() {
        let mut p = StreamParser::new(Harness::Cursor);
        let first = p.push(CURSOR_REAL_TURN[0]);
        assert!(matches!(
            first.as_slice(),
            [StreamEvent::SessionStarted { id }] if id == "470de677-c5df-45ab-9d87-190a6fb230f3"
        ));
        let mut done_at = Vec::new();
        for (i, l) in CURSOR_REAL_TURN.iter().enumerate().skip(1) {
            if p.push(l)
                .iter()
                .any(|e| matches!(e, StreamEvent::TurnDone { .. }))
            {
                done_at.push(i);
            }
        }
        assert_eq!(done_at, vec![CURSOR_REAL_TURN.len() - 1]);
    }

    /// Cached input is input: the prompt is `inputTokens` plus
    /// `cacheReadTokens`, and `duration_ms` is this turn's own (a resumed turn
    /// reported less than the first, so it is not cumulative).
    #[test]
    fn a_cursor_turn_counts_its_cached_input() {
        let out = drive(CURSOR_REAL_TURN);
        let stats = out
            .iter()
            .find_map(|e| match e {
                StreamEvent::TurnDone { stats, is_error } => {
                    assert!(!is_error);
                    Some(*stats)
                }
                _ => None,
            })
            .expect("TurnDone");
        assert_eq!(stats.duration_ms, Some(6413));
        assert_eq!(stats.input_tokens, Some(9747 + 26240));
        assert_eq!(stats.output_tokens, Some(215));
    }

    /// Without `--stream-partial-output` there are no deltas, and the
    /// restatement is the only copy of the text there is.
    #[test]
    fn a_segment_with_no_deltas_is_printed_from_its_restatement() {
        let out = drive(&[
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Just this."}]}}"#,
        ]);
        assert_eq!(text_of(&out), "Just this.");
    }

    /// Refusals are shaped as keys, and each is a refused chip carrying its
    /// reason — the measured `rejected` (no allow rule) and `permissionDenied`
    /// (a deny rule, whose completion carries no `args`).
    #[test]
    fn a_refused_call_is_a_refused_chip_with_its_reason() {
        let out = drive(&[
            r#"{"type":"tool_call","subtype":"completed","call_id":"a","tool_call":{"mcpToolCall":{"result":{"rejected":{"reason":"User rejected MCP: schemaic-run_query","isReadonly":false}}}}}"#,
            r#"{"type":"tool_call","subtype":"completed","call_id":"b","tool_call":{"shellToolCall":{"result":{"permissionDenied":{"command":"ls","error":"Command blocked by permissions configuration","isReadonly":false}}}}}"#,
        ]);
        let results: Vec<_> = out
            .iter()
            .filter_map(|e| match e {
                StreamEvent::ToolResult { text, is_error, .. } => Some((text.clone(), *is_error)),
                _ => None,
            })
            .collect();
        assert_eq!(
            results,
            vec![
                ("User rejected MCP: schemaic-run_query".to_string(), true),
                (
                    "Command blocked by permissions configuration".to_string(),
                    true
                ),
            ]
        );
        // A completion with no start still opens its chip, and names what it
        // can: the built-in by its kind, the arg-less MCP call as `mcp`.
        let names: Vec<_> = out
            .iter()
            .filter_map(|e| match e {
                StreamEvent::ToolUse { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(names, vec!["mcp", "shell"]);
    }

    /// A reader runs unprompted on this harness — measured — so it is shown
    /// under its own name rather than dropped with the catalogue lookup.
    #[test]
    fn a_builtin_reader_is_shown_and_the_catalogue_lookup_is_not() {
        let out = drive(&[
            r#"{"type":"tool_call","subtype":"started","call_id":"g","tool_call":{"getMcpToolsToolCall":{"args":{}}}}"#,
            r#"{"type":"tool_call","subtype":"started","call_id":"r","tool_call":{"grepToolCall":{"args":{"pattern":"."}}}}"#,
            r#"{"type":"tool_call","subtype":"completed","call_id":"r","tool_call":{"grepToolCall":{"args":{"pattern":"."},"result":{"success":{"content":"x"}}}}}"#,
        ]);
        let names: Vec<_> = out
            .iter()
            .filter_map(|e| match e {
                StreamEvent::ToolUse { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(names, vec!["grep"]);
    }

    #[test]
    fn a_result_that_is_not_success_fails_the_turn() {
        for line in [
            r#"{"type":"result","subtype":"error","is_error":true}"#,
            r#"{"type":"result","subtype":"error"}"#,
        ] {
            assert!(
                drive(&[line])
                    .iter()
                    .any(|e| matches!(e, StreamEvent::TurnDone { is_error: true, .. })),
                "{line}"
            );
        }
    }
}
