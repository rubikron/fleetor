//! WP-13 — `fleet handoff`, proved to be a report rather than a trigger.
//!
//! **The test this file exists for is
//! [`a_handoff_asks_the_app_for_nothing_and_changes_no_later_send`].** This verb
//! is the wake signal for a loop that does not exist yet, and the failure it
//! could grow into is the one `task.rs`'s tripwire list names: something reading
//! the log back and behaving differently because of what it found. A handoff is
//! only a claim for as long as nothing consults it, so that is counted here
//! rather than asserted in a comment — the app's asks do not move, and a `fleet
//! send` is byte-identical before and after one.
//!
//! The fake app is the same one `pane_messaging.rs` and `task_board.rs` use,
//! counting every [`AppCommand`] it receives. A separate file from both, for the
//! reason `task_board.rs` is separate: the message path's diff for this package
//! has to be empty, including its tests.

use fleetor_core::event::FleetEvent;
use fleetor_core::pane::{PaneEntry, PaneId, PaneState};
use fleetor_core::wire::{Hello, Op, OpResult};
use fleetor_core::Store;
use fleetor_db::SqliteStore;
use fleetor_ipc::{Client, Transport, UnixTransport};
use fleetor_server::{AppCommand, DeliveryResult, Hub};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

const SLOTS: [u8; 3] = [1, 2, 3];

type Writes = Arc<Mutex<Vec<(PaneId, String)>>>;
/// How many `AppCommand`s of any kind the app was asked to serve. A handoff must
/// not move this number.
type Asks = Arc<AtomicUsize>;

fn spawn_app(roster: Vec<PaneEntry>) -> (mpsc::UnboundedSender<AppCommand>, Writes, Asks) {
    let (tx, mut rx) = mpsc::unbounded_channel::<AppCommand>();
    let writes: Writes = Arc::new(Mutex::new(Vec::new()));
    let asks: Asks = Arc::new(AtomicUsize::new(0));
    let (sink, counter) = (writes.clone(), asks.clone());
    tokio::spawn(async move {
        let states: HashMap<PaneId, PaneState> = roster.iter().map(|e| (e.pane, e.state)).collect();
        while let Some(cmd) = rx.recv().await {
            counter.fetch_add(1, Ordering::Relaxed);
            let accept = |to: &PaneId, bytes: String| match states.get(to) {
                Some(state) if state.accepts_input() => {
                    sink.lock().unwrap().push((*to, bytes));
                    DeliveryResult::accepted()
                }
                Some(_) => DeliveryResult::rejected(format!("pane {to} is dead")),
                None => DeliveryResult::rejected(format!("no pane {to} is running")),
            };
            match cmd {
                AppCommand::Deliver { to, text, ack } => {
                    let _ = ack.send(accept(&to, text));
                }
                AppCommand::Command { to, command, ack } => {
                    let _ = ack.send(accept(&to, command));
                }
                AppCommand::Roster { ack } => {
                    let _ = ack.send(roster.clone());
                }
            }
        }
    });
    (tx, writes, asks)
}

