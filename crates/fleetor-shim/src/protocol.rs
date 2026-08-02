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

/// The **lead-facing** fleet tools (Phase 4d): the surface the orchestrator drives
/// the fleet through, when the shim runs in `FLEETOR_ROLE=lead`. Mirrors the
/// worker face — single-sourced here so the shim and hub can't drift.
pub fn lead_tool_list() -> Value {
    json!([
        {
            "name": "assign",
            "description": "Dispatch a ticket to a worker slot. The fleet spawns a fresh worker in its own worktree and drives it. Give crisp acceptance criteria and the files it may write — not how to do it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "ticket id, e.g. T-041" },
                    "title": { "type": "string" },
                    "body": { "type": "string", "description": "description + acceptance criteria" },
                    "slot": { "type": "integer", "description": "target worker slot" },
                    "files_owned": { "type": "array", "items": { "type": "string" }, "description": "paths this ticket may write" }
                },
                "required": ["id", "title", "body", "slot"]
            }
        },
        {
            "name": "await_events",
            "description": "BLOCK up to timeout_ms for worker traffic that needs you: blocking questions (ask_lead) and progress notices (including report-filed). This is how you supervise without burning turns.",
            "inputSchema": {
                "type": "object",
                "properties": { "timeout_ms": { "type": "integer", "description": "max wait; default 30000" } }
            }
        },
        {
            "name": "inbox",
            "description": "Non-blocking drain of the same worker-traffic queue await_events serves. Returns immediately.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "fleet_status",
            "description": "The board: every ticket with its state (backlog/assigned/in-progress/in-review/done/blocked/failed) and assigned slot.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "reply",
            "description": "Answer a worker blocked in ask_lead. Pass the question's event_id (from await_events) and your answer.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "event_id": { "type": "string", "description": "the question's id from await_events" },
                    "text": { "type": "string" }
                },
                "required": ["event_id", "text"]
            }
        },
        {
            "name": "send",
            "description": "Steer a worker mid-flight with an async message (queued, delivered at its next turn boundary). Never interrupts.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "to": { "type": "integer", "description": "worker slot" },
                    "text": { "type": "string" }
                },
                "required": ["to", "text"]
            }
        },
        {
            "name": "interrupt",
            "description": "Yank a worker's in-flight turn: kills its process now, ending its run as interrupted. Use when a worker is off-track or wedged and steering (send) is too slow.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slot": { "type": "integer", "description": "worker slot to interrupt" }
                },
                "required": ["slot"]
            }
        },
        {
            "name": "worker_restart",
            "description": "Kill the worker on a slot and re-dispatch its ticket as a fresh worker. Use to recover a stuck worker without abandoning its ticket.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slot": { "type": "integer", "description": "worker slot to restart" }
                },
                "required": ["slot"]
            }
        }
    ])
}

/// Translate a **lead** MCP `tools/call` into a fleet `Op` (Phase 4d).
pub fn lead_tool_to_op(name: &str, args: &Value) -> Result<Op> {
    let str_arg = |k: &str| -> Result<String> {
        args.get(k)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| anyhow!("`{name}` requires string argument `{k}`"))
    };
    let u8_arg = |k: &str| -> Result<u8> {
        args.get(k)
            .and_then(Value::as_u64)
            .map(|n| n as u8)
            .ok_or_else(|| anyhow!("`{name}` requires integer argument `{k}`"))
    };
    match name {
        "assign" => {
            // The arguments object *is* a Ticket (single-sourced schema), so the
            // orchestrator's assign call round-trips straight into Op::Assign.
            let ticket = serde_json::from_value(args.clone())
                .map_err(|e| anyhow!("`assign` arguments are not a valid ticket: {e}"))?;
            Ok(Op::Assign { ticket })
        }
        "await_events" => Ok(Op::AwaitEvents {
            timeout_ms: args.get("timeout_ms").and_then(Value::as_u64).unwrap_or(30_000),
        }),
        "inbox" => Ok(Op::Inbox),
        "fleet_status" => Ok(Op::FleetStatus),
        "reply" => Ok(Op::Reply { event_id: str_arg("event_id")?, text: str_arg("text")? }),
        "send" => Ok(Op::Send { to: u8_arg("to")?, text: str_arg("text")? }),
        "interrupt" => Ok(Op::Interrupt { slot: u8_arg("slot")? }),
        "worker_restart" => Ok(Op::WorkerRestart { slot: u8_arg("slot")? }),
        other => Err(anyhow!("unknown lead tool `{other}`")),
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
        OpResult::Events { events } => (render_events(events), false),
        OpResult::Status { board } => (render_board(board), false),
        OpResult::Error { message } => (format!("fleet error: {message}"), true),
    };
    json!({ "content": [ { "type": "text", "text": text } ], "isError": is_error })
}

