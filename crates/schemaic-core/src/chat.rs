//! Persisted AI conversations (`chats.json`), one per connection.
//!
//! The AI panel binds a conversation to a connection — switching connections
//! resets it — so the store is keyed the same way and a switch back restores
//! what was there instead of a blank panel.
//!
//! **A restored conversation is transcript, not memory.** The `claude` session
//! it belonged to is long gone; the next message spawns a fresh one. The app
//! replays [`ChatMessage::prose`] into that session's system prompt so
//! follow-ups still resolve, but tool calls and their results are not replayed
//! — the assistant re-runs whatever it needs.
//!
//! **Tool *results* are never written to this file.** They are query output —
//! real table rows — and the conversation is what is worth keeping; see
//! [`ChatFile::of`].

use serde::{Deserialize, Serialize};

use crate::transcript::ChatMessage;

/// Messages kept per connection. Generous enough that a working session
/// survives intact, bounded so the file can't grow without limit.
pub const MAX_MESSAGES: usize = 100;

/// Stands in for a tool result that was deliberately not written to disk — see
/// [`ChatFile::of`]. A visible marker rather than an absent field, so a restored
/// chip still reads as a call that *returned* and anyone opening `chats.json`
/// can see why there is no data there.
pub const RESULT_OMITTED: &str = "(result not saved)";

/// One connection's saved conversation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedChat {
    pub conn_id: u64,
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
}

/// The persisted chats file (`chats.json`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChatFile {
    #[serde(default)]
    pub chats: Vec<SavedChat>,
}

impl ChatFile {
    /// The on-disk form of `chats`: every returned tool result replaced with
    /// [`RESULT_OMITTED`].
    ///
    /// **Tool results are query output — real table rows.** A `run_query` result
    /// is up to 200 rows rendered as a pipe table (names, emails, whatever the
    /// question selected), and writing it here exported the user's data to a
    /// plaintext file in the config directory, for the most recent 100 messages
    /// of every connection, indefinitely, without the UI ever saying the
    /// conversation was stored at all. Nothing downstream wants it either: the
    /// replay path already refuses to hand tool results to a new session ("the
    /// assistant re-runs whatever it needs"), so a restored conversation reads
    /// exactly as well without them. The call, its SQL and whether it errored are
    /// kept — that is the scrollback; the rows are re-fetchable.
    ///
    /// Stripping happens *here*, where a conversation becomes bytes, rather than
    /// in [`save`] — so the results still live in memory and switching
    /// connections and back mid-session shows the session's own scrollback
    /// intact. Both write paths go through this, so the rows have no other route
    /// to the file.
    pub fn of(chats: &[SavedChat]) -> ChatFile {
        ChatFile {
            chats: chats
                .iter()
                .map(|c| SavedChat {
                    conn_id: c.conn_id,
                    messages: c.messages.iter().cloned().map(drop_tool_results).collect(),
                })
                .collect(),
        }
    }
}

/// Replace every returned tool result with [`RESULT_OMITTED`]. A call that never
/// returned keeps its `None` — that is "still running", not data withheld.
fn drop_tool_results(mut m: ChatMessage) -> ChatMessage {
    for seg in &mut m.segs {
        if let crate::transcript::Seg::Tool(tc) = seg
            && tc.result.is_some()
        {
            tc.result = Some(RESULT_OMITTED.to_string());
        }
    }
    m
}

/// Store `messages` as `conn_id`'s conversation, replacing any previous one.
///
/// Drops anything not worth restoring — a still-streaming turn, and the user
/// message that provoked it — then keeps the most recent [`MAX_MESSAGES`]. A
/// conversation with nothing left to save is removed rather than stored empty.
pub fn save(chats: &mut Vec<SavedChat>, conn_id: u64, messages: &[ChatMessage]) {
    let keep = persistable(messages);
    if keep.is_empty() {
        clear_conn(chats, conn_id);
        return;
    }
    match chats.iter_mut().find(|c| c.conn_id == conn_id) {
        Some(existing) => existing.messages = keep,
        None => chats.push(SavedChat {
            conn_id,
            messages: keep,
        }),
    }
}

/// `conn_id`'s saved conversation, or empty when there isn't one.
pub fn for_conn(chats: &[SavedChat], conn_id: u64) -> Vec<ChatMessage> {
    chats
        .iter()
        .find(|c| c.conn_id == conn_id)
        .map(|c| c.messages.clone())
        .unwrap_or_default()
}

/// Forget `conn_id`'s conversation — New Chat, or a connection being deleted.
pub fn clear_conn(chats: &mut Vec<SavedChat>, conn_id: u64) {
    chats.retain(|c| c.conn_id != conn_id);
}

