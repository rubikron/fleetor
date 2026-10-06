//! Goals and tasks over a real socket, against a dummy task store (D-100).
//!
//! Two pins carried from WP-05: task operations ask the app for nothing, and a
//! `fleet send` is byte-identical whether the task store is empty or full.

use fleetor_core::event::FleetEvent;
use fleetor_core::pane::{PaneEntry, PaneId, PaneState};
use fleetor_core::task::{ChainEntry, Kind, TaskRecord, TaskStatus};
use fleetor_core::wire::{Hello, Op, OpResult, TaskAction};
use fleetor_core::Store;
use fleetor_db::SqliteStore;
use fleetor_ipc::{Client, Transport, UnixTransport};
use fleetor_server::{AppCommand, BroadcastStore, DeliveryResult, Hub, TaskContext};
use std::collections::HashMap;
use std::path::PathBuf;
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

struct Fleet {
    transport: Arc<UnixTransport>,
    hub: Arc<Hub>,
    /// The run log.
    store: Arc<SqliteStore>,
    /// The dummy `tasks.db`, wrapped the way the app will wrap it.
    tasks: Arc<BroadcastStore>,
    writes: Writes,
    asks: Asks,
}

fn tempdir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fleetor-task-{}-{}",
        std::process::id(),
        fleetor_core::ids::new_id("t")
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A hub for `run` in `lineage` over the `tasks.db` in `dir`.
async fn start_hub_at(dir: &std::path::Path, run: &str, lineage: &str) -> Fleet {
    let roster: Vec<PaneEntry> =
        PaneId::roster(&SLOTS).into_iter().map(|p| PaneEntry::new(p, PaneState::Live)).collect();
    let transport = Arc::new(UnixTransport::new(dir.join(format!("{run}.sock"))));
    let store = Arc::new(SqliteStore::open_in_memory().unwrap());
    let tasks =
        Arc::new(BroadcastStore::new(Arc::new(SqliteStore::open(&dir.join("tasks.db")).unwrap())));
    let (app, writes, asks) = spawn_app(roster);
    let hub = Hub::with_tasks(
        store.clone(),
        app,
        tasks.clone(),
        TaskContext { run: run.into(), lineage: lineage.into() },
    );
    let listener = transport.bind().await.unwrap();
    tokio::spawn(hub.clone().serve(listener));
    Fleet { transport, hub, store, tasks, writes, asks }
}

async fn start_hub() -> Fleet {
    start_hub_at(&tempdir(), "run-1", "lin-1").await
}

async fn pane(fleet: &Fleet, pane: PaneId) -> Client {
    Client::connect(&*fleet.transport, Hello::for_pane(pane)).await.unwrap()
}

fn open_goal(outcome: &str) -> Op {
    Op::Task {
        action: TaskAction::Post {
            goal: true,
            outcome: outcome.into(),
            technical: vec![],
            vision: vec!["the parser is the product".into()],
            owner: None,
            instructions: None,
            parent: None,
            converges_on: None,
        },
    }
}

fn open_task(outcome: &str, owner: Option<u8>, parent: Option<u64>) -> Op {
    Op::Task {
        action: TaskAction::Post {
            goal: false,
            outcome: outcome.into(),
            technical: vec!["cargo test -p parser".into()],
            vision: vec!["one grammar".into()],
            owner: owner.map(PaneId::Worker),
            instructions: None,
            parent,
            converges_on: None,
        },
    }
}

fn set(task: u64, status: TaskStatus) -> Op {
    Op::Task { action: TaskAction::Update { task, status, note: None } }
}

fn number(result: OpResult) -> u64 {
    match result {
        OpResult::Recorded { record_id } => record_id.parse().expect("a task number"),
        other => panic!("expected a recorded entry, got {other:?}"),
    }
}