async fn start_hub() -> (Arc<UnixTransport>, Arc<SqliteStore>, Writes, Asks) {
    let roster: Vec<PaneEntry> =
        PaneId::roster(&SLOTS).into_iter().map(|p| PaneEntry::new(p, PaneState::Live)).collect();
    let dir = std::env::temp_dir().join(format!(
        "fleetor-handoff-{}-{}",
        std::process::id(),
        fleetor_core::ids::new_id("h")
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let transport = Arc::new(UnixTransport::new(dir.join("fleet.sock")));
    let store = Arc::new(SqliteStore::open_in_memory().unwrap());
    let (app, writes, asks) = spawn_app(roster);
    let hub = Hub::new(store.clone(), app);
    let listener = transport.bind().await.unwrap();
    tokio::spawn(hub.serve(listener));
    (transport, store, writes, asks)
}

async fn pane(transport: &UnixTransport, pane: PaneId) -> Client {
    Client::connect(transport, Hello::for_pane(pane)).await.unwrap()
}

fn handoff() -> Op {
    Op::Handoff {
        built: "the parser accepts nested groups end to end".into(),
        evidence: vec!["cargo test -p parser".into(), "fleet/integration @ a1b2c3d".into()],
        open: vec!["the error messages are still the tokenizer's".into()],
    }
}

fn handoffs_in_log(store: &SqliteStore) -> Vec<FleetEvent> {
    store
        .events_since(0)
        .unwrap()
        .into_iter()
        .map(|(_, e)| e)
        .filter(|e| matches!(e, FleetEvent::Handoff { .. }))
        .collect()
}

/// **The independence pin.**
///
/// A handoff asks the app for nothing — it is the second op after `fleet task`
/// that never leaves the log — and a `fleet send` afterwards is byte-identical
/// to the same send before it. If anything ever consults "has the goal been
/// declared met" before doing something else, this is what catches it.
#[tokio::test]
async fn a_handoff_asks_the_app_for_nothing_and_changes_no_later_send() {
    let (transport, store, writes, asks) = start_hub().await;
    let mut orch = pane(&transport, PaneId::Orch).await;

    orch.call(Op::Send { to: PaneId::Worker(2), text: "take the parser".into() }).await.unwrap();
    assert_eq!(asks.load(Ordering::Relaxed), 1, "one send is one ask");
    let before = writes.lock().unwrap().clone();

    let asks_before_the_handoff = asks.load(Ordering::Relaxed);
    let result = orch.call(handoff()).await.unwrap();
    assert!(matches!(result, OpResult::Recorded { .. }), "{result:?}");
    assert_eq!(
        asks.load(Ordering::Relaxed),
        asks_before_the_handoff,
        "a handoff asked the app for nothing at all — nothing was typed anywhere",
    );
    assert_eq!(*writes.lock().unwrap(), before, "and nothing reached a pty");

    orch.call(Op::Send { to: PaneId::Worker(2), text: "take the parser".into() }).await.unwrap();
    let after = writes.lock().unwrap().clone();
    assert_eq!(after.len(), 2, "the second send still landed");
    assert_eq!(after[0], after[1], "a send is byte-identical before and after a handoff");

    // And the delivery events are the same shape too — the second send is not a
    // different kind of thing because the fleet has declared itself finished.
    let messages: Vec<FleetEvent> = store
        .events_since(0)
        .unwrap()
        .into_iter()
        .map(|(_, e)| e)
        .filter(|e| matches!(e, FleetEvent::Message { .. }))
        .collect();
    assert_eq!(messages.len(), 2);
    let body = |e: &FleetEvent| match e {
        FleetEvent::Message { body, accepted, .. } => (body.clone(), *accepted),
        other => panic!("expected a message, got {other:?}"),
    };
    assert_eq!(body(&messages[0]), body(&messages[1]));
}

/// One handoff is one event of its own kind, attributed, with its evidence and
/// its loose ends intact — and it is **not** a message. An orchestrator's
/// declaration rendered in the message record would be putting words in its
/// mouth: it said nothing to anybody.
#[tokio::test]
async fn a_handoff_appends_one_event_of_its_own_kind_and_no_message() {
    let (transport, store, _writes, _asks) = start_hub().await;
    let mut orch = pane(&transport, PaneId::Orch).await;

    let result = orch.call(handoff()).await.unwrap();
    let OpResult::Recorded { record_id } = result else { panic!("expected recorded: {result:?}") };
    assert!(record_id.starts_with("handoff-"), "the id says what it names: {record_id}");

    let logged = handoffs_in_log(&store);
    assert_eq!(logged.len(), 1, "one handoff, one row");
    let FleetEvent::Handoff { id, from, built, evidence, open } = &logged[0] else {
        panic!("expected a handoff")
    };
    assert_eq!(*id, record_id, "the id the caller was given is the id in the log");
    assert_eq!(*from, PaneId::Orch);
    assert!(built.starts_with("the parser accepts nested groups"));
    assert_eq!(evidence.len(), 2, "every line of evidence survives the round trip");
    assert_eq!(open.len(), 1);

    let others: Vec<FleetEvent> = store
        .events_since(0)
        .unwrap()
        .into_iter()
        .map(|(_, e)| e)
        .filter(|e| !matches!(e, FleetEvent::Handoff { .. }))
        .collect();
    assert!(others.is_empty(), "a handoff is one event and nothing else: {others:?}");
}

/// A handoff with nothing checkable behind it is refused at accept time, with a
/// sentence for the sender's stderr — the same refusal class as an empty task
/// block, and nothing is written when it happens.
#[tokio::test]
async fn a_handoff_with_no_evidence_is_refused_and_nothing_is_logged() {
    let (transport, store, _writes, _asks) = start_hub().await;
    let mut orch = pane(&transport, PaneId::Orch).await;

    let result = orch
        .call(Op::Handoff { built: "we finished".into(), evidence: vec![], open: vec![] })
        .await
        .unwrap();
    let OpResult::Error { message } = result else { panic!("expected a refusal: {result:?}") };
    assert!(message.contains("--evidence"), "the refusal names the flag to fix: {message}");
    assert!(handoffs_in_log(&store).is_empty(), "a refused handoff is not a record");
}
