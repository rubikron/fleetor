//! Phase 4c exit test: the **live event bus**. The push side of the event log
//! must carry *exactly* what the persisted log holds — every event, in `seq`
//! order, no gaps, no duplicates — for both a subscriber that was live for the
//! whole run and one that joins late. Plus lag recovery from the DB.
//!
//! The driver is the real Phase 4a `run_fleet` against fake-claude (no tokens),
//! wrapped in a [`BroadcastStore`]: the same appends the supervisor and hub make
//! now fan out to the bus for free, which is the whole point of the decorator.

use fleetor_cc::{AgentProcess, FakeClaude};
use fleetor_core::event::FleetEvent;
use fleetor_core::{Store, Ticket};
use fleetor_db::SqliteStore;
use fleetor_ipc::UnixTransport;
use fleetor_server::{run_fleet, BroadcastStore, EventBus, HubConfig, LeadPolicy, WorkerSpec, BUS_CAPACITY};
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

fn worker(dir: &std::path::Path, sock: &std::path::Path, slot: u8, scenario: &str) -> FakeWithEnv {
    FakeWithEnv {
        inner: FakeClaude { script: fake_script(), cwd: dir.to_path_buf(), scenario: scenario.into() },
        env: vec![
            ("FLEET_SOCKET".into(), sock.to_string_lossy().into_owned()),
            ("FLEETOR_SLOT".into(), slot.to_string()),
        ],
    }
}

/// Drain a live follower until it has yielded `expect_at_least` events *and* has
/// caught up to `final_seq` (the persisted max after the run). Bounded so a bug
/// can't hang the suite.
async fn collect_until(
    follower: &mut fleetor_server::EventFollower,
    final_seq: i64,
) -> Vec<(i64, FleetEvent)> {
    let mut got = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while got.last().map(|(s, _)| *s).unwrap_or(0) < final_seq {
        match tokio::time::timeout_at(deadline, follower.next()).await {
            Ok(Ok(Some(ev))) => got.push(ev),
            Ok(Ok(None)) => break, // bus closed
            Ok(Err(e)) => panic!("follower error: {e}"),
            Err(_) => panic!("follower did not reach seq {final_seq}; got {got:?}"),
        }
    }
    got
}

