//! WP-05 — the task board, proved to be a diary rather than a dispatcher.
//!
//! **The test this file exists for is
//! [`a_send_is_byte_identical_whether_the_board_is_empty_or_full`].** The
//! requirements call it the delivery-independence pin, and it is the one that
//! stops this package regrowing what D-030 deleted: the old `Assign` op returned
//! `Ack` while nothing ever ran, and `wire.rs:16` records that `Assign` and
//! `FleetStatus` "are the seed of the ticket system growing back". A board is
//! only a record for as long as nothing consults it, so that claim is checked
//! here rather than asserted in a comment.
//!
//! The fake app is deliberately the *same* one `pane_messaging.rs` uses, with one
//! addition: it counts every [`AppCommand`] it receives, so "the task ops never
//! ask the app for anything" is a number rather than a reading of the code.
//!
//! A separate file from `pane_messaging.rs` on purpose. That file is the message
//! path's own test, and this package's diff on the message path has to be empty —
//! including its tests.

use fleetor_core::event::FleetEvent;
use fleetor_core::pane::{PaneEntry, PaneId, PaneState};
use fleetor_core::task::{TaskChange, TaskStatus};
use fleetor_core::wire::{Hello, Op, OpResult, TaskAction};
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
/// How many `AppCommand`s of any kind the app was asked to serve. The board must
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
        "fleetor-task-{}-{}",
        std::process::id(),
        fleetor_core::ids::new_id("t")
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

/// A complete block, differing only in its outcome so a board of ten is readable.
fn post(outcome: &str, worker: u8) -> Op {
    Op::Task {
        action: TaskAction::Post {
            outcome: outcome.into(),
            technical: vec!["cargo test -p parser".into()],
            semantic: vec!["one grammar".into()],
            worker: PaneId::Worker(worker),
            instructions: None,
            parent: None,
            converges_on: None,
        },
    }
}

fn task_id(result: OpResult) -> String {
    match result {
        OpResult::Recorded { record_id } => record_id,
        other => panic!("expected a recorded claim, got {other:?}"),
    }
}

fn board(result: OpResult) -> Vec<fleetor_core::task::TaskEntry> {
    match result {
        OpResult::Board { tasks } => tasks,
        other => panic!("expected the board, got {other:?}"),
    }
}

fn tasks_in_log(store: &SqliteStore) -> Vec<FleetEvent> {
    store
        .events_since(0)
        .unwrap()
        .into_iter()
        .map(|(_, e)| e)
        .filter(|e| matches!(e, FleetEvent::Task { .. }))
        .collect()
}

/// **The delivery-independence pin.**
///
/// A `fleet send` to worker-2 with an empty board and the same send with ten
/// blocks on it — one of them assigned to worker-2 and marked `done` — must be
/// the same event, byte for byte, at the pty and in the log. If any code path
/// ever consults task state before a delivery, this is what catches it.
///
/// It also counts the app's asks: the ten posts, the update and the two list
/// calls between the two sends contribute **zero**. A board that reached a
/// terminal, or a delivery that read the board, would move one of these numbers.
#[tokio::test]
async fn a_send_is_byte_identical_whether_the_board_is_empty_or_full() {
    let (transport, store, writes, asks) = start_hub().await;
    let mut orch = pane(&transport, PaneId::Orch).await;

    // 1 — a send against an empty board.
    let before = orch
        .call(Op::Send { to: PaneId::Worker(2), text: "take the parser".into() })
        .await
        .unwrap();
    assert_eq!(asks.load(Ordering::Relaxed), 1, "one send is one ask");
    let empty_board_bytes = writes.lock().unwrap().clone();

    // 2 — ten blocks, one of them worker-2's own, and a claim on it.
    let asks_before_the_board = asks.load(Ordering::Relaxed);
    let mut ids = Vec::new();
    for n in 0..10 {
        ids.push(task_id(orch.call(post(&format!("slice {n}"), 2)).await.unwrap()));
    }
    orch.call(Op::Task {
        action: TaskAction::Update {
            task: ids[3].clone(),
            status: Some(TaskStatus::Done),
            note: Some("cargo test passes".into()),
        },
    })
    .await
    .unwrap();
    assert_eq!(board(orch.call(Op::Task { action: TaskAction::List }).await.unwrap()).len(), 10);
    assert_eq!(
        asks.load(Ordering::Relaxed),
        asks_before_the_board,
        "ten posts, an update and a list asked the app for nothing at all",
    );

    // 3 — the same send again.
    let after = orch
        .call(Op::Send { to: PaneId::Worker(2), text: "take the parser".into() })
        .await
        .unwrap();
    assert_eq!(asks.load(Ordering::Relaxed), asks_before_the_board + 1, "still one ask per send");

    let all_bytes = writes.lock().unwrap().clone();
    assert_eq!(all_bytes.len(), 2, "the board wrote nothing to any pty: {all_bytes:?}");
    assert_eq!(
        all_bytes[1], empty_board_bytes[0],
        "a send to a worker with ten blocks must be byte-identical to one with none",
    );

    // The answers agree on everything but the per-message id.
    let (OpResult::Delivered { accepted: a, detail: da, .. }, OpResult::Delivered { accepted: b, detail: db, .. }) =
        (&before, &after)
    else {
        panic!("expected two deliveries, got {before:?} / {after:?}")
    };
    assert_eq!((a, da), (b, db), "the delivery outcome did not depend on the board");

    // And the two logged messages are the same message twice.
    let messages: Vec<(PaneId, PaneId, String, bool)> = store
        .events_since(0)
        .unwrap()
        .into_iter()
        .filter_map(|(_, e)| match e {
            FleetEvent::Message { from, to, body, accepted, .. } => Some((from, to, body, accepted)),
            _ => None,
        })
        .collect();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0], messages[1], "the record does not depend on the board either");
}

