//! `fleetor-shim` — the per-worker bridge Claude Code spawns as a stdio MCP
//! server (handoff §2). It speaks MCP JSON-RPC to CC on stdin/stdout and proxies
//! every `tools/call` to the fleet server over the unix socket, carrying this
//! worker's slot identity. One binary; the slot comes from the environment so
//! the same binary serves every worker (BUILDING §2 "slot identity via env var").
//!
//! Two modes, one binary:
//!   (default)     MCP stdio server CC spawns via `--mcp-config`.
//!   `stop-hook`   the `Stop` hook CC runs at turn end — drains queued mail and,
//!                 if any, emits a `block` decision to inject it mid-turn (§5).
//!
//! Env (both modes):
//!   FLEET_SOCKET   path to the fleet unix socket (`~/.fleetor/<key>/fleet.sock`)
//!   FLEETOR_SLOT   this worker's slot number (1..=N)
//!
//! The MCP + Stop-hook wire facts come from the captured spike — see
//! `protocol.rs` and `docs/phase2-spikes.md`.

mod protocol;

use anyhow::{Context, Result};
use fleetor_core::wire::{Hello, Op, OpResult};
use fleetor_core::Party;
use fleetor_ipc::{Client, UnixTransport};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let socket = std::env::var("FLEET_SOCKET").context("FLEET_SOCKET not set")?;

    // Role selects the party and the tool face (Phase 4d). Default: worker.
    let is_lead = std::env::var("FLEETOR_ROLE").map(|r| r == "lead").unwrap_or(false);
    let party = if is_lead {
        Party::Lead
    } else {
        let slot: u8 = std::env::var("FLEETOR_SLOT")
            .context("FLEETOR_SLOT not set")?
            .parse()
            .context("FLEETOR_SLOT must be a number")?;
        Party::Worker(slot)
    };

    let transport = UnixTransport::new(&socket);
    let mut client = connect_with_retry(&transport, party).await?;

    match std::env::args().nth(1).as_deref() {
        // The Stop hook is a worker-only turn-boundary drain; the lead's stdin is
        // the human's, never injected into.
        Some("stop-hook") => run_stop_hook(&mut client).await,
        _ => run_mcp_server(&mut client, is_lead).await,
    }
}

/// The `Stop` hook: drain this worker's mail; if any, emit a `block` decision so
/// CC re-injects it into the same turn (handoff §5). Empty queue → no output,
/// which lets the turn end. The queue emptying is the loop terminator, so no
/// `stop_hook_active` bookkeeping is needed.
async fn run_stop_hook(client: &mut Client) -> Result<()> {
    // Drain CC's Stop payload from stdin so it never blocks on a full pipe; we
    // don't need its contents (the mail queue drives the decision).
    let mut _payload = String::new();
    let _ = tokio::io::stdin().read_to_string(&mut _payload).await;

    let messages = match client.call(Op::DrainMail).await? {
        OpResult::Mail { messages } => messages,
        _ => Vec::new(),
    };
    if messages.is_empty() {
        return Ok(()); // allow the turn to end
    }
    let reason = protocol::frame_mail_for_injection(&messages);
    let decision = json!({ "decision": "block", "reason": reason });
    let mut out = tokio::io::stdout();
    out.write_all(decision.to_string().as_bytes()).await?;
    out.flush().await?;
    Ok(())
}

/// The MCP stdio server: bridge each `tools/call` to the fleet socket. `is_lead`
/// selects the tool face (lead vs worker).
async fn run_mcp_server(client: &mut Client, is_lead: bool) -> Result<()> {
    let stdin = BufReader::new(tokio::io::stdin());
    let mut stdout = tokio::io::stdout();
    let mut lines = stdin.lines();

    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue, // ignore non-JSON noise
        };
        if let Some(reply) = dispatch(client, &req, is_lead).await {
            let mut s = serde_json::to_string(&reply)?;
            s.push('\n');
            stdout.write_all(s.as_bytes()).await?;
            stdout.flush().await?;
        }
    }
    Ok(())
}

/// The hub may not be listening the instant CC spawns us; retry briefly.
async fn connect_with_retry(transport: &UnixTransport, party: Party) -> Result<Client> {
    let mut last_err = None;
    for _ in 0..50 {
        match Client::connect(transport, Hello::new(party.clone())).await {
            Ok(c) => return Ok(c),
            Err(e) => {
                last_err = Some(e);
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
    Err(last_err.unwrap()).context("could not connect to the fleet socket")
}

/// Handle one JSON-RPC message. Returns `None` for notifications (no response).
async fn dispatch(client: &mut Client, req: &Value, is_lead: bool) -> Option<Value> {
    let method = req.get("method").and_then(Value::as_str)?;
    let id = req.get("id").cloned();

    // Notifications carry no id and expect no reply.
    let id = id?;
    let params = req.get("params").cloned().unwrap_or(json!({}));

    let tools = if is_lead { protocol::lead_tool_list() } else { protocol::tool_list() };
    let result: Value = match method {
        "initialize" => protocol::initialize_result(&params),
        "tools/list" => json!({ "tools": tools }),
        "ping" => json!({}),
        "tools/call" => call_tool(client, &params, is_lead).await,
        _ => return Some(rpc_error(&id, -32601, &format!("method not found: {method}"))),
    };
    Some(rpc_ok(&id, result))
}

/// Proxy a `tools/call` to the fleet server and render its result for MCP.
async fn call_tool(client: &mut Client, params: &Value, is_lead: bool) -> Value {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(json!({}));

    let mapped = if is_lead {
        protocol::lead_tool_to_op(name, &args)
    } else {
        protocol::tool_to_op(name, &args)
    };
    let op = match mapped {
        Ok(op) => op,
        Err(e) => return protocol::op_result_to_mcp(&OpResult::Error { message: e.to_string() }),
    };
    match client.call(op).await {
        Ok(result) => protocol::op_result_to_mcp(&result),
        Err(e) => protocol::op_result_to_mcp(&OpResult::Error {
            message: format!("socket call failed: {e}"),
        }),
    }
}

fn rpc_ok(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn rpc_error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}