/// The tail of `messages` worth restoring.
///
/// A pending turn is mid-flight: it would come back as a "Thinking…" bubble
/// that never resolves, so it goes — along with the trailing user message it
/// was answering, which would otherwise restore as a question that was never
/// answered.
///
/// Tool *results* are kept here — they are the live session's own scrollback —
/// and dropped where the store becomes a file ([`ChatFile::of`]).
fn persistable(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    let mut keep: Vec<ChatMessage> = messages.iter().filter(|m| !m.pending).cloned().collect();
    while keep
        .last()
        .is_some_and(|m| m.role == crate::transcript::Role::User)
    {
        keep.pop();
    }
    if keep.len() > MAX_MESSAGES {
        keep.drain(..keep.len() - MAX_MESSAGES);
    }
    keep
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::{Role, Seg};

    fn user(text: &str) -> ChatMessage {
        ChatMessage::user(text.to_string())
    }

    fn reply(text: &str) -> ChatMessage {
        ChatMessage {
            role: Role::Assistant,
            text: String::new(),
            segs: vec![Seg::Text(text.to_string())],
            stats: None,
            pending: false,
            attachment: None,
        }
    }

    fn turn(q: &str, a: &str) -> Vec<ChatMessage> {
        vec![user(q), reply(a)]
    }

    #[test]
    fn save_then_load_round_trips_a_conversation() {
        let mut chats = Vec::new();
        save(&mut chats, 1, &turn("hi", "hello"));
        let got = for_conn(&chats, 1);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].prose(), "hi");
        assert_eq!(got[1].prose(), "hello");
    }

    #[test]
    fn conversations_are_kept_per_connection() {
        let mut chats = Vec::new();
        save(&mut chats, 1, &turn("q1", "a1"));
        save(&mut chats, 2, &turn("q2", "a2"));
        assert_eq!(for_conn(&chats, 1)[0].prose(), "q1");
        assert_eq!(for_conn(&chats, 2)[0].prose(), "q2");
        // An unknown connection has no history, not someone else's.
        assert!(for_conn(&chats, 3).is_empty());
    }

    #[test]
    fn saving_again_replaces_that_connection_only() {
        let mut chats = Vec::new();
        save(&mut chats, 1, &turn("q1", "a1"));
        save(&mut chats, 2, &turn("q2", "a2"));
        save(&mut chats, 1, &turn("q3", "a3"));
        assert_eq!(chats.len(), 2);
        assert_eq!(for_conn(&chats, 1).len(), 2);
        assert_eq!(for_conn(&chats, 1)[0].prose(), "q3");
        assert_eq!(for_conn(&chats, 2)[0].prose(), "q2");
    }

    #[test]
    fn a_pending_turn_and_its_question_are_not_saved() {
        let mut chats = Vec::new();
        let mut msgs = turn("done", "answered");
        msgs.push(user("interrupted"));
        msgs.push(ChatMessage::pending());
        save(&mut chats, 1, &msgs);
        let got = for_conn(&chats, 1);
        // Only the completed exchange survives.
        assert_eq!(got.len(), 2);
        assert_eq!(got[1].prose(), "answered");
    }

    #[test]
    fn a_conversation_with_nothing_answered_is_not_stored() {
        let mut chats = Vec::new();
        save(&mut chats, 1, &[user("hi"), ChatMessage::pending()]);
        assert!(chats.is_empty());
    }

    #[test]
    fn clearing_removes_only_the_named_conversation() {
        let mut chats = Vec::new();
        save(&mut chats, 1, &turn("q1", "a1"));
        save(&mut chats, 2, &turn("q2", "a2"));
        clear_conn(&mut chats, 1);
        assert!(for_conn(&chats, 1).is_empty());
        assert_eq!(for_conn(&chats, 2).len(), 2);
        // Clearing an absent connection is a no-op, not a panic.
        clear_conn(&mut chats, 99);
        assert_eq!(chats.len(), 1);
    }

    #[test]
    fn saving_an_emptied_conversation_drops_the_entry() {
        let mut chats = Vec::new();
        save(&mut chats, 1, &turn("q", "a"));
        save(&mut chats, 1, &[]); // New Chat
        assert!(chats.is_empty());
    }

    #[test]
    fn only_the_most_recent_messages_are_kept() {
        let mut chats = Vec::new();
        let msgs: Vec<ChatMessage> = (0..MAX_MESSAGES + 10)
            .flat_map(|i| turn(&format!("q{i}"), &format!("a{i}")))
            .collect();
        save(&mut chats, 1, &msgs);
        let got = for_conn(&chats, 1);
        assert_eq!(got.len(), MAX_MESSAGES);
        // The tail is what survives.
        assert_eq!(
            got.last().unwrap().prose(),
            format!("a{}", MAX_MESSAGES + 9)
        );
    }

    fn tool(sql: &str, result: Option<&str>) -> Seg {
        Seg::Tool(crate::transcript::ToolCall {
            name: "mcp__schemaic__run_query".to_string(),
            sql: Some(sql.to_string()),
            result: result.map(str::to_string),
            is_error: false,
        })
    }

    fn tool_of(m: &ChatMessage, i: usize) -> &crate::transcript::ToolCall {
        match &m.segs[i] {
            Seg::Tool(tc) => tc,
            other => panic!("expected a tool segment, got {other:?}"),
        }
    }

    #[test]
    fn a_tool_result_never_reaches_the_file() {
        // `run_query` returns up to 200 rows of real table data. Writing that to
        // `chats.json` exported the user's rows to a plaintext file without ever
        // telling them, and nothing downstream wants it — the replay path refuses
        // to hand tool results to a new session anyway.
        let mut chats = Vec::new();
        let mut msgs = turn("newest orders?", "here they are");
        msgs[1].segs.push(tool(
            "SELECT * FROM orders",
            Some("| id | email |\n| 1 | a@b.c |"),
        ));
        save(&mut chats, 1, &msgs);

        let json = serde_json::to_string(&ChatFile::of(&chats)).unwrap();
        assert!(!json.contains("a@b.c"), "row data reached the file: {json}");

        // What comes back from that file: the call and its SQL — the scrollback —
        // with a marker where the rows were, so the chip still reads as returned.
        let back: ChatFile = serde_json::from_str(&json).unwrap();
        let restored = for_conn(&back.chats, 1);
        let tc = tool_of(&restored[1], 1);
        assert_eq!(tc.sql.as_deref(), Some("SELECT * FROM orders"));
        assert_eq!(tc.result.as_deref(), Some(RESULT_OMITTED));
    }

    #[test]
    fn the_live_session_keeps_its_own_tool_results() {
        // Stripping happens where the store becomes bytes, not on `save` — so
        // switching connections and back mid-session still shows the results this
        // session produced.
        let mut chats = Vec::new();
        let mut msgs = turn("q", "a");
        msgs[1].segs.push(tool("SELECT 1", Some("| 1 |")));
        save(&mut chats, 1, &msgs);
        assert_eq!(
            tool_of(&for_conn(&chats, 1)[1], 1).result.as_deref(),
            Some("| 1 |")
        );
    }

    #[test]
    fn a_tool_call_that_never_returned_keeps_its_empty_result() {
        // `None` means "still running" — replacing it with the marker would show
        // a restored chip as finished.
        let mut chats = Vec::new();
        let mut msgs = turn("q", "a");
        msgs[1].segs.push(tool("SELECT 1", None));
        save(&mut chats, 1, &msgs);
        let file = ChatFile::of(&chats);
        assert_eq!(tool_of(&for_conn(&file.chats, 1)[1], 1).result, None);
    }

    #[test]
    fn messages_survive_a_json_round_trip() {
        let mut chats = Vec::new();
        let mut msgs = turn("count them", "42 rows");
        msgs[1].stats = Some(crate::transcript::TurnStats {
            duration_ms: Some(1200),
            input_tokens: Some(10),
            output_tokens: Some(20),
        });
        msgs[1].segs.push(Seg::Tool(crate::transcript::ToolCall {
            name: "mcp__schemaic__run_query".to_string(),
            sql: Some("SELECT 1".to_string()),
            result: Some("| 1 |".to_string()),
            is_error: false,
        }));
        save(&mut chats, 1, &msgs);

        let file = ChatFile { chats };
        let json = serde_json::to_string(&file).expect("serialize");
        let back: ChatFile = serde_json::from_str(&json).expect("deserialize");
        let got = for_conn(&back.chats, 1);
        assert_eq!(got[1].prose(), "42 rows");
        assert_eq!(got[1].stats.unwrap().duration_ms, Some(1200));
        match &got[1].segs[1] {
            Seg::Tool(tc) => {
                assert_eq!(tc.sql.as_deref(), Some("SELECT 1"));
                assert_eq!(tc.short_name(), "run_query");
            }
            other => panic!("expected a tool segment, got {other:?}"),
        }
    }

    #[test]
    fn an_empty_file_deserializes_to_no_chats() {
        let back: ChatFile = serde_json::from_str("{}").expect("deserialize");
        assert!(back.chats.is_empty());
    }
}
