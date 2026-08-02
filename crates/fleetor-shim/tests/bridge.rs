//! End-to-end: the real shim binary bridges MCP (stdin/stdout) to the fleet
//! socket. Drives the captured handshake against a live hub and asserts a
//! `tools/call` routes all the way through to the hub's event log. This is the
//! shim's proof before the real-CC pass (Phase 2 step 6).

use fleetor_core::Store;
use fleetor_db::SqliteStore;
use fleetor_ipc::{Transport, UnixTransport};
use fleetor_server::{Hub, HubConfig};
use serde_json::{json, Value};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn shim_bridges_mcp_tools_call_to_the_hub() {
    // --- live hub on a temp socket ---
    let dir = std::env::temp_dir().join(format!("fleetor-shim-it-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("fleet.sock");
    let transport = Arc::new(UnixTransport::new(&sock));
    let store = Arc::new(SqliteStore::open_in_memory().unwrap());
    let hub = Hub::new(store.clone(), HubConfig { slots: vec![1, 2], ask_timeout: Duration::from_secs(5) });
    let listener = transport.bind().await.unwrap();
    tokio::spawn(hub.serve(listener));

    // --- spawn the real shim binary as worker-1 ---
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_fleetor-shim"))
        .env("FLEET_SOCKET", &sock)
        .env("FLEETOR_SLOT", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut sin = child.stdin.take().unwrap();
    let mut sout = BufReader::new(child.stdout.take().unwrap()).lines();

    // initialize → expect a result with serverInfo.name = "fleet"
    send_line(&mut sin, json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25"}})).await;
    let init: Value = read_json(&mut sout).await;
    assert_eq!(init["result"]["serverInfo"]["name"], "fleet");

    // notifications/initialized (no reply expected)
    send_line(&mut sin, json!({"jsonrpc":"2.0","method":"notifications/initialized"})).await;

    // tools/list → expect our four tools
    send_line(&mut sin, json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).await;
    let list: Value = read_json(&mut sout).await;
    let names: Vec<&str> = list["result"]["tools"].as_array().unwrap()
        .iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"ask_lead") && names.contains(&"dm"), "tools: {names:?}");

    // tools/call dm(to=2) → routes through the socket; hub persists a mail event.
    send_line(&mut sin, json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":"dm","arguments":{"to":2,"text":"routed via the shim"}}})).await;
    let call: Value = read_json(&mut sout).await;
    assert_eq!(call["result"]["isError"], json!(false), "call result: {call}");

    // The dm reached the hub: a mail event is in the log, and worker-2 can drain it.
    let mail_events = store.events_since(0).unwrap().into_iter().filter(|(_, e)| e.kind() == "mail").count();
    assert_eq!(mail_events, 1, "the dm should have produced one mail event");
    let w2 = store.take_mail(&fleetor_core::Party::Worker(2)).unwrap();
    assert_eq!(w2.len(), 1);
    assert_eq!(w2[0].body, "routed via the shim");

    let _ = child.kill().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn lead_shim_bridges_assign_to_the_hub() {
    // --- live hub with a dispatcher (a dynamic fleet) on a temp socket ---
    let dir = std::env::temp_dir().join(format!("fleetor-lead-it-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("fleet.sock");
    let transport = Arc::new(UnixTransport::new(&sock));
    let store = Arc::new(SqliteStore::open_in_memory().unwrap());
    let (assign_tx, mut assign_rx) = tokio::sync::mpsc::unbounded_channel();
    let hub = fleetor_server::Hub::with_dispatcher(
        store.clone(),
        HubConfig { slots: vec![1, 2], ask_timeout: Duration::from_secs(5) },
        assign_tx,
    );
    let listener = transport.bind().await.unwrap();
    tokio::spawn(hub.serve(listener));

    // --- spawn the real shim binary in LEAD role (no slot needed) ---
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_fleetor-shim"))
        .env("FLEET_SOCKET", &sock)
        .env("FLEETOR_ROLE", "lead")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut sin = child.stdin.take().unwrap();
    let mut sout = BufReader::new(child.stdout.take().unwrap()).lines();

    send_line(&mut sin, json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25"}})).await;
    let _ = read_json(&mut sout).await;
    send_line(&mut sin, json!({"jsonrpc":"2.0","method":"notifications/initialized"})).await;

    // tools/list → the LEAD face, not the worker face.
    send_line(&mut sin, json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).await;
    let list: Value = read_json(&mut sout).await;
    let names: Vec<&str> = list["result"]["tools"].as_array().unwrap()
        .iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"assign") && names.contains(&"await_events"), "lead tools: {names:?}");
    assert!(!names.contains(&"ask_lead"), "lead face must not expose worker tools: {names:?}");

    // tools/call assign(ticket) → routes through the socket to the dispatcher.
    send_line(&mut sin, json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":"assign","arguments":{"id":"T-77","title":"do it","body":"AC…","slot":2,"files_owned":["src/a.rs"]}}})).await;
    let call: Value = read_json(&mut sout).await;
    assert_eq!(call["result"]["isError"], json!(false), "call result: {call}");

    // The assign reached the runner: the dispatcher received the ticket, and the
    // hub persisted it so fleet_status would show it.
    let cmd = tokio::time::timeout(Duration::from_secs(5), assign_rx.recv())
        .await
        .expect("assign should reach the dispatcher")
        .expect("dispatcher channel open");
    let ticket = match cmd {
        fleetor_server::RunnerCommand::Assign(a) => a.ticket,
        other => panic!("expected an assign command, got {other:?}"),
    };
    assert_eq!(ticket.id, "T-77");
    assert_eq!(ticket.slot, Some(2));
    assert!(store.tickets().unwrap().iter().any(|t| t.id == "T-77"), "hub should have persisted the ticket");

    let _ = child.kill().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn stop_hook_drains_mail_into_a_block_decision() {
    use fleetor_core::wire::{Hello, Op, OpResult};
    use fleetor_ipc::Client;

    let dir = std::env::temp_dir().join(format!("fleetor-hook-it-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("fleet.sock");
    let transport = Arc::new(UnixTransport::new(&sock));
    let store = Arc::new(SqliteStore::open_in_memory().unwrap());
    let hub = Hub::new(store.clone(), HubConfig { slots: vec![1, 2], ask_timeout: Duration::from_secs(5) });
    let listener = transport.bind().await.unwrap();
    tokio::spawn(hub.serve(listener));

    // Queue mail for worker-1: the lead sends it (idle/mid-turn does not matter
    // to the queue — it waits until drained).
    let mut lead = Client::connect(&*transport, Hello::new(fleetor_core::Party::Lead)).await.unwrap();
    assert_eq!(
        lead.call(Op::Send { to: 1, text: "rebase before you push".into() }).await.unwrap(),
        OpResult::Ack
    );

    // Run the shim's stop-hook for worker-1 → it should emit a block decision.
    let out = run_stop_hook(&sock, 1).await;
    let decision: Value = serde_json::from_str(out.trim()).expect("hook should emit JSON");
    assert_eq!(decision["decision"], "block");
    let reason = decision["reason"].as_str().unwrap();
    assert!(reason.contains("rebase before you push"), "reason: {reason}");
    assert!(reason.contains("lead"), "mail should be attributed to its sender");

    // Second run: queue is now empty → no output (the turn is allowed to end).
    let out2 = run_stop_hook(&sock, 1).await;
    assert!(out2.trim().is_empty(), "empty queue must produce no decision, got: {out2:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn tool_result_piggybacks_pending_mail() {
    use fleetor_core::wire::{Hello, Op};
    use fleetor_ipc::Client;

    let dir = std::env::temp_dir().join(format!("fleetor-piggy-it-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("fleet.sock");
    let transport = Arc::new(UnixTransport::new(&sock));
    let store = Arc::new(SqliteStore::open_in_memory().unwrap());
    let hub = Hub::new(store.clone(), HubConfig { slots: vec![1, 2], ask_timeout: Duration::from_secs(5) });
    let listener = transport.bind().await.unwrap();
    tokio::spawn(hub.serve(listener));

    // --- worker-1 shim, initialized ---
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_fleetor-shim"))
        .env("FLEET_SOCKET", &sock)
        .env("FLEETOR_SLOT", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut sin = child.stdin.take().unwrap();
    let mut sout = BufReader::new(child.stdout.take().unwrap()).lines();
    send_line(&mut sin, json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25"}})).await;
    let _ = read_json(&mut sout).await;
    send_line(&mut sin, json!({"jsonrpc":"2.0","method":"notifications/initialized"})).await;

    // Queue mail for worker-1 (the lead steers it while it works).
    let mut lead = Client::connect(&*transport, Hello::new(fleetor_core::Party::Lead)).await.unwrap();
    lead.call(Op::Send { to: 1, text: "rebase before you push".into() }).await.unwrap();

    // A mid-turn tool call folds the pending mail into its own result (free delivery).
    send_line(&mut sin, json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
        "params":{"name":"whos_working_on","arguments":{"path":"src/a.rs"}}})).await;
    let call = read_json(&mut sout).await;
    let text = content_text(&call);
    assert_eq!(call["result"]["isError"], json!(false), "call: {call}");
    assert!(text.contains("rebase before you push"), "mail not piggybacked: {text}");
    assert!(text.contains("Fleet mail"), "piggyback must carry the coordination framing: {text}");

    // Mailbox now drained → a second identical call carries no piggyback.
    send_line(&mut sin, json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":"whos_working_on","arguments":{"path":"src/a.rs"}}})).await;
    let call2 = read_json(&mut sout).await;
    assert!(!content_text(&call2).contains("Fleet mail"), "empty mailbox must not piggyback: {call2}");

    // `report` must NOT piggyback (mail-after-report can prompt a redundant report — D-018).
    lead.call(Op::Send { to: 1, text: "one more note".into() }).await.unwrap();
    send_line(&mut sin, json!({"jsonrpc":"2.0","id":3,"method":"tools/call",
        "params":{"name":"report","arguments":{"ticket":"T-1","status":"done","summary":"finished"}}})).await;
    let call3 = read_json(&mut sout).await;
    assert!(!content_text(&call3).contains("Fleet mail"), "report must not piggyback: {call3}");
    // …and the mail it skipped is still queued for a later path to deliver.
    let still = store.take_mail(&fleetor_core::Party::Worker(1)).unwrap();
    assert_eq!(still.len(), 1);
    assert_eq!(still[0].body, "one more note");

    let _ = child.kill().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// Concatenate every text content block of an MCP tool-call result.
fn content_text(call: &Value) -> String {
    call["result"]["content"]
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// Run the shim in `stop-hook` mode for `slot`, feeding a realistic Stop payload
/// on stdin; return its stdout.
async fn run_stop_hook(sock: &std::path::Path, slot: u8) -> String {
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_fleetor-shim"))
        .arg("stop-hook")
        .env("FLEET_SOCKET", sock)
        .env("FLEETOR_SLOT", slot.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let payload = json!({
        "session_id": "test", "hook_event_name": "Stop", "stop_hook_active": false,
        "last_assistant_message": "done"
    });
    let mut sin = child.stdin.take().unwrap();
    sin.write_all(payload.to_string().as_bytes()).await.unwrap();
    drop(sin); // EOF so the hook's read_to_string returns
    let out = tokio::time::timeout(Duration::from_secs(5), child.wait_with_output())
        .await
        .expect("stop-hook timed out")
        .unwrap();
    String::from_utf8(out.stdout).unwrap()
}

/// Write one JSON value as a framed line to the shim's stdin.
async fn send_line(sin: &mut tokio::process::ChildStdin, v: Value) {
    let mut s = v.to_string();
    s.push('\n');
    sin.write_all(s.as_bytes()).await.unwrap();
    sin.flush().await.unwrap();
}

/// Read lines until one parses as a JSON object (skips any blank/noise).
async fn read_json(
    lines: &mut tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
) -> Value {
    loop {
        let line = tokio::time::timeout(Duration::from_secs(5), lines.next_line())
            .await
            .expect("shim response timed out")
            .unwrap()
            .expect("shim closed stdout");
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            return v;
        }
    }
}