/// A posted block reaches the log as one attributed, timestamped claim, and the
/// id the poster got back is the id the board answers with. No second table: the
/// only thing written is a `FleetEvent::Task`.
#[tokio::test]
async fn a_posted_block_is_one_event_and_the_board_reads_it_back() {
    let (transport, store, _writes, _asks) = start_hub().await;
    let mut orch = pane(&transport, PaneId::Orch).await;

    let id = task_id(
        orch.call(Op::Task {
            action: TaskAction::Post {
                outcome: "the parser accepts nested groups".into(),
                technical: vec!["cargo test -p parser".into(), "the CLI round-trips one".into()],
                semantic: vec!["serves the one-grammar part of the vision".into()],
                worker: PaneId::Worker(2),
                instructions: Some("start from the existing tokenizer".into()),
                parent: None,
                converges_on: Some("task-later".into()),
            },
        })
        .await
        .unwrap(),
    );
    assert!(id.starts_with("task-"), "ids say what they are: {id}");

    let logged = tasks_in_log(&store);
    assert_eq!(logged.len(), 1, "one post is one event");
    let FleetEvent::Task { task, from, at, change } = &logged[0] else { panic!("expected a task") };
    assert_eq!(task, &id);
    assert_eq!(*from, PaneId::Orch, "the claim is attributed");
    assert!(*at > 0, "and timestamped");
    assert!(matches!(change, TaskChange::Posted { .. }));

    let tasks = board(orch.call(Op::Task { action: TaskAction::List }).await.unwrap());
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, id);
    assert_eq!(tasks[0].block.worker, PaneId::Worker(2));
    assert_eq!(tasks[0].block.technical.len(), 2);
    assert_eq!(tasks[0].block.converges_on.as_deref(), Some("task-later"));
    assert_eq!(tasks[0].status, TaskStatus::Planned, "a fresh block is planned, never inferred");
}

/// Ownership is social, not coded. A peer may append a claim to a block that is
/// not theirs — that is what makes WP-06's review possible without a permission
/// system — and the log records who said it.
#[tokio::test]
async fn any_pane_may_update_any_block_and_the_log_names_them() {
    let (transport, _store, _writes, _asks) = start_hub().await;
    let mut orch = pane(&transport, PaneId::Orch).await;
    let mut peer = pane(&transport, PaneId::Worker(3)).await;

    let id = task_id(orch.call(post("the parser lands", 2)).await.unwrap());
    peer.call(Op::Task {
        action: TaskAction::Update {
            task: id.clone(),
            status: Some(TaskStatus::Dropped),
            note: Some("duplicate of the tokenizer block".into()),
        },
    })
    .await
    .unwrap();

    let tasks = board(orch.call(Op::Task { action: TaskAction::List }).await.unwrap());
    assert_eq!(tasks[0].status, TaskStatus::Dropped);
    assert_eq!(tasks[0].block.worker, PaneId::Worker(2), "the block is still worker-2's");
    assert_eq!(tasks[0].updates[0].from, PaneId::Worker(3), "worker-3 said this, and it shows");
}

/// Referential validation, not a gate: an update has to have a block to be a
/// claim *about*. Nothing is written when it does not — a typo'd id would
/// otherwise vanish into the log unread — and nothing about *which* status may
/// follow which is checked, here or anywhere.
#[tokio::test]
async fn an_update_to_a_block_that_is_not_there_is_refused_and_writes_nothing() {
    let (transport, store, _writes, _asks) = start_hub().await;
    let mut orch = pane(&transport, PaneId::Orch).await;

    let result = orch
        .call(Op::Task {
            action: TaskAction::Update {
                task: "task-typo".into(),
                status: Some(TaskStatus::Done),
                note: None,
            },
        })
        .await
        .unwrap();
    let OpResult::Error { message } = result else { panic!("expected a refusal, got {result:?}") };
    assert!(message.contains("fleet task list"), "the refusal says how to find the ids: {message}");
    assert!(tasks_in_log(&store).is_empty(), "a refused update writes nothing");

    // A block with no criteria is refused the same way, and for the same reason:
    // this is input validation at the boundary, not permission.
    let empty = orch
        .call(Op::Task {
            action: TaskAction::Post {
                outcome: "something good".into(),
                technical: vec![],
                semantic: vec!["the vision".into()],
                worker: PaneId::Worker(1),
                instructions: None,
                parent: None,
                converges_on: None,
            },
        })
        .await
        .unwrap();
    assert!(matches!(empty, OpResult::Error { .. }), "got {empty:?}");
    assert!(tasks_in_log(&store).is_empty());
}

