//! Phase 2 EXIT TEST (BUILDING §6): a scripted 3-agent conversation with a
//! mid-turn delivery, against fake-claude — no real tokens.
//!
//! Three fake-claude *processes* connect to the fleet hub over the unix socket:
//!  - worker-2 calls `ask_lead` and blocks; the lead (this test) sees the
//!    question via `await_events` and replies, unblocking it.
//!  - worker-1 `dm`s worker-3 while worker-3 is mid-turn; worker-3 receives it
//!    at its turn boundary via the drain path and echoes it back.
//! This exercises the whole messaging model end-to-end across process
//! boundaries: shim-style socket clients, blocking ask/reply, and async
//! peer mail with turn-boundary delivery.

use fleetor_core::wire::{Hello, LeadEventKind, Op, OpResult};
use fleetor_core::{Party, Store};
use fleetor_db::SqliteStore;
use fleetor_ipc::{Client, Transport, UnixTransport};
use fleetor_server::{Hub, HubConfig};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

fn fake_script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fake-claude/fake-claude.mjs")
        .canonicalize()
        .expect("fake-claude.mjs must exist")
}

/// Spawn one fake-claude worker process wired to the socket.
fn spawn_fake(
    sock: &std::path::Path,
    slot: u8,
    scenario: &str,
    extra_env: &[(&str, &str)],
) -> tokio::process::Child {
    let mut cmd = tokio::process::Command::new("node");
    cmd.arg(fake_script())
        .env("FAKE_CLAUDE_SCENARIO", scenario)
        .env("FLEET_SOCKET", sock)
        .env("FLEETOR_SLOT", slot.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    cmd.spawn().expect("spawn fake-claude")
}

async fn output(child: tokio::process::Child) -> String {
    let out = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output())
        .await
        .expect("fake worker timed out")
        .unwrap();
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[tokio::test]
async fn scripted_three_agent_conversation_with_mid_turn_delivery() {
    let dir = std::env::temp_dir().join(format!("fleetor-exit-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("fleet.sock");
    let transport = Arc::new(UnixTransport::new(&sock));
    let store = Arc::new(SqliteStore::open_in_memory().unwrap());
    let hub = Hub::new(store.clone(), HubConfig { slots: vec![1, 2, 3], ask_timeout: Duration::from_secs(20) });
    let listener = transport.bind().await.unwrap();
    tokio::spawn(hub.serve(listener));

    // worker-3 waits (polling the drain) for mail; worker-1 DMs it; worker-2 asks.
    let recv = spawn_fake(&sock, 3, "msg-recv", &[]);
    let dm = spawn_fake(&sock, 1, "msg-dm", &[("FAKE_DM_TO", "3"), ("FAKE_DM_BODY", "I own src/api this ticket")]);
    let ask = spawn_fake(&sock, 2, "msg-ask", &[]);

    // The lead (this test) fields the blocking question and answers it.
    let mut lead = Client::connect(&*transport, Hello::new(Party::Lead)).await.unwrap();
    let mut q = None;
    for _ in 0..3 {
        match lead.call(Op::AwaitEvents { timeout_ms: 5_000 }).await.unwrap() {
            OpResult::Events { events } if !events.is_empty() => {
                q = events.into_iter().next();
                break;
            }
            OpResult::Events { .. } => continue,
            other => panic!("expected events, got {other:?}"),
        }
    }
    let q = q.expect("worker-2 never asked the lead a question");
    assert_eq!(q.from, 2);
    assert!(matches!(q.kind, LeadEventKind::Question { .. }));
    lead.call(Op::Reply { event_id: q.id, text: "Use B".into() }).await.unwrap();

    // All three processes finish; check what each observed.
    let ask_out = output(ask).await;
    let recv_out = output(recv).await;
    let _dm_out = output(dm).await;

    assert!(ask_out.contains("lead answered: Use B"), "asker did not get the reply: {ask_out}");
    assert!(
        recv_out.contains("received: I own src/api this ticket"),
        "receiver did not get the mid-turn dm: {recv_out}"
    );

    // The hub logged the traffic: one dm mail event, and the ask's block/unblock.
    let mail = store.events_since(0).unwrap().into_iter().filter(|(_, e)| e.kind() == "mail").count();
    assert_eq!(mail, 1, "expected exactly the one dm mail event");
    let worker_states = store.events_since(0).unwrap().into_iter().filter(|(_, e)| e.kind() == "worker-state").count();
    assert_eq!(worker_states, 2, "ask_lead should flip worker-2 Working→Blocked→Working");

    let _ = std::fs::remove_dir_all(&dir);
}