/// A subscriber live for the entire run receives exactly the persisted log — same
/// seqs, same order, no gaps, no dupes.
#[tokio::test]
async fn live_subscriber_receives_exactly_the_persisted_log() {
    let dir = std::env::temp_dir().join(format!("fleetor-bus-live-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("fleet.sock");

    // Wrap the real store: keep the concrete handle for subscriptions, pass the
    // erased `Arc<dyn Store>` into the runner.
    let inner: Arc<dyn Store> = Arc::new(SqliteStore::open_in_memory().unwrap());
    let bcast = Arc::new(BroadcastStore::new(inner));
    let store: Arc<dyn Store> = bcast.clone();
    let transport = Arc::new(UnixTransport::new(&sock));

    // Subscribe *before* the run so the follower is live from seq 0.
    let mut follower = bcast.follow(0).unwrap();

    let slot = 2u8;
    let agent = worker(&dir, &sock, slot, "fleet-ask");
    let ticket = Ticket::new("T-4c1", "wire the bus", "Ask the lead, then proceed.");
    let workers = vec![WorkerSpec::fake(Box::new(agent), ticket, slot, None, 20)];
    let lead = LeadPolicy::answering("Use B").with_mail(slot, "W3 finished the API contract");

    tokio::time::timeout(
        Duration::from_secs(25),
        run_fleet(store.clone(), transport, workers, HubConfig::default(), lead),
    )
    .await
    .expect("fleet run timed out")
    .expect("fleet run failed");

    // Ground truth: the full persisted log.
    let persisted = store.events_since(0).unwrap();
    assert!(persisted.len() >= 3, "run should have produced several events: {persisted:?}");
    let final_seq = persisted.last().unwrap().0;

    // The live stream must match it byte-for-byte in (seq, event).
    let live = collect_until(&mut follower, final_seq).await;
    assert_eq!(live, persisted, "live bus diverged from the persisted log");

    // Strictly increasing seqs — no gaps, no dupes on the wire.
    for pair in live.windows(2) {
        assert!(pair[0].0 < pair[1].0, "seqs not strictly increasing: {pair:?}");
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// A subscriber that joins *after* the run still gets the whole history via the
/// DB snapshot, then would see live events — proving snapshot+subscribe has no
/// gap at the boundary.
#[tokio::test]
async fn late_subscriber_gets_full_history_then_live() {
    let dir = std::env::temp_dir().join(format!("fleetor-bus-late-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("fleet.sock");

    let inner: Arc<dyn Store> = Arc::new(SqliteStore::open_in_memory().unwrap());
    let bcast = Arc::new(BroadcastStore::new(inner));
    let store: Arc<dyn Store> = bcast.clone();
    let transport = Arc::new(UnixTransport::new(&sock));

    let slot = 1u8;
    let agent = worker(&dir, &sock, slot, "mcp-report");
    let ticket = Ticket::new("T-4c2", "report over mcp", "Do the work, then file via the fleet tool.");
    let workers = vec![WorkerSpec::fake(Box::new(agent), ticket, slot, None, 20)];

    tokio::time::timeout(
        Duration::from_secs(25),
        run_fleet(store.clone(), transport, workers, HubConfig::default(), LeadPolicy::answering("n/a")),
    )
    .await
    .expect("fleet run timed out")
    .expect("fleet run failed");

    let persisted = store.events_since(0).unwrap();
    let final_seq = persisted.last().unwrap().0;

    // Subscribe only now — history must come from the DB snapshot.
    let mut follower = bcast.follow(0).unwrap();
    let history = collect_until(&mut follower, final_seq).await;
    assert_eq!(history, persisted, "late subscriber's snapshot != persisted log");

    // The boundary is seamless: appending one more event surfaces live, exactly
    // once, right after the snapshot's last seq.
    let seq = store.append_event(&FleetEvent::Notice {
        level: fleetor_core::event::NoticeLevel::Info,
        text: "post-snapshot".into(),
    }).unwrap();
    let live = collect_until(&mut follower, seq).await;
    assert_eq!(live.len(), 1, "expected exactly the one post-snapshot event, got {live:?}");
    assert_eq!(live[0].0, seq);

    let _ = std::fs::remove_dir_all(&dir);
}

/// A follower that falls behind the bounded ring does not drop events: it detects
/// the lag and refills from the DB, still yielding every seq exactly once.
#[tokio::test]
async fn lagged_follower_recovers_every_event_from_the_db() {
    let inner: Arc<dyn Store> = Arc::new(SqliteStore::open_in_memory().unwrap());
    let bcast = Arc::new(BroadcastStore::new(inner));
    let store: Arc<dyn Store> = bcast.clone();

    // Subscribe, then flood well past the ring capacity *without* reading — the
    // receiver is guaranteed to lag.
    let mut follower = bcast.follow(0).unwrap();
    let total = BUS_CAPACITY + 250;
    let mut last_seq = 0;
    for i in 0..total {
        last_seq = store
            .append_event(&FleetEvent::Notice { level: fleetor_core::event::NoticeLevel::Info, text: format!("e{i}") })
            .unwrap();
    }

    // Despite the overrun, the follower yields all `total` events, contiguous and
    // in order — recovered from the durable log.
    let got = collect_until(&mut follower, last_seq).await;
    assert_eq!(got.len(), total, "lagged follower lost events");
    for (i, (seq, _)) in got.iter().enumerate() {
        assert_eq!(*seq, i as i64 + 1, "seq {seq} out of order at index {i}");
    }
}

/// Sanity: a bare `EventBus` (no store wrapping) fans a published event to every
/// current subscriber — the primitive under `BroadcastStore`.
#[tokio::test]
async fn bare_bus_fans_out_to_all_subscribers() {
    let store: Arc<dyn Store> = Arc::new(SqliteStore::open_in_memory().unwrap());
    let bus = EventBus::new();
    let mut a = bus.follow(store.clone(), 0).unwrap();
    let mut b = bus.follow(store.clone(), 0).unwrap();

    let ev = FleetEvent::Notice { level: fleetor_core::event::NoticeLevel::Warn, text: "hi".into() };
    bus.publish(7, ev.clone());

    assert_eq!(a.next().await.unwrap(), Some((7, ev.clone())));
    assert_eq!(b.next().await.unwrap(), Some((7, ev)));
}
