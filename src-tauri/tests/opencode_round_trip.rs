//! **WP-32 phase 4: an opencode orch and an opencode worker complete a `fleet send`
//! round trip.** Real hub, real panes, real `fleet` binary, scratch layout.
//!
//! **Spends turns.** Ignored; run by hand with
//! `OPENCODE_ROUND_TRIP_MODEL=opencode-go/beam-free cargo test --test opencode_round_trip -- --ignored --nocapture`
//! after `cargo build -p fleetor-cli` at the workspace root.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use fleetor_core::event::FleetEvent;
use fleetor_core::pane::PaneId;
use fleetor_core::wire::Op;
use fleetor_core::Store;
use fleetor_db::SqliteStore;
use fleetor_server::{AppCommand, Hub};
use fleetor_shell::context_gauge::GaugeSources;
use fleetor_shell::deliver::spawn_delivery;
use fleetor_shell::placement::harness::by_name;
use fleetor_shell::placement::{self, Host, Layout, PaneSpec};
use fleetor_shell::prompts::PaneContext;
use fleetor_shell::pty::{Emit, PaneRegistry};
use tokio::sync::mpsc;

const WORD: &str = "PLUM-3141";

fn fleet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/debug/fleet")
}

fn messages(store: &Arc<dyn Store>) -> Vec<(PaneId, PaneId, String, bool)> {
    store
        .events_since(0)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(_, e)| match e {
            FleetEvent::Message { from, to, body, accepted, .. } => {
                Some((from, to, body, accepted))
            }
            _ => None,
        })
        .collect()
}

#[test]
#[ignore = "spends turns on the operator's opencode"]
fn an_opencode_orch_and_worker_complete_a_round_trip() {
    let model = std::env::var("OPENCODE_ROUND_TRIP_MODEL").expect("name a model");
    let harness = by_name("opencode").expect("opencode is registered");
    assert!(fleet_bin().is_file(), "build the fleet CLI first");

    let root = std::env::temp_dir().join(format!("fleetor-opencode-rt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let layout = Layout::under(root.join("state"));
    let target = root.join("repo");
    std::fs::create_dir_all(&target).unwrap();
    for args in
        [&["init", "-q", "-b", "main"][..], &["commit", "-q", "--allow-empty", "-m", "first"]]
    {
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(&target)
            .args([
                "-c",
                "user.email=rt@fleetor.test",
                "-c",
                "user.name=rt",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .status()
            .unwrap();
        assert!(ok.success());
    }

    let home = std::env::var_os("HOME").map(PathBuf::from);
    let host = Host {
        fleet_bin: Some(fleet_bin()),
        operator_logins: harness
            .read_operator_login(home.as_deref())
            .map(|l| vec![(harness.spec().name, l)])
            .unwrap_or_default(),
        operator_home: home,
        inherited_path: std::env::var("PATH").unwrap_or_default(),
        ..Host::bare()
    };
    let ctx = PaneContext::baked();

    let store: Arc<dyn Store> = Arc::new(SqliteStore::open_in_memory().unwrap());
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let (tx, rx) = mpsc::unbounded_channel::<AppCommand>();
    let log = root.join("panes.log");
    let sink = log.clone();
    let emit: Emit = Arc::new(move |channel: &str, payload: String| {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD.decode(&payload).unwrap_or_default();
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&sink) {
            let _ = writeln!(f, "[{channel}] {}", String::from_utf8_lossy(&bytes));
        }
    });
    let registry = Arc::new(PaneRegistry::new(emit, root.join("pids.json")));
    spawn_delivery(&rt, registry.clone(), rx, store.clone(), Arc::new(GaugeSources::default()));

    let sock = layout.socket();
    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
    let hub = Hub::new(store.clone(), tx);
    let transport = Arc::new(fleetor_ipc::UnixTransport::new(&sock));
    rt.spawn(hub.clone().run(transport));

    for spec in [
        PaneSpec::orch(harness).with_model(model.clone()),
        PaneSpec::worker(1, harness).with_model(model.clone()),
    ] {
        let pane = spec.pane();
        let placed = placement::place(spec, &layout, &host, &target, &ctx)
            .unwrap_or_else(|e| panic!("placing {pane}: {e}"));
        for (level, text) in &placed.notices {
            eprintln!("{pane} {level:?}: {text}");
        }
        registry.spawn(pane, placed.command, placed.harness, 40, 120).unwrap();
    }
    std::thread::sleep(Duration::from_secs(15));

    let ask = format!(
        "Use `fleet send worker-1` to ask worker-1 to reply to you with the word {WORD}. \
         When its reply arrives, send me `fleet send operator \"got {WORD}\"`."
    );
    let sent = rt.block_on(hub.handle(PaneId::Operator, Op::Send { to: PaneId::Orch, text: ask }));
    eprintln!("operator → orch: {sent:?}");

    let deadline = Instant::now() + Duration::from_secs(300);
    // A message to the operator is recorded, never typed, so it carries no `accepted`.
    let hop = |from: PaneId, to: PaneId| {
        messages(&store).into_iter().any(|(f, t, body, ok)| {
            f == from && t == to && (ok || to == PaneId::Operator) && body.contains(WORD)
        })
    };
    while Instant::now() < deadline && !hop(PaneId::Orch, PaneId::Operator) {
        std::thread::sleep(Duration::from_secs(2));
    }
    for (f, t, body, ok) in messages(&store) {
        eprintln!(
            "message {f} → {t} accepted={ok}: {}",
            body.chars().take(160).collect::<String>()
        );
    }
    registry.kill_all();
    eprintln!("pane output: {}", log.display());

    assert!(hop(PaneId::Orch, PaneId::Worker(1)), "orch never relayed to worker-1");
    assert!(hop(PaneId::Worker(1), PaneId::Orch), "worker-1 never replied to orch");
    assert!(hop(PaneId::Orch, PaneId::Operator), "orch never reported back");
}
