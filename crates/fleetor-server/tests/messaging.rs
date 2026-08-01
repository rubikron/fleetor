//! Phase 2 hub routing — the messaging model proved in-process with real socket
//! clients (no worker processes yet; that's the fake-claude exit test). Covers
//! the whole surface: the `ask_lead`/`reply` blocking round-trip, async `dm`
//! with turn-boundary (`drain`) delivery, `broadcast` fan-out, and the lead's
//! `await_events` long-poll. Fast, deterministic, free.

use fleetor_core::wire::{Hello, LeadEventKind, Op, OpResult};
use fleetor_core::{Party, Store};
use fleetor_db::SqliteStore;
use fleetor_ipc::{Client, Transport, UnixTransport};
use fleetor_server::{Hub, HubConfig};
use std::sync::Arc;
use std::time::Duration;

/// Bring up a hub on a fresh temp socket; return (transport, store). The hub
/// task runs until the test ends.
async fn start_hub(slots: Vec<u8>, ask_timeout: Duration) -> (Arc<UnixTransport>, Arc<SqliteStore>) {
    let dir = std::env::temp_dir().join(format!(
        "fleetor-hub-{}-{}",
        std::process::id(),
        fleetor_core::ids::new_id("t")
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let transport = Arc::new(UnixTransport::new(dir.join("fleet.sock")));
    let store = Arc::new(SqliteStore::open_in_memory().unwrap());
    let hub = Hub::new(store.clone(), HubConfig { slots, ask_timeout });
    // Bind first so clients never race the listener.
    let listener = transport.bind().await.unwrap();
    tokio::spawn(hub.serve(listener));
    (transport, store)
}

async fn client(transport: &UnixTransport, party: Party) -> Client {
    Client::connect(transport, Hello::new(party)).await.unwrap()
}

fn count_kind(store: &SqliteStore, kind: &str) -> usize {
    store.events_since(0).unwrap().into_iter().filter(|(_, e)| e.kind() == kind).count()
}

/// The scripted 3-agent conversation (BUILDING §6 exit test, hub layer):
/// W2 asks the lead and blocks; the lead sees the question via `await_events`
/// and replies, unblocking W2. Meanwhile W1 DMs W3, which W3 receives at its
/// turn boundary via `drain`. W3 then broadcasts, reaching W1 and W2.
#[tokio::test]
async fn three_agent_conversation_with_ask_reply_dm_and_broadcast() {
    let (transport, store) = start_hub(vec![1, 2, 3], Duration::from_secs(30)).await;

    let mut lead = client(&transport, Party::Lead).await;
    let mut w1 = client(&transport, Party::Worker(1)).await;
    let mut w3 = client(&transport, Party::Worker(3)).await;

    // --- ask_lead / reply: W2 blocks on a question; the lead answers it. ---
    // W2 runs on its own task because ask_lead blocks until the reply lands.
    let ask_transport = transport.clone();
    let asker = tokio::spawn(async move {
        let mut w2 = client(&ask_transport, Party::Worker(2)).await;
        w2.call(Op::AskLead {
            question: "Approach A or B for the parser?".into(),
            options: Some(vec!["A".into(), "B".into()]),
        })
        .await
        .unwrap()
    });

    // Lead long-polls, receives exactly the question, and replies to it.
    let events = match lead.call(Op::AwaitEvents { timeout_ms: 5_000 }).await.unwrap() {
        OpResult::Events { events } => events,
        other => panic!("expected events, got {other:?}"),
    };
    assert_eq!(events.len(), 1, "the one pending question");
    let q = &events[0];
    assert_eq!(q.from, 2);
    assert!(matches!(q.kind, LeadEventKind::Question { .. }));
    assert_eq!(lead.call(Op::Reply { event_id: q.id.clone(), text: "Use B".into() }).await.unwrap(), OpResult::Ack);

    // W2's blocked ask now returns the real answer.
    let answer = asker.await.unwrap();
    assert_eq!(answer, OpResult::Answer { text: "Use B".into(), answered: true });

    // --- dm + turn-boundary delivery: W1 → W3, W3 drains at its turn end. ---
    assert_eq!(w1.call(Op::Dm { to: 3, text: "I own src/api this ticket".into() }).await.unwrap(), OpResult::Ack);
    let w3_mail = match w3.call(Op::DrainMail).await.unwrap() {
        OpResult::Mail { messages } => messages,
        other => panic!("expected mail, got {other:?}"),
    };
    assert_eq!(w3_mail.len(), 1);
    assert_eq!(w3_mail[0].from, Party::Worker(1));
    assert_eq!(w3_mail[0].body, "I own src/api this ticket");

    // Draining again yields nothing (delivered-once).
    assert_eq!(w3.call(Op::DrainMail).await.unwrap(), OpResult::Mail { messages: vec![] });

    // --- broadcast: W3 → all peers; W1 and W2 receive it, W3 does not. ---
    assert_eq!(w3.call(Op::Broadcast { text: "parser done, rebasing".into() }).await.unwrap(), OpResult::Ack);
    let w1_mail = match w1.call(Op::DrainMail).await.unwrap() {
        OpResult::Mail { messages } => messages,
        other => panic!("expected mail, got {other:?}"),
    };
    assert_eq!(w1_mail.len(), 1);
    assert_eq!(w1_mail[0].body, "parser done, rebasing");
    // W3 excluded from its own broadcast.
    assert_eq!(w3.call(Op::DrainMail).await.unwrap(), OpResult::Mail { messages: vec![] });

    // --- the log recorded it all: mail events + the ask's block/unblock. ---
    // 1 dm + 2 broadcast fan-out = 3 mail events.
    assert_eq!(count_kind(&store, "mail"), 3);
    // ask_lead flipped W2 Working→Blocked→Working = 2 worker-state events.
    assert_eq!(count_kind(&store, "worker-state"), 2);
}

/// An `ask_lead` with no reply returns the park answer rather than deadlocking
/// (handoff §5) — the AFK-lead safety valve.
#[tokio::test]
async fn ask_lead_times_out_to_a_park_answer() {
    let (transport, _store) = start_hub(vec![1], Duration::from_millis(150)).await;
    let mut w1 = client(&transport, Party::Worker(1)).await;
    let answer = w1
        .call(Op::AskLead { question: "anyone home?".into(), options: None })
        .await
        .unwrap();
    match answer {
        OpResult::Answer { answered, .. } => assert!(!answered, "a timeout is not a real answer"),
        other => panic!("expected a park answer, got {other:?}"),
    }
}

/// The worker/lead split is structural: a worker may not invoke a lead op, and
/// vice-versa (Tier-1.5 enforced at the seam).
#[tokio::test]
async fn party_op_split_is_enforced() {
    let (transport, _store) = start_hub(vec![1], Duration::from_secs(1)).await;

    let mut w1 = client(&transport, Party::Worker(1)).await;
    let r = w1.call(Op::AwaitEvents { timeout_ms: 10 }).await.unwrap();
    assert!(matches!(r, OpResult::Error { .. }), "worker cannot await_events");

    let mut lead = client(&transport, Party::Lead).await;
    let r = lead.call(Op::AskLead { question: "x".into(), options: None }).await.unwrap();
    assert!(matches!(r, OpResult::Error { .. }), "lead cannot ask_lead");
}
