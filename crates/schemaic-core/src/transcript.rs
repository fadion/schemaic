//! Rendered shape of one assistant turn in the AI panel.
//!
//! The AI crate accumulates a `claude` stream into these segments; the UI
//! renders them (prose as markdown, tool calls as chips) and shows the
//! per-turn [`TurnStats`] footer. Keeping the type here lets both crates share
//! it without the UI depending on the CLI-integration crate.

/// One piece of a rendered assistant turn, in emission order.
#[derive(Clone, Debug)]
pub enum Seg {
    /// Assistant prose (light markdown).
    Text(String),
    /// A tool the assistant invoked, with its result once it returns.
    Tool(ToolCall),
}

/// A single tool invocation and (once it returns) its result.
#[derive(Clone, Debug)]
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
#[derive(Clone, Copy, Debug, Default)]
pub struct TurnStats {
    pub duration_ms: Option<u64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
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
            parts.push(if ms >= 1000 {
                format!("{:.1}s", ms as f64 / 1000.0)
            } else {
                format!("{ms}ms")
            });
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
