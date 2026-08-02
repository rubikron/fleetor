//! Phase 4d exit test: the **orchestrator-as-lead**, driving a *dynamic* fleet.
//!
//! Unlike 4a's `run_fleet` (a fixed worker list spawned up front), here the lead
//! seat is external and the worker is spawned *because the lead called `assign`*
//! over the hub. A fake orchestrator (a lead `Client`) dispatches a ticket,
//! long-polls `await_events`, answers the worker's `ask_lead`, steers it with
//! mid-turn mail, and watches for the report notice — the scripted `LeadPolicy`
//! loop replaced by a real lead session over the wire. No tokens: the real
//! Opus-in-the-seat run is a deferred `--real` gate.

use anyhow::{ensure, Result};
use fleetor_cc::{AgentProcess, FakeClaude};
use fleetor_core::event::TicketState;
use fleetor_core::report::ReportStatus;
use fleetor_core::wire::{Hello, LeadEventKind, Op, OpResult};
use fleetor_core::{Party, Store, Ticket};
use fleetor_db::SqliteStore;
use fleetor_ipc::{Client, Transport, UnixTransport};
use fleetor_server::{run_dynamic_fleet, HubConfig, Outcome, WorkerFactory, WorkerSpec};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn fake_script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fake-claude/fake-claude.mjs")
        .canonicalize()
        .expect("fake-claude.mjs must exist")
}

/// A fake worker carrying the fleet env a wired real worker would get, so its
/// socket half can find the hub. (Mirrors the runner test's helper.)
struct FakeWithEnv {
    inner: FakeClaude,
    env: Vec<(String, String)>,
}

impl AgentProcess for FakeWithEnv {
    fn command(&self) -> Command {
        let mut cmd = self.inner.command();
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        cmd
    }
    fn label(&self) -> String {
        self.inner.label()
    }
}