/// No enforced transitions, at the socket rather than in a unit test: a worker
/// who claimed `done` and then found a failing criterion has to be able to walk
/// it back, and every claim stays on the record.
#[tokio::test]
async fn a_status_may_go_backwards_and_the_whole_trail_survives() {
    let (transport, _store, _writes, _asks) = start_hub().await;
    let mut orch = pane(&transport, PaneId::Orch).await;
    let mut worker = pane(&transport, PaneId::Worker(2)).await;

    let id = task_id(orch.call(post("the parser lands", 2)).await.unwrap());
    for (status, note) in [
        (Some(TaskStatus::Claimed), Some("starting now")),
        (Some(TaskStatus::Done), Some("cargo test passes")),
        (Some(TaskStatus::Claimed), Some("crit 2 fails after all")),
        (None, Some("reworking the tokenizer first")),
    ] {
        worker
            .call(Op::Task {
                action: TaskAction::Update {
                    task: id.clone(),
                    status,
                    note: note.map(str::to_string),
                },
            })
            .await
            .unwrap();
    }

    let tasks = board(orch.call(Op::Task { action: TaskAction::List }).await.unwrap());
    assert_eq!(tasks[0].status, TaskStatus::Claimed, "the last status claimed wins, backwards or not");
    assert_eq!(tasks[0].updates.len(), 4, "nothing is folded away");
    assert_eq!(tasks[0].updates[3].status, None, "a note-only claim keeps the status it found");
}

/// The board is the log and nothing else. A second hub over the same store — a
/// restart, in effect — computes the same board, which it could not do if any
/// part of it lived in memory.
#[tokio::test]
async fn a_restarted_hub_replays_the_same_board_from_the_log_alone() {
    let (transport, store, _writes, _asks) = start_hub().await;
    let mut orch = pane(&transport, PaneId::Orch).await;
    let first = task_id(orch.call(post("slice one", 1)).await.unwrap());
    let second = task_id(orch.call(post("slice two", 2)).await.unwrap());
    orch.call(Op::Task {
        action: TaskAction::Update {
            task: second.clone(),
            status: Some(TaskStatus::Claimed),
            note: None,
        },
    })
    .await
    .unwrap();
    let before = board(orch.call(Op::Task { action: TaskAction::List }).await.unwrap());

    // A brand-new hub, a brand-new app, the same store.
    let dir = std::env::temp_dir()
        .join(format!("fleetor-task-restart-{}", fleetor_core::ids::new_id("t")));
    std::fs::create_dir_all(&dir).unwrap();
    let restarted = Arc::new(UnixTransport::new(dir.join("fleet.sock")));
    let (app, _writes, _asks) = spawn_app(Vec::new());
    let listener = restarted.bind().await.unwrap();
    tokio::spawn(Hub::new(store.clone(), app).serve(listener));

    let mut after_restart = pane(&restarted, PaneId::Worker(4)).await;
    let after = board(after_restart.call(Op::Task { action: TaskAction::List }).await.unwrap());

    assert_eq!(after, before, "the board is a fold over the log, not state a hub holds");
    assert_eq!(after[0].id, first, "post order survives the replay");
    assert_eq!(after[1].id, second);
    assert_eq!(after[1].status, TaskStatus::Claimed);
}

/// Tree links are ids and nothing validates them into a graph. A block may name a
/// parent that does not exist, and two blocks may name each other — the board
/// still answers, because the shape is somebody's note about how the work fits
/// together rather than a workflow the fleet executes.
#[tokio::test]
async fn tree_links_are_ids_and_a_cycle_is_still_a_board() {
    let (transport, _store, _writes, _asks) = start_hub().await;
    let mut orch = pane(&transport, PaneId::Orch).await;

    let linked = |parent: &str| Op::Task {
        action: TaskAction::Post {
            outcome: "a slice".into(),
            technical: vec!["cargo test".into()],
            semantic: vec!["the vision".into()],
            worker: PaneId::Worker(1),
            instructions: None,
            parent: Some(parent.to_string()),
            converges_on: None,
        },
    };

    let dangling = task_id(orch.call(linked("task-nobody-posted")).await.unwrap());
    let first = task_id(orch.call(linked("placeholder")).await.unwrap());
    let second = task_id(orch.call(linked(&first)).await.unwrap());

    let tasks = board(orch.call(Op::Task { action: TaskAction::List }).await.unwrap());
    assert_eq!(tasks.len(), 3, "every block is on the board, links or no links");
    assert_eq!(tasks[0].id, dangling);
    assert_eq!(
        tasks[0].block.parent.as_deref(),
        Some("task-nobody-posted"),
        "a link to nothing is kept as written — validating it away would lose what was meant",
    );
    assert_eq!(tasks[2].block.parent.as_deref(), Some(first.as_str()));
    assert_ne!(first, second);
}
