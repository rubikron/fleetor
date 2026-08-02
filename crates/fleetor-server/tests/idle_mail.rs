//! D-015 integration test: **idle→stdin** mail delivery, end-to-end against
//! fake-claude — no real tokens.
//!
//! A worker ends its first turn with no report (it's idle, waiting for steering).
//! Mail is queued for its slot; the supervisor's idle-drain writes that mail
//! straight to the worker's stdin as a fresh turn, and the worker acts on it and
//! files a done report. This is the delivery path the turn-boundary Stop hook
//! can't cover (the hook already drained empty when the turn ended).

use fleetor_cc::{AgentProcess, FakeClaude};
use fleetor_core::envelope::{Envelope, MessageKind};
use fleetor_core::event::{FleetEvent, WorkerState};
use fleetor_core::report::ReportStatus;
use fleetor_core::{Party, Store, Ticket};
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

/// A fake worker carrying the fleet env a real wired worker would get.
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
async fn idle_mail_is_injected_over_stdin_as_a_fresh_turn() {
    let dir = std::env::temp_dir().join(format!("fleetor-idle-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("fleet.sock");
    let raw_log = dir.join("worker-1.jsonl");

    let store: Arc<dyn Store> = Arc::new(SqliteStore::open_in_memory().unwrap());
    let transport = Arc::new(UnixTransport::new(&sock));

    let slot = 1u8;

    // Queue steering mail for the worker BEFORE it runs. In the fake path there is
    // no Stop hook draining, so the supervisor's idle-drain is the sole consumer:
    // the mail waits in the queue until the worker's first (report-less) turn ends.
    store
        .save_mail(&Envelope::new(
            Party::Lead,
            Party::Worker(slot),
            MessageKind::Dm,
            "the upstream API shipped — use v2 of the endpoint",
        ))
        .unwrap();

    let agent = FakeWithEnv {
        inner: FakeClaude {
            script: fake_script(),
            cwd: dir.clone(),
            scenario: "idle-mail".into(),
        },
        env: vec![
            ("FLEET_SOCKET".into(), sock.to_string_lossy().into_owned()),
            ("FLEETOR_SLOT".into(), slot.to_string()),
        ],
    };
    let ticket = Ticket::new("T-501", "await steering", "Start, then wait for the lead's steering before finalizing.");
    let workers = vec![WorkerSpec::fake(Box::new(agent), ticket, slot, Some(raw_log.clone()), 20)];

    // The lead never answers a question here — the whole point is unprompted,
    // idle-time delivery.
    let lead = LeadPolicy::answering("n/a");

    let outcome = tokio::time::timeout(
        Duration::from_secs(25),
        run_fleet(store.clone(), transport, workers, HubConfig::default(), lead),
    )
    .await
    .expect("fleet run timed out")
    .expect("fleet run failed");

    // The ticket closed done — reached only because the idle mail drove a 2nd turn.
    assert!(
        matches!(outcome.workers[0].1, Outcome::Reported { status: ReportStatus::Done }),
        "expected a done report after idle injection, got {:?}",
        outcome.workers[0].1
    );

    // The framed mail reached the worker over stdin (a fresh turn), not the socket.
    let transcript = std::fs::read_to_string(&raw_log).expect("worker transcript");
    assert!(
        transcript.contains("idle mail received:") && transcript.contains("use v2 of the endpoint"),
        "worker never received the injected idle mail:\n{transcript}"
    );
    assert!(
        transcript.contains("Fleet mail"),
        "idle injection must carry the coordination framing:\n{transcript}"
    );

    // The worker was flipped to Idle around the wait — an honest, renderable beat.
    let saw_idle = store
        .events_since(0)
        .unwrap()
        .into_iter()
        .any(|(_, e)| matches!(e, FleetEvent::WorkerState { to: WorkerState::Idle, .. }));
    assert!(saw_idle, "the supervisor should have emitted a Working→Idle beat");

    let _ = std::fs::remove_dir_all(&dir);
}