fn refusal(result: OpResult) -> String {
    match result {
        OpResult::Error { message } => message,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

fn records(result: OpResult) -> Vec<TaskRecord> {
    match result {
        OpResult::Board { tasks } => tasks,
        other => panic!("expected records, got {other:?}"),
    }
}

async fn list(client: &mut Client) -> Vec<TaskRecord> {
    records(client.call(Op::Task { action: TaskAction::List }).await.unwrap())
}

fn chain_len(fleet: &Fleet) -> usize {
    fleet.tasks.events_since(0).unwrap().len()
}

/// **The delivery-independence pin.** The same send, before and after ten
/// tasks exist (one of them the recipient's own, marked done), is the same
/// bytes at the pty and the same row in the run log, and the task operations
/// in between ask the app for nothing.
#[tokio::test]
async fn a_send_is_byte_identical_whether_the_task_store_is_empty_or_full() {
    let fleet = start_hub().await;
    let mut orch = pane(&fleet, PaneId::Orch).await;
    let mut worker = pane(&fleet, PaneId::Worker(2)).await;
    let send = || Op::Send { to: PaneId::Worker(2), text: "take the parser".into() };

    let before = orch.call(send()).await.unwrap();
    assert_eq!(fleet.asks.load(Ordering::Relaxed), 1, "one send is one ask");

    let goal = number(orch.call(open_goal("one grammar")).await.unwrap());
    let mut numbers = Vec::new();
    for n in 0..10 {
        numbers.push(number(orch.call(open_task(&format!("slice {n}"), Some(2), Some(goal))).await.unwrap()));
    }
    worker.call(set(numbers[3], TaskStatus::InProgress)).await.unwrap();
    worker.call(set(numbers[3], TaskStatus::Done)).await.unwrap();
    worker.call(Op::Task { action: TaskAction::Show { task: numbers[3] } }).await.unwrap();
    assert_eq!(list(&mut orch).await.len(), 11);
    assert_eq!(fleet.asks.load(Ordering::Relaxed), 1, "task operations asked the app for nothing");

    let after = orch.call(send()).await.unwrap();
    assert_eq!(fleet.asks.load(Ordering::Relaxed), 2, "still one ask per send");

    let bytes = fleet.writes.lock().unwrap().clone();
    assert_eq!(bytes.len(), 2, "task operations wrote nothing to any pty: {bytes:?}");
    assert_eq!(bytes[0], bytes[1]);

    let (OpResult::Delivered { accepted: a, detail: da, .. }, OpResult::Delivered { accepted: b, detail: db, .. }) =
        (&before, &after)
    else {
        panic!("expected two deliveries, got {before:?} / {after:?}")
    };
    assert_eq!((a, da), (b, db));

    let log = fleet.store.events_since(0).unwrap();
    let messages: Vec<_> = log
        .iter()
        .filter_map(|(_, e)| match e {
            FleetEvent::Message { from, to, body, accepted, .. } => Some((from, to, body, accepted)),
            _ => None,
        })
        .collect();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0], messages[1]);
    assert_eq!(log.len(), 2, "no task entry reached the run log: {log:?}");
}

/// The thin path: the operator opens a goal, orch opens a task under it, a
/// worker takes it up and marks it done, and `show` reads the chain back.
#[tokio::test]
async fn a_goal_is_split_taken_up_and_done_and_the_chain_says_who() {
    let fleet = start_hub().await;
    let mut orch = pane(&fleet, PaneId::Orch).await;
    let mut worker = pane(&fleet, PaneId::Worker(2)).await;

    let goal = number(fleet.hub.handle(PaneId::Operator, open_goal("one grammar")).await);
    let task = number(orch.call(open_task("nested groups parse", None, Some(goal))).await.unwrap());
    assert_eq!((goal, task), (1, 2), "numbers count up per target");

    assert_eq!(number(worker.call(set(task, TaskStatus::InProgress)).await.unwrap()), task);
    worker
        .call(Op::Task {
            action: TaskAction::Update {
                task,
                status: TaskStatus::Done,
                note: Some("cargo test passes".into()),
            },
        })
        .await
        .unwrap();

    let shown = records(orch.call(Op::Task { action: TaskAction::Show { task } }).await.unwrap());
    assert_eq!(shown.len(), 1);
    let record = &shown[0];
    assert_eq!(record.number, task);
    assert_eq!(record.creator, PaneId::Orch);
    assert_eq!(record.block.parent, Some(goal));
    assert_eq!(record.status, TaskStatus::Done);
    let owner = record.owner.as_ref().expect("taking it up made worker-2 the owner");
    assert_eq!((owner.pane, owner.run.as_str(), owner.lineage.as_str()), (PaneId::Worker(2), "run-1", "lin-1"));

    let said: Vec<(PaneId, &ChainEntry)> = record.chain.iter().map(|l| (l.from, &l.entry)).collect();
    assert!(matches!(said[0], (PaneId::Orch, ChainEntry::Opened { .. })), "{said:?}");
    assert!(matches!(said[1], (PaneId::Worker(2), ChainEntry::TakenUp { .. })), "{said:?}");
    assert!(
        matches!(said[2], (PaneId::Worker(2), ChainEntry::Status { status: TaskStatus::Done, note: Some(n) }) if n == "cargo test passes"),
        "{said:?}"
    );
    assert!(record.chain.iter().all(|l| l.run == "run-1" && l.lineage == "lin-1" && l.at > 0));

    let all = list(&mut worker).await;
    assert_eq!(all[0].block.kind, Kind::Goal);
    assert_eq!(all[0].creator, PaneId::Operator);
}

