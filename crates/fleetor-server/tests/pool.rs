//! Phase 4i integration test: the **persistent worker pool** end-to-end against
//! fake-claude — no real tokens.
//!
//! Four standby workers are spawned idle at startup; the lead `broadcast`s one
//! message; it reaches **all four** (delivered to each idle worker over stdin, the
//! D-015 path) and shows up in every worker's transcript (the D-028 `worker-said`
//! events). Then the run drains cleanly.

use fleetor_cc::{AgentProcess, FakeClaude};
use fleetor_core::event::FleetEvent;
use fleetor_core::wire::{Hello, Op};
use fleetor_core::{Party, Store, Ticket};
use fleetor_db::SqliteStore;
use fleetor_ipc::{Client, UnixTransport};
use fleetor_server::{run_pool_fleet, HubConfig, WorkerSpec};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

fn fake_script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fake-claude/fake-claude.mjs")
        .canonicalize()
        .expect("fake-claude.mjs must exist")
}

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
async fn pool_spawns_four_standby_workers_and_broadcast_reaches_all() {
    let dir = std::env::temp_dir().join(format!("fleetor-pool-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("fleet.sock");

    let store: Arc<dyn Store> = Arc::new(SqliteStore::open_in_memory().unwrap());
    let transport = Arc::new(UnixTransport::new(&sock));

    // Four idle standby workers, one per slot.
    let workers: Vec<WorkerSpec> = (1..=4u8)
        .map(|slot| {
            let agent = FakeWithEnv {
                inner: FakeClaude {
                    script: fake_script(),
                    cwd: dir.clone(),
                    scenario: "standby".into(),
                },
                env: vec![
                    ("FLEET_SOCKET".into(), sock.to_string_lossy().into_owned()),
                    ("FLEETOR_SLOT".into(), slot.to_string()),
                ],
            };
            let ticket = Ticket {
                slot: Some(slot),
                ..Ticket::new(format!("worker-{slot}"), "standby", "standby")
            };
            WorkerSpec::fake(Box::new(agent), ticket, slot, Some(dir.join(format!("worker-{slot}.jsonl"))), 60)
        })
        .collect();

    // The lead connects, broadcasts once, and gives the pool a beat to deliver +
    // respond before ending the session (which drains the workers).
    let driver_transport = transport.clone();
    let driver = move || async move {
        let mut lead = Client::connect(&*driver_transport, Hello::new(Party::Lead)).await.unwrap();
        lead.call(Op::LeadBroadcast { text: "standby-check-ping".into() }).await.unwrap();
        tokio::time::sleep(Duration::from_secs(3)).await;
        Ok(())
    };

    let outcomes = tokio::time::timeout(
        Duration::from_secs(25),
        run_pool_fleet(store.clone(), transport, HubConfig::default(), workers, driver),
    )
    .await
    .expect("pool run timed out")
    .expect("pool run failed");

    assert_eq!(outcomes.len(), 4, "four standby workers should have run and drained");

    let events: Vec<FleetEvent> = store.events_since(0).unwrap().into_iter().map(|(_, e)| e).collect();

    // The broadcast fanned out to all four slots (one mail event each).
    let mail = events.iter().filter(|e| e.kind() == "mail").count();
    assert_eq!(mail, 4, "broadcast should queue mail to all four workers");

    // Every worker received the message — its transcript echoes it back.
    for slot in 1..=4u8 {
        let got = events.iter().any(|e| matches!(
            e,
            FleetEvent::WorkerSaid { slot: s, text, .. } if *s == slot && text.contains("standby-check-ping")
        ));
        assert!(got, "worker-{slot} never received the broadcast (no matching worker-said)");
    }

    let _ = std::fs::remove_dir_all(&dir);
}
