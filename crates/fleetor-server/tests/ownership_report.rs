//! Phase 3 hub surface: `report()` over MCP (D-008 promoted) and the ownership
//! tools `whos_working_on` / `claim_file` / `backlog_add` (deferred from Phase 2
//! per D-015). Proven in-process with real socket clients — the same layer the
//! worker shims reach in Phase 4. Fast, deterministic, free.

use fleetor_core::report::{Report, ReportStatus};
use fleetor_core::wire::{Hello, LeadEventKind, Op, OpResult};
use fleetor_core::{LeaseGrant, Party, Store, Ticket};
use fleetor_db::SqliteStore;
use fleetor_ipc::{Client, Transport, UnixTransport};
use fleetor_server::{Hub, HubConfig};
use std::sync::Arc;
use std::time::Duration;

async fn start_hub(slots: Vec<u8>) -> (Arc<UnixTransport>, Arc<SqliteStore>) {
    let dir = std::env::temp_dir().join(format!(
        "fleetor-own-{}-{}",
        std::process::id(),
        fleetor_core::ids::new_id("t")
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let transport = Arc::new(UnixTransport::new(dir.join("fleet.sock")));
    let store = Arc::new(SqliteStore::open_in_memory().unwrap());
    let hub = Hub::new(store.clone(), HubConfig { slots, ask_timeout: Duration::from_secs(5) });
    let listener = transport.bind().await.unwrap();
    tokio::spawn(hub.serve(listener));
    (transport, store)
}

async fn client(transport: &UnixTransport, party: Party) -> Client {
    Client::connect(transport, Hello::new(party)).await.unwrap()
}

fn report(ticket: &str) -> Report {
    Report {
        ticket: ticket.into(),
        status: ReportStatus::Done,
        summary: "did the thing".into(),
        branch: Some("ticket/T-1".into()),
        diffstat: None,
        gate: None,
        decisions: vec![],
        questions: vec![],
        risks: vec![],
        followups: vec![],
    }
}

/// A worker files a structured report over the socket: it is persisted, a
/// `report-filed` event is logged, and a lead long-poll learns of it.
#[tokio::test]
async fn worker_files_a_report_over_mcp() {
    let (transport, store) = start_hub(vec![1, 2]).await;
    store.upsert_ticket(&Ticket::new("T-1", "t", "b")).unwrap();

    let mut lead = client(&transport, Party::Lead).await;
    let mut w1 = client(&transport, Party::Worker(1)).await;

    assert_eq!(w1.call(Op::Report { report: report("T-1") }).await.unwrap(), OpResult::Ack);

    // The lead sees a notice that the report landed.
    let events = match lead.call(Op::AwaitEvents { timeout_ms: 2_000 }).await.unwrap() {
        OpResult::Events { events } => events,
        other => panic!("expected events, got {other:?}"),
    };
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].from, 1);
    assert!(matches!(events[0].kind, LeadEventKind::Notice { .. }));

    // And a report-filed event is in the log.
    let filed = store
        .events_since(0)
        .unwrap()
        .into_iter()
        .filter(|(_, e)| e.kind() == "report-filed")
        .count();
    assert_eq!(filed, 1);
}

/// `claim_file` grants a free path and denies one another slot holds;
/// `whos_working_on` reflects the holder.
#[tokio::test]
async fn claim_file_grants_then_denies_and_reports_the_holder() {
    let (transport, _store) = start_hub(vec![1, 2]).await;
    let mut w1 = client(&transport, Party::Worker(1)).await;
    let mut w2 = client(&transport, Party::Worker(2)).await;

    // W1 claims src/api.rs for its ticket → granted.
    let g = w1.call(Op::ClaimFile { path: "src/api.rs".into(), ticket: "T-1".into() }).await.unwrap();
    assert_eq!(g, OpResult::Claim { grant: LeaseGrant::Granted });

    // Idempotent for the same holder.
    let g2 = w1.call(Op::ClaimFile { path: "src/api.rs".into(), ticket: "T-1".into() }).await.unwrap();
    assert_eq!(g2, OpResult::Claim { grant: LeaseGrant::Granted });

    // W2 claiming the same path is denied, told who holds it.
    let d = w2.call(Op::ClaimFile { path: "src/api.rs".into(), ticket: "T-2".into() }).await.unwrap();
    match d {
        OpResult::Claim { grant: LeaseGrant::Denied { held_by } } => {
            assert_eq!(held_by.slot, 1);
            assert_eq!(held_by.ticket, "T-1");
        }
        other => panic!("expected a denial, got {other:?}"),
    }

    // whos_working_on shows W1 as the owner.
    let owners = match w2.call(Op::WhosWorkingOn { path: "src/api.rs".into() }).await.unwrap() {
        OpResult::Owners { owners } => owners,
        other => panic!("expected owners, got {other:?}"),
    };
    assert_eq!(owners.len(), 1);
    assert_eq!(owners[0].slot, 1);

    // A free path has no owners.
    let none = match w1.call(Op::WhosWorkingOn { path: "src/free.rs".into() }).await.unwrap() {
        OpResult::Owners { owners } => owners,
        other => panic!("expected owners, got {other:?}"),
    };
    assert!(none.is_empty());
}

/// `backlog_add` parks an out-of-scope discovery, persisted for the board.
#[tokio::test]
async fn backlog_add_persists_the_discovery() {
    let (transport, store) = start_hub(vec![1]).await;
    let mut w1 = client(&transport, Party::Worker(1)).await;

    assert_eq!(
        w1.call(Op::BacklogAdd { text: "the auth module also leaks a fd".into() }).await.unwrap(),
        OpResult::Ack
    );

    let backlog = store.list_backlog().unwrap();
    assert_eq!(backlog.len(), 1);
    assert_eq!(backlog[0].text, "the auth module also leaks a fd");
    assert_eq!(backlog[0].added_by, Party::Worker(1));
}