/// Every refusal names what to do instead, and none of them writes anything.
#[tokio::test]
async fn refusals_follow_who_is_asking_and_write_nothing() {
    let fleet = start_hub().await;
    let mut orch = pane(&fleet, PaneId::Orch).await;
    let mut owner = pane(&fleet, PaneId::Worker(2)).await;
    let mut peer = pane(&fleet, PaneId::Worker(3)).await;

    let goal = number(orch.call(open_goal("one grammar")).await.unwrap());
    let task = number(orch.call(open_task("nested groups parse", None, Some(goal))).await.unwrap());
    let found = number(peer.call(open_task("the tokenizer leaks", None, None)).await.unwrap());
    owner.call(set(task, TaskStatus::InProgress)).await.unwrap();
    let written = chain_len(&fleet);

    let why = refusal(peer.call(open_goal("my own vision")).await.unwrap());
    assert!(why.contains("orch and the operator") && why.contains("worker-3"), "{why}");
    let why = refusal(peer.call(open_task("hand this to a peer", Some(2), None)).await.unwrap());
    assert!(why.contains("starts unowned"), "{why}");

    let why = refusal(orch.call(open_task("serves nothing", Some(2), None)).await.unwrap());
    assert!(why.contains("--parent"), "{why}");
    let why = refusal(orch.call(open_task("under a task", None, Some(task))).await.unwrap());
    assert!(why.contains(&format!("#{task} is a task")), "{why}");
    let why = refusal(orch.call(open_task("under nothing", None, Some(99))).await.unwrap());
    assert!(why.contains("there is no #99"), "{why}");

    let why = refusal(peer.call(set(task, TaskStatus::Done)).await.unwrap());
    assert!(why.contains("only the owner") && why.contains("owner is worker-2"), "{why}");
    let why = refusal(orch.call(set(task, TaskStatus::Done)).await.unwrap());
    assert!(why.contains("only the owner"), "orch is not the owner either: {why}");
    let why = refusal(orch.call(set(goal, TaskStatus::Done)).await.unwrap());
    assert!(why.contains("is a goal"), "{why}");

    let why = refusal(owner.call(set(task, TaskStatus::Dropped)).await.unwrap());
    assert!(why.contains("opened by orch"), "the owner is not the creator: {why}");
    let why = refusal(owner.call(set(found, TaskStatus::Dropped)).await.unwrap());
    assert!(why.contains("opened by worker-3"), "{why}");

    let why = refusal(owner.call(set(99, TaskStatus::InProgress)).await.unwrap());
    assert!(why.contains("there is no #99") && why.contains("fleet task list"), "{why}");
    let why = refusal(owner.call(Op::Task { action: TaskAction::Show { task: 99 } }).await.unwrap());
    assert!(why.contains("there is no #99"), "{why}");

    assert_eq!(chain_len(&fleet), written, "a refusal writes nothing");

    // The same acts by someone with the authority.
    orch.call(set(found, TaskStatus::Dropped)).await.unwrap();
    peer.call(set(found, TaskStatus::Planned)).await.unwrap();
    peer.call(set(task, TaskStatus::InProgress)).await.unwrap();
    peer.call(set(task, TaskStatus::Done)).await.unwrap();
    let why = refusal(owner.call(set(task, TaskStatus::Done)).await.unwrap());
    assert!(why.contains("owner is worker-3"), "taking it up moved the owner: {why}");
}

