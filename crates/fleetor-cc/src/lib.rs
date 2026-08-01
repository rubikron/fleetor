//! `fleetor-cc` — the Claude Code adapter.
//!
//! Protocol only: the typed `stream-json` event model ([`event`]), a tolerant
//! line parser ([`parse`]), and the isolated worker-spawn builder ([`spawn`]).
//! Supervision, routing, and analysis live in higher crates. This crate is the
//! single seam where Claude Code version drift is absorbed (BUILDING §3).

pub mod agent;
pub mod event;
pub mod parse;
pub mod session;
pub mod spawn;

pub use agent::{AgentProcess, FakeClaude, RealClaude};
pub use event::{ContentBlock, Event, InitEvent, Message, ResultEvent, ToolResult, ToolUse, Usage};
pub use parse::{parse_line, parse_transcript};
pub use session::{Recv, Session, SessionMsg};
pub use spawn::{WorkerConfig, DEEPSEEK_ANTHROPIC_BASE_URL, MODEL_FLASH};

#[cfg(test)]
mod tests {
    use super::*;

    /// The captured Flash tool-use transcript must decode to the exact tool
    /// sequence we saw by hand: Bash(fail) → Read → Edit → Bash(pass) → result.
    #[test]
    fn parses_captured_tooluse_fixture() {
        let text = include_str!("../../../tests/fixtures/ndjson/spike-03-tooluse.ndjson");
        let events = parse_transcript(text);

        let init = events.iter().find_map(|e| match e {
            Event::Init(i) => Some(i),
            _ => None,
        });
        assert!(init.is_some(), "expected an init event");
        assert_eq!(init.unwrap().model.as_deref(), Some("deepseek-v4-flash"));

        let tool_uses: Vec<&ToolUse> = events
            .iter()
            .filter_map(|e| match e {
                Event::Assistant(m) => Some(&m.message.content),
                _ => None,
            })
            .flatten()
            .filter_map(|b| match b {
                ContentBlock::ToolUse(t) => Some(t),
                _ => None,
            })
            .collect();
        let names: Vec<&str> = tool_uses.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["Bash", "Read", "Edit", "Bash"]);

        // The Edit call carried well-formed keys (fidelity signal).
        let edit = tool_uses.iter().find(|t| t.name == "Edit").unwrap();
        assert!(edit.input.get("old_string").is_some());
        assert!(edit.input.get("new_string").is_some());

        // Exactly one tool_result carried is_error (the intended failing test),
        // and it belongs to a Bash call — a task-level error, not a fidelity one.
        let errored: Vec<&ToolResult> = events
            .iter()
            .filter_map(|e| match e {
                Event::User(m) => Some(&m.message.content),
                _ => None,
            })
            .flatten()
            .filter_map(|b| match b {
                ContentBlock::ToolResult(t) if t.is_error => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(errored.len(), 1);
        assert!(errored[0].text().contains("Exit code 1"));

        // Turn ended cleanly.
        let result = events.iter().find_map(|e| match e {
            Event::Result(r) => Some(r),
            _ => None,
        });
        assert!(result.is_some());
        assert!(!result.unwrap().is_error);
        assert!(result.unwrap().usage.input_tokens > 0);
    }

    #[test]
    fn tolerates_blank_and_garbage_lines() {
        assert!(parse_line("").is_none());
        assert!(parse_line("   ").is_none());
        assert!(parse_line("{not json").unwrap().is_err());
        // Unknown type falls into Other, not an error.
        let ev = parse_line(r#"{"type":"system","subtype":"thinking_tokens"}"#)
            .unwrap()
            .unwrap();
        assert!(matches!(ev, Event::Other(_)));
    }
}