/// Fold pending mail into an already-rendered tool result (D-015 opportunistic
/// piggyback): a mid-turn worker that calls any fleet tool gets its queued mail
/// for free, as an extra content block framed like every other injection path.
/// A no-op for an empty queue.
pub fn append_piggyback(mut result: Value, messages: &[fleetor_core::Envelope]) -> Value {
    if messages.is_empty() {
        return result;
    }
    let framed = fleetor_core::frame_mail_for_injection(messages);
    if let Some(content) = result.get_mut("content").and_then(Value::as_array_mut) {
        content.push(json!({ "type": "text", "text": framed }));
    }
    result
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

/// Render lead-event traffic (`await_events` / `inbox`) as text the orchestrator
/// reads. Questions carry the `event_id` it must pass back to `reply`.
fn render_events(events: &[fleetor_core::wire::LeadEvent]) -> String {
    use fleetor_core::wire::LeadEventKind;
    if events.is_empty() {
        return "No new worker traffic.".to_string();
    }
    let mut s = String::from("Worker traffic:\n");
    for e in events {
        match &e.kind {
            LeadEventKind::Question { text, options } => {
                s.push_str(&format!(
                    "- ⛔ worker-{} asks (event_id {}): {}",
                    e.from, e.id, text
                ));
                if let Some(opts) = options {
                    if !opts.is_empty() {
                        s.push_str(&format!(" [options: {}]", opts.join(" / ")));
                    }
                }
                s.push_str(" — answer with reply(event_id, text)\n");
            }
            LeadEventKind::Notice { text } => {
                s.push_str(&format!("- worker-{}: {}\n", e.from, text));
            }
        }
    }
    s
}

/// Render the board (`fleet_status`): one line per ticket with state and slot.
fn render_board(board: &[fleetor_core::Ticket]) -> String {
    if board.is_empty() {
        return "No tickets on the board yet.".to_string();
    }
    let mut s = String::from("Board:\n");
    for t in board {
        let slot = t.slot.map(|n| format!("worker-{n}")).unwrap_or_else(|| "unassigned".to_string());
        let state = serde_json::to_value(t.state).ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        s.push_str(&format!("- {} [{}] {} — {}\n", t.id, state, slot, t.title));
    }
    s
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
    fn maps_each_lead_tool_to_its_op() {
        assert_eq!(
            lead_tool_to_op("await_events", &json!({"timeout_ms": 500})).unwrap(),
            Op::AwaitEvents { timeout_ms: 500 }
        );
        // Missing timeout defaults, never errors.
        assert_eq!(
            lead_tool_to_op("await_events", &json!({})).unwrap(),
            Op::AwaitEvents { timeout_ms: 30_000 }
        );
        assert_eq!(lead_tool_to_op("inbox", &json!({})).unwrap(), Op::Inbox);
        assert_eq!(lead_tool_to_op("fleet_status", &json!({})).unwrap(), Op::FleetStatus);
        assert_eq!(
            lead_tool_to_op("reply", &json!({"event_id": "q1", "text": "use B"})).unwrap(),
            Op::Reply { event_id: "q1".into(), text: "use B".into() }
        );
        assert_eq!(
            lead_tool_to_op("send", &json!({"to": 2, "text": "rebase first"})).unwrap(),
            Op::Send { to: 2, text: "rebase first".into() }
        );
        assert_eq!(
            lead_tool_to_op("interrupt", &json!({"slot": 3})).unwrap(),
            Op::Interrupt { slot: 3 }
        );
        assert_eq!(
            lead_tool_to_op("worker_restart", &json!({"slot": 4})).unwrap(),
            Op::WorkerRestart { slot: 4 }
        );
        // assign: the arguments object is a Ticket.
        let op = lead_tool_to_op(
            "assign",
            &json!({"id": "T-1", "title": "do it", "body": "AC…", "slot": 3, "files_owned": ["src/a.rs"]}),
        )
        .unwrap();
        match op {
            Op::Assign { ticket } => {
                assert_eq!(ticket.id, "T-1");
                assert_eq!(ticket.slot, Some(3));
                assert_eq!(ticket.files_owned, vec!["src/a.rs".to_string()]);
            }
            other => panic!("expected Assign, got {other:?}"),
        }
    }

    #[test]
    fn every_listed_lead_tool_is_translatable() {
        let sample = json!({
            "assign": {"id": "T-1", "title": "t", "body": "b", "slot": 1},
            "await_events": {"timeout_ms": 100},
            "inbox": {},
            "fleet_status": {},
            "reply": {"event_id": "q1", "text": "answer"},
            "send": {"to": 1, "text": "steer"},
            "interrupt": {"slot": 1},
            "worker_restart": {"slot": 1},
        });
        for tool in lead_tool_list().as_array().unwrap() {
            let name = tool["name"].as_str().unwrap();
            let args = &sample[name];
            assert!(lead_tool_to_op(name, args).is_ok(), "lead tool `{name}` has no Op mapping");
        }
    }

    #[test]
    fn lead_and_worker_faces_are_disjoint() {
        // A worker tool is not a lead tool and vice-versa — the split is real.
        assert!(lead_tool_to_op("ask_lead", &json!({"question": "?"})).is_err());
        assert!(tool_to_op("assign", &json!({"id": "T-1", "title": "t", "body": "b", "slot": 1})).is_err());
    }

    #[test]
    fn renders_events_and_board_for_the_lead() {
        use fleetor_core::wire::{LeadEvent, LeadEventKind};
        let events = vec![
            LeadEvent { id: "q7".into(), from: 2, kind: LeadEventKind::Question { text: "A or B?".into(), options: Some(vec!["A".into(), "B".into()]) } },
            LeadEvent { id: "n1".into(), from: 3, kind: LeadEventKind::Notice { text: "filed a report".into() } },
        ];
        let text = super::render_events(&events);
        assert!(text.contains("q7") && text.contains("A or B?") && text.contains("worker-2"), "{text}");
        assert!(text.contains("worker-3") && text.contains("filed a report"), "{text}");

        let board = vec![fleetor_core::Ticket::new("T-9", "wire it", "body")];
        let b = super::render_board(&board);
        assert!(b.contains("T-9") && b.contains("backlog"), "{b}");
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
