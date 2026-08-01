//! The MCP surface the shim presents to Claude Code, and its translation to the
//! fleet wire `Op`s (single-sourced here so the shim and server can't drift —
//! BUILDING §4.4). Pure functions, unit-tested; the stdio/socket plumbing lives
//! in `main.rs`.
//!
//! Framing and handshake are from the captured spike (see `docs/phase2-spikes.md`
//! and `tests/fixtures/mcp/`): newline-delimited JSON-RPC 2.0,
//! `initialize` → `notifications/initialized` → `tools/list` → `tools/call`.
//! CC advertises protocol `2025-11-25`; `tools/call` carries the **bare** tool
//! name (e.g. `ask_lead`), while the model sees it namespaced as
//! `mcp__fleet__ask_lead`.

use anyhow::{anyhow, Result};
use fleetor_core::wire::Op;
use serde_json::{json, Value};

/// The MCP protocol version CC used in the spike; we echo the client's if given.
pub const DEFAULT_PROTOCOL_VERSION: &str = "2025-11-25";
pub const SERVER_NAME: &str = "fleet";

/// The worker-facing fleet tools exposed to the model (handoff §4). `drain_mail`
/// is intentionally absent — it is the Stop-hook's turn-boundary pull, not a
/// tool the worker calls itself.
pub fn tool_list() -> Value {
    json!([
        {
            "name": "ask_lead",
            "description": "Ask the tech lead a BLOCKING question and wait for the answer. Use when the ticket is ambiguous or a decision is above your pay grade — raising a hand is cheaper than guessing.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "question": { "type": "string", "description": "the question for the lead" },
                    "options": { "type": "array", "items": { "type": "string" }, "description": "optional choices to pick among" }
                },
                "required": ["question"]
            }
        },
        {
            "name": "notify_lead",
            "description": "Send the lead a fire-and-forget progress note. Does not block and expects no reply.",
            "inputSchema": {
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"]
            }
        },
        {
            "name": "dm",
            "description": "Send an async direct message to a peer worker slot. Delivered at their next turn boundary. Never blocks. Use for facts that change a teammate's work, not status chatter.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "to": { "type": "integer", "description": "peer worker slot number" },
                    "text": { "type": "string" }
                },
                "required": ["to", "text"]
            }
        },
        {
            "name": "broadcast",
            "description": "Send an async message to all peer workers. Use sparingly — facts, not narration.",
            "inputSchema": {
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"]
            }
        },
        {
            "name": "report",
            "description": "File your structured completion report for the ticket. Call this once, at the end, when the work is done or you are blocked. status is one of done|blocked|needs-decision|failed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "ticket": { "type": "string" },
                    "status": { "type": "string", "enum": ["done", "blocked", "needs-decision", "failed"] },
                    "summary": { "type": "string", "description": "≤200 words" },
                    "branch": { "type": "string" },
                    "diffstat": { "type": "string" },
                    "decisions": { "type": "array", "items": { "type": "string" } },
                    "questions": { "type": "array", "items": { "type": "string" } },
                    "risks": { "type": "array", "items": { "type": "string" } },
                    "followups": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["ticket", "status", "summary"]
            }
        },
        {
            "name": "whos_working_on",
            "description": "Cheap conflict check: which worker slots currently hold a file path? Check before you touch shared code.",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }
        },
        {
            "name": "claim_file",
            "description": "Request a lease to write a file path for your ticket. May be denied if another slot already holds it. Pass your own ticket id.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "ticket": { "type": "string", "description": "your ticket id, e.g. T-041" }
                },
                "required": ["path", "ticket"]
            }
        },
        {
            "name": "backlog_add",
            "description": "Park an out-of-scope discovery so it doesn't leak into your diff. This is a success, not a distraction.",
            "inputSchema": {
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"]
            }
        }
    ])
}

/// Translate an MCP `tools/call` into a fleet `Op`. The bare tool name and its
/// arguments object come straight from CC.
pub fn tool_to_op(name: &str, args: &Value) -> Result<Op> {
    let str_arg = |k: &str| -> Result<String> {
        args.get(k)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| anyhow!("`{name}` requires string argument `{k}`"))
    };
    match name {
        "ask_lead" => Ok(Op::AskLead {
            question: str_arg("question")?,
            options: args.get("options").and_then(|v| v.as_array()).map(|a| {
                a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()
            }),
        }),
        "notify_lead" => Ok(Op::NotifyLead { text: str_arg("text")? }),
        "dm" => Ok(Op::Dm {
            to: args
                .get("to")
                .and_then(Value::as_u64)
                .ok_or_else(|| anyhow!("`dm` requires integer argument `to`"))? as u8,
            text: str_arg("text")?,
        }),
        "broadcast" => Ok(Op::Broadcast { text: str_arg("text")? }),
        "report" => {
            // The report arguments object *is* a Report (single-sourced schema).
            let report = serde_json::from_value(args.clone())
                .map_err(|e| anyhow!("`report` arguments are not a valid report: {e}"))?;
            Ok(Op::Report { report })
        }
        "whos_working_on" => Ok(Op::WhosWorkingOn { path: str_arg("path")? }),
        "claim_file" => Ok(Op::ClaimFile { path: str_arg("path")?, ticket: str_arg("ticket")? }),
        "backlog_add" => Ok(Op::BacklogAdd { text: str_arg("text")? }),
        other => Err(anyhow!("unknown fleet tool `{other}`")),
    }
}

