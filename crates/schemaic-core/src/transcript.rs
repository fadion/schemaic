//! Rendered shape of one assistant turn in the AI panel.
//!
//! The AI crate accumulates a `claude` stream into these segments; the UI
//! renders them (prose as markdown, tool calls as chips) and shows the
//! per-turn [`TurnStats`] footer. Keeping the type here lets both crates share
//! it without the UI depending on the CLI-integration crate.
//!
//! These types are serializable so a conversation can outlive the process —
//! see [`crate::chat`], which persists them per connection.

use serde::{Deserialize, Serialize};

/// Who authored a chat message in the AI panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    User,
    Assistant,
    Error,
}

/// One message in the AI panel conversation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    /// The user's text (user messages only).
    pub text: String,
    /// The assistant turn's rendered segments (assistant/error messages).
    #[serde(default)]
    pub segs: Vec<Seg>,
    /// Cost/usage footer, once the turn completes.
    #[serde(default)]
    pub stats: Option<TurnStats>,
    /// True while awaiting the assistant's reply (renders as "Thinking…").
    /// Never persisted as true — a restored turn is always finished.
    #[serde(default)]
    pub pending: bool,
}

impl ChatMessage {
    pub fn user(text: String) -> ChatMessage {
        ChatMessage {
            role: Role::User,
            text,
            segs: Vec::new(),
            stats: None,
            pending: false,
        }
    }
    /// Placeholder assistant message shown while the CLI runs.
    pub fn pending() -> ChatMessage {
        ChatMessage {
            role: Role::Assistant,
            text: String::new(),
            segs: Vec::new(),
            stats: None,
            pending: true,
        }
    }

    /// The message's prose: the user's own text, or the assistant's text
    /// segments joined (tool calls are the assistant *using* tools, not
    /// content). Used for the copy action and for replaying a restored
    /// conversation into a fresh session's prompt.
    pub fn prose(&self) -> String {
        if self.role == Role::User {
            return self.text.trim().to_string();
        }
        let mut out = String::new();
        for s in &self.segs {
            if let Seg::Text(t) = s {
                out.push_str(t);
            }
        }
        out.trim().to_string()
    }
}

/// One piece of a rendered assistant turn, in emission order.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Seg {
    /// Assistant prose (light markdown).
    Text(String),
    /// A tool the assistant invoked, with its result once it returns.
    Tool(ToolCall),
}

/// A single tool invocation and (once it returns) its result.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    /// Fully-qualified tool name, e.g. `mcp__schemaic__run_query`.
    pub name: String,
    /// The SQL argument, when the tool is a query tool.
    pub sql: Option<String>,
    /// The tool's textual result; `None` until it returns.
    pub result: Option<String>,
    /// Whether the returned result was an error.
    pub is_error: bool,
}

impl ToolCall {
    /// A short human label for the chip (strips the `mcp__server__` prefix).
    pub fn short_name(&self) -> &str {
        self.name.rsplit("__").next().unwrap_or(&self.name)
    }
}

/// Timing/usage summary for a finished turn (from the CLI's `result` event).
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct TurnStats {
    pub duration_ms: Option<u64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

/// A duration for a turn footer: `450ms` under a second, `1.2s` above.
///
/// Shared by [`TurnStats::summary`] (the finished turn) and the AI panel's live
/// counter (the turn in progress), which had its own copy — with a doc comment
/// saying it formatted "like `TurnStats::summary`'s time part", which is a
/// promise nothing enforced. The two sit next to each other on screen, one
/// replacing the other when the turn ends, so a drift between them would show
/// as the number changing format at the moment it stops moving.
pub fn elapsed_text(ms: u64) -> String {
    if ms >= 1000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{ms}ms")
    }
}

impl TurnStats {
    /// True when there's nothing worth showing in a footer.
    pub fn is_empty(&self) -> bool {
        self.duration_ms.is_none() && self.input_tokens.is_none() && self.output_tokens.is_none()
    }

    /// A compact one-line footer, e.g. `1.2s · ↑1.2k ↓340`.
    pub fn summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(ms) = self.duration_ms {
            parts.push(elapsed_text(ms));
        }
        let tok = match (self.input_tokens, self.output_tokens) {
            (Some(i), Some(o)) => Some(format!("↑{} ↓{}", human_count(i), human_count(o))),
            (Some(i), None) => Some(format!("↑{}", human_count(i))),
            (None, Some(o)) => Some(format!("↓{}", human_count(o))),
            (None, None) => None,
        };
        if let Some(t) = tok {
            parts.push(t);
        }
        parts.join("  ·  ")
    }
}

/// `1234 -> "1.2k"`, `12345 -> "12k"`, small counts unchanged.
fn human_count(n: u64) -> String {
    if n < 1000 {
        n.to_string()
    } else if n < 10_000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        format!("{}k", n / 1000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str) -> ToolCall {
        ToolCall {
            name: name.to_string(),
            sql: None,
            result: None,
            is_error: false,
        }
    }

    #[test]
    fn short_name_strips_mcp_prefix() {
        assert_eq!(call("mcp__schemaic__run_query").short_name(), "run_query");
    }

    #[test]
    fn short_name_passes_through_plain_names() {
        assert_eq!(call("Read").short_name(), "Read");
        assert_eq!(call("").short_name(), "");
    }

    #[test]
    fn is_empty_true_only_when_all_fields_none() {
        assert!(TurnStats::default().is_empty());
        assert!(
            !TurnStats {
                duration_ms: Some(1),
                ..Default::default()
            }
            .is_empty()
        );
        assert!(
            !TurnStats {
                input_tokens: Some(1),
                ..Default::default()
            }
            .is_empty()
        );
        assert!(
            !TurnStats {
                output_tokens: Some(1),
                ..Default::default()
            }
            .is_empty()
        );
    }

    #[test]
    fn summary_formats_sub_second_as_ms_and_over_second_as_seconds() {
        assert_eq!(
            TurnStats {
                duration_ms: Some(340),
                ..Default::default()
            }
            .summary(),
            "340ms"
        );
        assert_eq!(
            TurnStats {
                duration_ms: Some(1234),
                ..Default::default()
            }
            .summary(),
            "1.2s"
        );
        // Exactly 1000ms is the >= boundary → seconds.
        assert_eq!(
            TurnStats {
                duration_ms: Some(1000),
                ..Default::default()
            }
            .summary(),
            "1.0s"
        );
    }

    #[test]
    fn summary_covers_all_token_combinations() {
        let s = TurnStats {
            duration_ms: Some(1200),
            input_tokens: Some(1234),
            output_tokens: Some(340),
        }
        .summary();
        assert_eq!(s, "1.2s  ·  ↑1.2k ↓340");

        assert_eq!(
            TurnStats {
                input_tokens: Some(500),
                ..Default::default()
            }
            .summary(),
            "↑500"
        );
        assert_eq!(
            TurnStats {
                output_tokens: Some(12345),
                ..Default::default()
            }
            .summary(),
            "↓12k"
        );
        // No stats at all → empty string.
        assert_eq!(TurnStats::default().summary(), "");
    }

    #[test]
    fn human_count_buckets() {
        assert_eq!(human_count(0), "0");
        assert_eq!(human_count(999), "999");
        assert_eq!(human_count(1000), "1.0k");
        assert_eq!(human_count(1234), "1.2k");
        assert_eq!(human_count(9999), "10.0k");
        assert_eq!(human_count(10_000), "10k");
        assert_eq!(human_count(12_345), "12k");
    }
}