#[tokio::test]
async fn orchestrator_assigns_over_the_hub_and_drives_a_worker_to_done() {
    let dir = std::env::temp_dir().join(format!("fleetor-orch-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("fleet.sock");

    let store: Arc<dyn Store> = Arc::new(SqliteStore::open_in_memory().unwrap());
    let transport = Arc::new(UnixTransport::new(&sock));

    // The factory the runner uses to turn an assigned ticket into a worker. In
    // the live fleet this builds a wired real worker; here a fake that speaks the
    // socket, scenario keyed to ask the lead then report.
    let factory: WorkerFactory = {
        let dir = dir.clone();
        let sock = sock.clone();
        Arc::new(move |ticket: &Ticket| {
            let slot = ticket.slot.unwrap_or(1);
            let agent = FakeWithEnv {
                inner: FakeClaude { script: fake_script(), cwd: dir.clone(), scenario: "fleet-ask".into() },
                env: vec![
                    ("FLEET_SOCKET".into(), sock.to_string_lossy().into_owned()),
                    ("FLEETOR_SLOT".into(), slot.to_string()),
                ],
            };
            let raw = dir.join(format!("worker-{slot}.jsonl"));
            WorkerSpec::fake(Box::new(agent), ticket.clone(), slot, Some(raw), 20)
        })
    };

    // The ticket the orchestrator will dispatch — slot 2, must be assigned, not
    // pre-spawned.
    let ticket = Ticket::new("T-4d1", "wire the fleet", "Ask the lead which approach, then proceed.")
        .with_files(vec!["src/wire.rs".into()]);
    let ticket = Ticket { slot: Some(2), ..ticket };

    // The fake orchestrator session: assign, then supervise over the lead tools.
    let driver = {
        let transport = transport.clone();
        let ticket = ticket.clone();
        move || async move {
            let mut lead = Client::connect(&*transport, Hello::new(Party::Lead)).await?;

            // Dispatch — the worker does not exist until this lands.
            let acked = lead.call(Op::Assign { ticket }).await?;
            ensure!(matches!(acked, OpResult::Ack), "assign should ack, got {acked:?}");

            // Supervise over the lead tools: answer the worker's blocking question,
            // steer it with mid-turn mail, and poll `fleet_status` until the ticket
            // is done on the board (works regardless of the worker's report channel).
            let deadline = Instant::now() + Duration::from_secs(20);
            let mut done = false;
            while !done && Instant::now() < deadline {
                if let OpResult::Events { events } =
                    lead.call(Op::AwaitEvents { timeout_ms: 300 }).await?
                {
                    for ev in events {
                        if let LeadEventKind::Question { .. } = ev.kind {
                            lead.call(Op::Reply { event_id: ev.id, text: "Use B".into() }).await?;
                            lead.call(Op::Send { to: 2, text: "W3 finished the API contract".into() }).await?;
                        }
                    }
                }
                if let OpResult::Status { board } = lead.call(Op::FleetStatus).await? {
                    done = board.iter().any(|t| t.id == "T-4d1" && t.state == TicketState::Done);
                }
            }
            ensure!(done, "orchestrator never saw the ticket reach done via fleet_status");
            Result::<()>::Ok(())
        }
    };

    let workers = tokio::time::timeout(
        Duration::from_secs(30),
        run_dynamic_fleet(store.clone(), transport.clone(), HubConfig::default(), factory, driver),
    )
    .await
    .expect("dynamic fleet timed out")
    .expect("dynamic fleet failed");

    // The assign-spawned worker reported done.
    assert_eq!(workers.len(), 1, "exactly one worker was assigned");
    assert_eq!(workers[0].0, 2, "it ran in the assigned slot");
    assert!(
        matches!(workers[0].1, Outcome::Reported { status: ReportStatus::Done }),
        "expected a done report, got {:?}",
        workers[0].1
    );

    // The board — the same store `fleet_status` reads — shows the ticket done.
    let board = store.tickets().unwrap();
    assert!(
        board.iter().any(|t| t.id == "T-4d1" && t.state == TicketState::Done),
        "ticket should be done on the board: {board:?}"
    );
    // The full loop ran, not just assign→done: the orchestrator answered the
    // worker's ask_lead and steered it — provable by the mail it sent landing in
    // the log (the lead only sends after seeing a Question).
    let mail = store.events_since(0).unwrap().into_iter().filter(|(_, e)| e.kind() == "mail").count();
    assert!(mail >= 1, "orchestrator should have sent steering mail after answering the ask");

    let _ = std::fs::remove_dir_all(&dir);
}

/// Phase 4f: the lead can yank a busy worker. A worker on the `hang` scenario
/// consumes its assignment and never reports; the lead `interrupt`s it, and the
/// runner ends its run as `Interrupted` — proving the kill reached the *sync*
/// supervisor (via the published pid) before the idle watchdog fired, and that a
/// deliberate stream-close is not mislabelled `Crashed`.
#[tokio::test]
async fn lead_interrupt_yanks_a_busy_worker() {
    let dir = std::env::temp_dir().join(format!("fleetor-int-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("fleet.sock");
    let store: Arc<dyn Store> = Arc::new(SqliteStore::open_in_memory().unwrap());
    let transport = Arc::new(UnixTransport::new(&sock));

    // Workers hang (consume the assignment, never report) — so only an interrupt
    // ends them. wall_secs is high so the idle watchdog does not fire first.
    let factory: WorkerFactory = {
        let dir = dir.clone();
        let sock = sock.clone();
        Arc::new(move |ticket: &Ticket| {
            let slot = ticket.slot.unwrap_or(1);
            let agent = FakeWithEnv {
                inner: FakeClaude { script: fake_script(), cwd: dir.clone(), scenario: "hang".into() },
                env: vec![
                    ("FLEET_SOCKET".into(), sock.to_string_lossy().into_owned()),
                    ("FLEETOR_SLOT".into(), slot.to_string()),
                ],
            };
            WorkerSpec::fake(Box::new(agent), ticket.clone(), slot, None, 30)
        })
    };

    let ticket = Ticket { slot: Some(2), ..Ticket::new("T-int", "hang then get yanked", "Consume the assignment and hang.") };

    let driver = {
        let transport = transport.clone();
        let ticket = ticket.clone();
        move || async move {
            let mut lead = Client::connect(&*transport, Hello::new(Party::Lead)).await?;
            let acked = lead.call(Op::Assign { ticket }).await?;
            ensure!(matches!(acked, OpResult::Ack), "assign should ack, got {acked:?}");

            // Wait until the worker is actually in progress (its pid is published on
            // spawn), then yank it.
            let started = poll_until(&mut lead, "T-int", TicketState::InProgress, Duration::from_secs(15)).await?;
            ensure!(started, "worker never reached in-progress to interrupt");

            let acked = lead.call(Op::Interrupt { slot: 2 }).await?;
            ensure!(matches!(acked, OpResult::Ack), "interrupt should ack, got {acked:?}");

            // Wait until the interrupt takes effect (worker dies → ticket Failed)
            // before returning, so the runner's shutdown never races the command.
            let yanked = poll_until(&mut lead, "T-int", TicketState::Failed, Duration::from_secs(10)).await?;
            ensure!(yanked, "interrupt did not end the worker");
            Result::<()>::Ok(())
        }
    };

    let workers = tokio::time::timeout(
        Duration::from_secs(30),
        run_dynamic_fleet(store.clone(), transport.clone(), HubConfig::default(), factory, driver),
    )
    .await
    .expect("interrupt fleet timed out")
    .expect("interrupt fleet failed");

    assert_eq!(workers.len(), 1, "one worker was assigned");
    assert_eq!(workers[0].0, 2, "it ran in the assigned slot");
    assert_eq!(
        workers[0].1,
        Outcome::Interrupted,
        "a yanked worker ends interrupted, not crashed/timed-out: {:?}",
        workers[0].1
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Poll `fleet_status` until `ticket` reaches `want` or the deadline passes.
async fn poll_until(
    lead: &mut Client,
    ticket: &str,
    want: TicketState,
    within: Duration,
) -> Result<bool> {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if let OpResult::Status { board } = lead.call(Op::FleetStatus).await? {
            if board.iter().any(|t| t.id == ticket && t.state == want) {
                return Ok(true);
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Ok(false)
}

/// A static fleet (no runner attached to the hub) rejects `assign` rather than
/// dropping it silently — the error the orchestrator would see, and the guard
/// that keeps 4d's dynamic path from being assumed elsewhere.
#[tokio::test]
async fn assign_on_a_static_hub_is_a_clean_error() {
    use fleetor_server::Hub;

    let dir = std::env::temp_dir().join(format!("fleetor-static-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("fleet.sock");
    let transport = Arc::new(UnixTransport::new(&sock));
    let store: Arc<dyn Store> = Arc::new(SqliteStore::open_in_memory().unwrap());

    let hub = Hub::new(store.clone(), HubConfig::default());
    let listener = transport.bind().await.unwrap();
    let hub_task = tokio::spawn(hub.serve(listener));

    let mut lead = Client::connect(&*transport, Hello::new(Party::Lead)).await.unwrap();
    let ticket = Ticket { slot: Some(1), ..Ticket::new("T-x", "t", "b") };
    let r = lead.call(Op::Assign { ticket }).await.unwrap();
    match r {
        OpResult::Error { message } => assert!(message.contains("does not accept dynamic assignment"), "{message}"),
        other => panic!("expected an error, got {other:?}"),
    }

    hub_task.abort();
    let _ = std::fs::remove_dir_all(&dir);
}