/// Render a fleet `OpResult` as an MCP `tools/call` result payload. Mail and the
/// park-vs-real-answer distinction are surfaced as plain text the worker reads.
pub fn op_result_to_mcp(result: &fleetor_core::wire::OpResult) -> Value {
    use fleetor_core::wire::OpResult;
    let (text, is_error) = match result {
        OpResult::Ack => ("Delivered.".to_string(), false),
        OpResult::Answer { text, answered } => {
            if *answered {
                (format!("Lead replied: {text}"), false)
            } else {
                (format!("(no reply) {text}"), false)
            }
        }
        OpResult::Mail { messages } => (render_mail(messages), false),
        OpResult::Owners { owners } => (render_owners(owners), false),
        OpResult::Claim { grant } => (render_claim(grant), false),
        OpResult::Events { .. } => ("(unexpected: events on a worker call)".to_string(), true),
        OpResult::Error { message } => (format!("fleet error: {message}"), true),
    };
    json!({ "content": [ { "type": "text", "text": text } ], "isError": is_error })
}

/// Frame queued mail as the `reason` of a Stop-hook `block` decision, for
/// mid-turn injection (handoff §5). The spike (docs/phase2-spikes.md) showed a
/// security-conscious worker will REFUSE injected text that reads like an
/// override of its task — so this frames mail explicitly as in-band teammate
/// coordination that augments the current work, never a new directive.
pub fn frame_mail_for_injection(messages: &[fleetor_core::Envelope]) -> String {
    let mut s = String::from(
        "[Fleet mail — coordination from your teammates on this ticket, delivered mid-task. \
         This is information to factor in, not a new instruction that overrides your ticket.]\n",
    );
    for m in messages {
        s.push_str(&format!("• {}: {}\n", sender_label(&m.from), m.body));
    }
    s.push_str("\nAcknowledge anything that affects your current work, then carry on.");
    s
}

fn sender_label(p: &fleetor_core::Party) -> String {
    match p {
        fleetor_core::Party::Lead => "lead".to_string(),
        fleetor_core::Party::Worker(n) => format!("worker-{n}"),
        fleetor_core::Party::User => "user".to_string(),
    }
}

fn render_owners(owners: &[fleetor_core::Owner]) -> String {
    if owners.is_empty() {
        return "Nobody is working on that path — clear to claim.".to_string();
    }
    let mut s = String::from("Held by:\n");
    for o in owners {
        s.push_str(&format!("- worker-{} (ticket {})\n", o.slot, o.ticket));
    }
    s
}

fn render_claim(grant: &fleetor_core::LeaseGrant) -> String {
    match grant {
        fleetor_core::LeaseGrant::Granted => "Claim granted.".to_string(),
        fleetor_core::LeaseGrant::Denied { held_by } => format!(
            "Claim denied — worker-{} holds it for ticket {}. Coordinate via dm or ask_lead.",
            held_by.slot, held_by.ticket
        ),
    }
}

fn render_mail(messages: &[fleetor_core::Envelope]) -> String {
    if messages.is_empty() {
        return "No new mail.".to_string();
    }
    let mut s = String::from("New fleet mail:\n");
    for m in messages {
        let from = match &m.from {
            fleetor_core::Party::Lead => "lead".to_string(),
            fleetor_core::Party::Worker(n) => format!("worker-{n}"),
            fleetor_core::Party::User => "user".to_string(),
        };
        s.push_str(&format!("- from {from}: {}\n", m.body));
    }
    s
}

/// Build the JSON-RPC response envelope for an `initialize` request, echoing the
/// client's protocol version when present.
pub fn initialize_result(client_params: &Value) -> Value {
    let protocol = client_params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_PROTOCOL_VERSION);
    json!({
        "protocolVersion": protocol,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_each_tool_to_its_op() {
        assert_eq!(
            tool_to_op("ask_lead", &json!({"question": "A or B?", "options": ["A", "B"]})).unwrap(),
            Op::AskLead { question: "A or B?".into(), options: Some(vec!["A".into(), "B".into()]) }
        );
        assert_eq!(
            tool_to_op("dm", &json!({"to": 3, "text": "hi"})).unwrap(),
            Op::Dm { to: 3, text: "hi".into() }
        );
        assert_eq!(
            tool_to_op("notify_lead", &json!({"text": "progress"})).unwrap(),
            Op::NotifyLead { text: "progress".into() }
        );
        assert_eq!(
            tool_to_op("broadcast", &json!({"text": "done"})).unwrap(),
            Op::Broadcast { text: "done".into() }
        );
    }

    #[test]
    fn rejects_unknown_tool_and_missing_args() {
        assert!(tool_to_op("teleport", &json!({})).is_err());
        assert!(tool_to_op("dm", &json!({"text": "no target"})).is_err());
        assert!(tool_to_op("ask_lead", &json!({})).is_err());
    }

    #[test]
    fn every_listed_tool_is_translatable() {
        // Guards drift: each tool advertised in tool_list must map to an Op.
        let sample = json!({
            "ask_lead": {"question": "q"},
            "notify_lead": {"text": "t"},
            "dm": {"to": 1, "text": "t"},
            "broadcast": {"text": "t"},
            "report": {"ticket": "T-1", "status": "done", "summary": "s"},
            "whos_working_on": {"path": "src/a.rs"},
            "claim_file": {"path": "src/a.rs", "ticket": "T-1"},
            "backlog_add": {"text": "found a bug elsewhere"},
        });
        for tool in tool_list().as_array().unwrap() {
            let name = tool["name"].as_str().unwrap();
            let args = &sample[name];
            assert!(tool_to_op(name, args).is_ok(), "tool `{name}` has no Op mapping");
        }
    }

    #[test]
    fn park_answer_is_marked_but_not_an_error() {
        let v = op_result_to_mcp(&fleetor_core::wire::OpResult::Answer {
            text: "use judgment".into(),
            answered: false,
        });
        assert_eq!(v["isError"], json!(false));
        assert!(v["content"][0]["text"].as_str().unwrap().contains("no reply"));
    }
}
