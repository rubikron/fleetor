//! Phase 4a integration test: the **multi-worker fleet runner** end-to-end
//! against fake-claude — no real tokens.
//!
//! It proves the seam Phases 1–3 left open: one worker, driven over stdin by the
//! supervisor, *simultaneously* reaches the hub over the socket to `ask_lead`,
//! and receives the lead's mid-turn mail — all orchestrated by `run_fleet`
//! (hub + supervisor-on-a-blocking-thread + lead loop). The real-CC version of
//! this (shim + Stop hook) is the 4a live confirmation gate.

use fleetor_cc::{AgentProcess, FakeClaude};
use fleetor_core::report::ReportStatus;
use fleetor_core::{Store, Ticket};
use fleetor_db::SqliteStore;
use fleetor_ipc::UnixTransport;
use fleetor_server::{run_fleet, HubConfig, LeadPolicy, Outcome, WorkerSpec};
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

/// A fake worker that also carries the fleet env a real wired worker would get —
/// so its socket-speaking half knows where the hub is. Keeps `FakeClaude` itself
/// untouched (env-injection is a test concern, not a seam concern).
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
async fn runner_wires_one_worker_ask_lead_and_mid_turn_mail() {
    let dir = std::env::temp_dir().join(format!("fleetor-runner-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("fleet.sock");
    let raw_log = dir.join("worker-2.jsonl");

    let store: Arc<dyn Store> = Arc::new(SqliteStore::open_in_memory().unwrap());
    let transport = Arc::new(UnixTransport::new(&sock));

    let slot = 2u8;
    let agent = FakeWithEnv {
        inner: FakeClaude {
            script: fake_script(),
            cwd: dir.clone(),
            scenario: "fleet-ask".into(),
        },
        env: vec![
            ("FLEET_SOCKET".into(), sock.to_string_lossy().into_owned()),
            ("FLEETOR_SLOT".into(), slot.to_string()),
        ],
    };
    let ticket = Ticket::new("T-401", "wire the fleet", "Ask the lead which approach, then proceed.");
    let workers = vec![WorkerSpec::fake(Box::new(agent), ticket, slot, Some(raw_log.clone()), 20)];

    let lead = LeadPolicy::answering("Use B")
        .with_mail(slot, "W3 finished the API contract you were waiting on");

    let outcome = tokio::time::timeout(
        Duration::from_secs(25),
        run_fleet(store.clone(), transport, workers, HubConfig::default(), lead),
    )
    .await
    .expect("fleet run timed out")
    .expect("fleet run failed");

    // The lead fielded exactly one question and sent one piece of mid-turn mail.
    assert_eq!(outcome.questions_answered, 1, "lead should have answered one ask_lead");
    assert_eq!(outcome.mail_sent, 1, "lead should have sent one piece of mail");

    // The worker's ticket closed done via the supervisor's report ingestion.
    assert_eq!(outcome.workers.len(), 1);
    assert_eq!(outcome.workers[0].0, slot);
    assert!(
        matches!(outcome.workers[0].1, Outcome::Reported { status: ReportStatus::Done }),
        "expected a done report, got {:?}",
        outcome.workers[0].1
    );

    // Both channels landed: the worker saw the lead's answer over the socket, and
    // drained the lead's mid-turn mail. Its transcript records both.
    let transcript = std::fs::read_to_string(&raw_log).expect("worker transcript");
    assert!(transcript.contains("lead answered: Use B"), "worker never got the ask reply:\n{transcript}");
    assert!(
        transcript.contains("mail received: W3 finished the API contract"),
        "worker never drained the mid-turn mail:\n{transcript}"
    );

    // The hub logged exactly the one piece of mail.
    let mail = store.events_since(0).unwrap().into_iter().filter(|(_, e)| e.kind() == "mail").count();
    assert_eq!(mail, 1, "expected exactly one mail event in the log");

    let _ = std::fs::remove_dir_all(&dir);
}