/// A second session over the same `tasks.db`: the same records, numbers that
/// carry on, and an owner from the earlier lineage who can no longer say done.
#[tokio::test]
async fn a_new_session_over_the_same_store_reads_the_same_records() {
    let dir = tempdir();
    let before = {
        let first = start_hub_at(&dir, "run-1", "lin-1").await;
        let mut orch = pane(&first, PaneId::Orch).await;
        let mut worker = pane(&first, PaneId::Worker(2)).await;
        let goal = number(orch.call(open_goal("one grammar")).await.unwrap());
        let task = number(orch.call(open_task("nested groups parse", None, Some(goal))).await.unwrap());
        worker.call(set(task, TaskStatus::InProgress)).await.unwrap();
        list(&mut orch).await
    };

    let second = start_hub_at(&dir, "run-2", "lin-2").await;
    let mut orch = pane(&second, PaneId::Orch).await;
    let mut worker = pane(&second, PaneId::Worker(2)).await;
    assert_eq!(list(&mut orch).await, before);

    assert_eq!(number(orch.call(open_task("errors name the token", None, Some(1))).await.unwrap()), 3);

    let why = refusal(worker.call(set(2, TaskStatus::Done)).await.unwrap());
    assert!(why.contains("worker-2, earlier run"), "{why}");
    worker.call(set(2, TaskStatus::InProgress)).await.unwrap();
    worker.call(set(2, TaskStatus::Done)).await.unwrap();

    let record = list(&mut orch).await.remove(1);
    let runs: Vec<&str> = record.chain.iter().map(|l| l.run.as_str()).collect();
    assert_eq!(runs, vec!["run-1", "run-1", "run-2", "run-2"], "the chain shows where one run ended");
    assert_eq!(record.owner.unwrap().lineage, "lin-2");
}

/// The task store's own pipe: a follower from zero sees every chain entry once,
/// in order, whether it was written before or after it subscribed.
#[tokio::test]
async fn the_task_pipe_replays_without_gaps_and_the_run_log_stays_empty() {
    let fleet = start_hub().await;
    let mut orch = pane(&fleet, PaneId::Orch).await;
    let mut run_log = fleet.store.events_since(0).unwrap().len();
    assert_eq!(run_log, 0);

    let goal = number(orch.call(open_goal("one grammar")).await.unwrap());
    orch.call(open_task("before the follower", None, Some(goal))).await.unwrap();
    let mut follower = fleet.tasks.follow(0).unwrap();
    orch.call(open_task("after the follower", None, Some(goal))).await.unwrap();
    orch.call(set(2, TaskStatus::InProgress)).await.unwrap();

    let mut seen = Vec::new();
    for _ in 0..4 {
        let (seq, event) = follower.next().await.unwrap().expect("the pipe is open");
        let FleetEvent::Chain { task, .. } = event else { panic!("only chain entries: {event:?}") };
        seen.push((seq, task));
    }
    assert_eq!(seen, vec![(1, 1), (2, 2), (3, 3), (4, 2)]);

    run_log = fleet.store.events_since(0).unwrap().len();
    assert_eq!(run_log, 0, "task entries never enter the run log");
}

/// A hub built without a task store says so instead of recording anywhere else.
#[tokio::test]
async fn a_hub_without_a_task_store_refuses_task_operations() {
    let store = Arc::new(SqliteStore::open_in_memory().unwrap());
    let (app, _writes, asks) = spawn_app(vec![]);
    let hub = Hub::new(store.clone(), app);
    for op in [open_goal("one grammar"), Op::Task { action: TaskAction::List }] {
        let why = refusal(hub.handle(PaneId::Orch, op).await);
        assert!(why.contains("no task store attached"), "{why}");
    }
    assert!(store.events_since(0).unwrap().is_empty());
    assert_eq!(asks.load(Ordering::Relaxed), 0);
}
