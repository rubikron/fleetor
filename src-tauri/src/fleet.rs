//! Phase 4e-2 — the **live fleet, embedded in the shell**.
//!
//! 4e-1 embedded the observability core (store + live bus + follower→webview) and
//! animated it with a scripted `demo`. 4e-2 removes the stand-in and lets a *real*
//! orchestrator drive: the hub is now bound **dynamic** (via
//! [`run_dynamic_fleet`]), so a worker is spawned the moment the lead calls
//! `assign` over the socket. The lead seat is the operator's real `claude` TUI in
//! the pty ([`crate::pty`]), which connects as [`Party::Lead`](fleetor_core::Party)
//! through the shim — this backend never fills that seat itself.
//!
//! **What is real here:** the store, the live event bus, the follower→UI pump, and
//! a dynamic hub that turns each `assign` into a supervised worker via the
//! [`factory`](build_factory). Worker/hub/supervisor writes stream to the UI over
//! the same 4c seam, unchanged.
//!
//! **Token posture (4e-2 gate):** the worker backend is [`fake`](WorkerBackend)
//! by default — a `fake-claude` that dials the socket and drives the loop for
//! free, so the wire can be proven without DeepSeek spend. Setting
//! `FLEETOR_WORKER_BACKEND=real` (with a `DEEPSEEK_API_KEY`) swaps in real Flash
//! workers. Either way the lead is the operator's own Opus, whose spend the UI
//! gates behind an explicit "start session" confirm before the pty is spawned.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use fleetor_cc::spawn::{FleetWiring, WorkerConfig};
use fleetor_cc::{AgentProcess, FakeClaude};
use fleetor_core::event::{FleetEvent, NoticeLevel, TicketState};
use fleetor_core::ticket::Ticket;
use fleetor_core::Store;
use fleetor_db::SqliteStore;
use fleetor_ipc::UnixTransport;
use fleetor_server::{run_dynamic_fleet, BroadcastStore, HubConfig, WorkerFactory, WorkerSpec};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Notify;

/// Emitted for every appended event, in `seq` order, the moment it persists.
const EVENT_FLEET: &str = "fleet://event";

/// Default per-ticket wall-clock budget for a worker (seconds).
const WORKER_WALL_SECS: u64 = 300;

/// One event as the webview sees it: the `seq` cursor plus the flattened
/// [`FleetEvent`] (its `#[serde(tag = "type")]` discriminator carries through, so
/// the UI matches on `type`).
#[derive(Serialize, Clone)]
struct WireEvent {
    seq: i64,
    #[serde(flatten)]
    event: FleetEvent,
}

/// The board + cursor a freshly-mounted UI seeds from. The live feed itself
/// arrives entirely over [`EVENT_FLEET`] (the follower replays history from 0),
/// so this carries only what events cannot: the tickets' full metadata.
#[derive(Serialize)]
pub struct BootSnapshot {
    board: Vec<Ticket>,
    latest_seq: i64,
}

/// The fleet's live configuration, surfaced to the top bar so it shows *facts*
/// (the real target, branch, and worker backend) instead of placeholders.
#[derive(Serialize, Clone)]
pub struct FleetConfig {
    /// The repo the fleet operates on (display path — the scratch repo in 4e-2).
    target: String,
    /// That repo's current git branch (best-effort).
    branch: String,
    /// `"fake"` (proof) or `"flash"` (real DeepSeek workers).
    worker_backend: String,
    /// The model in the lead seat — the operator's own Opus.
    lead_model: String,
    /// A short, honest label for the quality gate workers pass through.
    gate: String,
}

/// The live backend, created once by [`fleet_bootstrap`] and kept for the app's
/// lifetime. Owns the tokio runtime the follower, hub, and workers run on.
struct Fleet {
    _rt: tokio::runtime::Runtime,
    store: Arc<dyn Store>,
    /// Fired on shutdown to end the dynamic runner's driver so it drains workers
    /// and stops the hub cleanly.
    shutdown: Arc<Notify>,
    config: FleetConfig,
}

/// Managed Tauri state: at most one embedded fleet.
#[derive(Default)]
pub struct FleetState(Mutex<Option<Fleet>>);

/// State root, out of the user's repo so `rm -rf ~/.fleetor/_shell` fully undoes
/// it (Tier-1 boundary).
pub(crate) fn shell_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".fleetor").join("_shell")
}

/// The scratch repo the fleet operates on. A throwaway git repo under the shell
/// dir — real work, zero blast radius on any of the operator's real code.
pub fn scratch_repo() -> PathBuf {
    shell_dir().join("repo")
}

/// The fleet unix socket the shim (lead and workers) dial.
pub(crate) fn socket_path() -> PathBuf {
    shell_dir().join("fleet.sock")
}

/// Start the embedded fleet (idempotent) and return the board snapshot.
///
/// First call: opens the store, wraps it in the live bus, spawns the follower
/// pump, ensures the scratch repo exists, and binds the **dynamic** hub (so an
/// `assign` from the lead spawns a worker). Later calls (e.g. React StrictMode's
/// double-mount) find it already running and just return a fresh snapshot.
#[tauri::command]
pub fn fleet_bootstrap(
    app: AppHandle,
    state: State<'_, FleetState>,
) -> Result<BootSnapshot, String> {
    let mut guard = state.0.lock().map_err(|e| e.to_string())?;
    if let Some(fleet) = guard.as_ref() {
        return snapshot(&fleet.store);
    }

    let dir = shell_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("create shell dir: {e}"))?;
    let repo = ensure_scratch_repo()?;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("start runtime: {e}"))?;

    // The observability core: real store, wrapped once so every append publishes.
    let bcast = Arc::new(BroadcastStore::new(Arc::new(
        SqliteStore::open(&dir.join("state.db")).map_err(|e| format!("open store: {e}"))?,
    )));
    let store: Arc<dyn Store> = bcast.clone();

    spawn_follower(&rt, bcast.clone(), app);

    // Resolve the worker backend once, announcing the choice on the feed so the
    // token posture is visible, never silent.
    let backend = resolve_backend(&store, &repo);
    let config = fleet_config_for(&backend, &repo);
    let shutdown = spawn_dynamic_fleet(&rt, store.clone(), backend);

    let snap = snapshot(&store)?;
    *guard = Some(Fleet { _rt: rt, store, shutdown, config });
    Ok(snap)
}

/// Pump every appended event to the webview, oldest-first then live. Started once;
/// runs for the app's lifetime.
fn spawn_follower(rt: &tokio::runtime::Runtime, bcast: Arc<BroadcastStore>, app: AppHandle) {
    rt.spawn(async move {
        let mut follower = match bcast.follow(0) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("fleet: follower failed to start: {e}");
                return;
            }
        };
        while let Ok(Some((seq, event))) = follower.next().await {
            if app.emit(EVENT_FLEET, WireEvent { seq, event }).is_err() {
                break; // webview gone
            }
        }
    });
}

/// Bind the **dynamic** hub and run it for the app's lifetime, spawning one
/// supervised worker per `assign` the lead sends. Returns the shutdown handle:
/// the runner's `driver` is a pure lifetime gate that resolves when it fires, at
/// which point [`run_dynamic_fleet`] drains the workers and stops the hub.
///
/// The lead seat stays empty here — the real orchestrator (the pty `claude`)
/// fills it over the socket. A bind failure degrades gracefully: the shell stays
/// observable, only live assignment is unavailable.
fn spawn_dynamic_fleet(
    rt: &tokio::runtime::Runtime,
    store: Arc<dyn Store>,
    backend: WorkerBackend,
) -> Arc<Notify> {
    let sock = socket_path();
    let _ = std::fs::remove_file(&sock); // clear a stale socket from a prior run
    let transport = Arc::new(UnixTransport::new(&sock));
    let factory = build_factory(backend);

    let shutdown = Arc::new(Notify::new());
    let driver_gate = shutdown.clone();
    rt.spawn(async move {
        // Driver = lifetime gate: the runner keeps the hub up and dispatches
        // assigns until the app shuts down. It never occupies the lead seat.
        let driver = move || async move {
            driver_gate.notified().await;
            Ok(())
        };
        match run_dynamic_fleet(store, transport, HubConfig::default(), factory, driver).await {
            Ok(outcomes) => eprintln!("fleet: runner stopped ({} worker(s) drained)", outcomes.len()),
            Err(e) => eprintln!("fleet: dynamic runner error: {e}"),
        }
    });
    shutdown
}

/// The board as it stands now, straight from the store (the source of truth the
/// UI refetches whenever it sees a ticket move).
#[tauri::command]
pub fn fleet_board(state: State<'_, FleetState>) -> Result<Vec<Ticket>, String> {
    let guard = state.0.lock().map_err(|e| e.to_string())?;
    let fleet = guard.as_ref().ok_or("fleet not bootstrapped")?;
    fleet.store.tickets().map_err(|e| e.to_string())
}

/// The live fleet configuration for the top bar (real target/branch/backend).
#[tauri::command]
pub fn fleet_config(state: State<'_, FleetState>) -> Result<FleetConfig, String> {
    let guard = state.0.lock().map_err(|e| e.to_string())?;
    let fleet = guard.as_ref().ok_or("fleet not bootstrapped")?;
    Ok(fleet.config.clone())
}

/// Put a ticket on the board as `Assigned`. A real action through the store, so
/// the move streams to the UI over the event bus like any other. Note: this only
/// seeds the board — the *live* dispatch to a worker happens when the lead calls
/// `assign` over the socket (which the dynamic hub turns into a spawned worker).
#[tauri::command]
pub fn fleet_assign(state: State<'_, FleetState>, ticket: Ticket) -> Result<(), String> {
    let guard = state.0.lock().map_err(|e| e.to_string())?;
    let fleet = guard.as_ref().ok_or("fleet not bootstrapped")?;
    let from = ticket.state;
    let assigned = Ticket { state: TicketState::Assigned, ..ticket };
    fleet.store.upsert_ticket(&assigned).map_err(|e| e.to_string())?;
    fleet
        .store
        .append_event(&FleetEvent::TicketState {
            ticket: assigned.id.clone(),
            from,
            to: TicketState::Assigned,
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Fire the shutdown gate so the dynamic runner drains workers and stops the hub.
/// Best-effort, called on window close alongside the pty teardown.
pub fn shutdown(state: &FleetState) {
    if let Ok(guard) = state.0.lock() {
        if let Some(fleet) = guard.as_ref() {
            fleet.shutdown.notify_one();
        }
    }
}

fn snapshot(store: &Arc<dyn Store>) -> Result<BootSnapshot, String> {
    Ok(BootSnapshot {
        board: store.tickets().map_err(|e| e.to_string())?,
        latest_seq: store.latest_seq().map_err(|e| e.to_string())?,
    })
}

// --- worker backend + factory -------------------------------------------------

/// Which kind of worker the factory spawns per assigned ticket.
#[derive(Clone)]
pub enum WorkerBackend {
    /// A `fake-claude` (node script) that dials the socket and drives the loop for
    /// free — the proof path, no DeepSeek spend.
    Fake { script: PathBuf },
    /// A real, fully-wired DeepSeek Flash `claude` worker — costs tokens.
    Real { api_key: String },
}

impl WorkerBackend {
    fn label(&self) -> &'static str {
        match self {
            WorkerBackend::Fake { .. } => "fake",
            WorkerBackend::Real { .. } => "flash",
        }
    }
}

/// Decide the worker backend from the environment, announcing the choice (and any
/// fallback) on the feed so the token posture is never a silent surprise.
///
/// `FLEETOR_WORKER_BACKEND=real` opts into real Flash workers; it requires a
/// `DEEPSEEK_API_KEY` (env or the repo's `.env`) and falls back to `fake` with a
/// warning if the key is missing. Anything else selects `fake`.
fn resolve_backend(store: &Arc<dyn Store>, repo: &Path) -> WorkerBackend {
    let want_real = std::env::var("FLEETOR_WORKER_BACKEND")
        .map(|v| v.eq_ignore_ascii_case("real"))
        .unwrap_or(false);

    if want_real {
        match load_api_key(repo) {
            Ok(api_key) => {
                note(store, NoticeLevel::Info, "worker backend: real DeepSeek Flash (costs tokens)");
                return WorkerBackend::Real { api_key };
            }
            Err(e) => note(
                store,
                NoticeLevel::Warn,
                &format!("FLEETOR_WORKER_BACKEND=real but no DEEPSEEK_API_KEY ({e}); using fake workers"),
            ),
        }
    }

    let script = fake_script_path();
    note(
        store,
        NoticeLevel::Info,
        "worker backend: fake (free proof path — set FLEETOR_WORKER_BACKEND=real for live Flash)",
    );
    WorkerBackend::Fake { script }
}

/// Build the [`WorkerFactory`] the dynamic runner uses to turn an assigned ticket
/// into a supervised worker. Real workers get [`FleetWiring`] (shim MCP + Stop
/// hook), materialized by the runner before spawn; fake workers carry the socket
/// env and dial it themselves.
fn build_factory(backend: WorkerBackend) -> WorkerFactory {
    let sock = socket_path();
    let fleet_dir = shell_dir();
    let wt_base = shell_dir().join("wt");
    let config_base = shell_dir().join("cc-config");
    let logs = shell_dir().join("logs");

    Arc::new(move |ticket: &Ticket| {
        let slot = ticket.slot.unwrap_or(1);
        let raw = logs.join(format!("worker-{slot}")).join(format!("{}.jsonl", ticket.id));

        match &backend {
            WorkerBackend::Real { api_key } => {
                let cwd = wt_base.join(format!("worker-{slot}"));
                let _ = std::fs::create_dir_all(&cwd);
                let config_dir = config_base.join(format!("worker-{slot}"));
                let wiring = FleetWiring {
                    shim_path: shim_path(),
                    socket_path: sock.clone(),
                    slot,
                    fleet_dir: fleet_dir.clone(),
                };
                let config = WorkerConfig::probe(cwd, config_dir, api_key.clone()).with_wiring(wiring);
                WorkerSpec::real(config, ticket.clone(), slot, Some(raw), WORKER_WALL_SECS)
            }
            WorkerBackend::Fake { script } => {
                let _ = std::fs::create_dir_all(&wt_base);
                let agent = FakeWithEnv {
                    inner: FakeClaude {
                        script: script.clone(),
                        cwd: wt_base.clone(),
                        scenario: "fleet-ask".into(),
                    },
                    env: vec![
                        ("FLEET_SOCKET".into(), sock.to_string_lossy().into_owned()),
                        ("FLEETOR_SLOT".into(), slot.to_string()),
                    ],
                };
                WorkerSpec::fake(Box::new(agent), ticket.clone(), slot, Some(raw), WORKER_WALL_SECS)
            }
        }
    })
}

/// A fake worker carrying the fleet env a wired real worker would get, so its
/// socket half can find the hub. (Mirrors the server integration-test helper.)
struct FakeWithEnv {
    inner: FakeClaude,
    env: Vec<(String, String)>,
}

impl AgentProcess for FakeWithEnv {
    fn command(&self) -> std::process::Command {
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

/// The `fleetor-shim` binary sits next to this executable's siblings. In a Tauri
/// dev build the shim is built by the workspace into the same target dir; in a
/// bundle it is shipped as a sidecar. Best-effort: a wrong path fails observably
/// at worker spawn (a dead worker + notice), not a crash.
pub(crate) fn shim_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .map(|exe| exe.with_file_name("fleetor-shim"))
        .unwrap_or_else(|| PathBuf::from("fleetor-shim"))
}

/// Locate the `fake-claude.mjs` proof script: `FLEETOR_FAKE_CLAUDE` if set, else a
/// repo-relative default (present when running from the workspace).
fn fake_script_path() -> PathBuf {
    if let Ok(p) = std::env::var("FLEETOR_FAKE_CLAUDE") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    cwd.join("tests").join("fake-claude").join("fake-claude.mjs")
}

/// Read `DEEPSEEK_API_KEY` from the env or the repo's gitignored `.env`. Never
/// logged. (Mirrors the CLI's loader.)
fn load_api_key(repo_root: &Path) -> Result<String, String> {
    if let Ok(k) = std::env::var("DEEPSEEK_API_KEY") {
        if !k.is_empty() {
            return Ok(k);
        }
    }
    // The scratch repo has no .env; fall back to the launch dir's .env (dev).
    let cwd = std::env::current_dir().unwrap_or_else(|_| repo_root.to_path_buf());
    let env_path = cwd.join(".env");
    let text = std::fs::read_to_string(&env_path)
        .map_err(|_| format!("no DEEPSEEK_API_KEY in env and cannot read {env_path:?}"))?;
    for line in text.lines() {
        if let Some(rest) = line.trim().strip_prefix("DEEPSEEK_API_KEY=") {
            let val = rest.trim().trim_matches('"').trim_matches('\'');
            if !val.is_empty() {
                return Ok(val.to_string());
            }
        }
    }
    Err(format!("DEEPSEEK_API_KEY not found in env or {env_path:?}"))
}

// --- scratch repo + config ----------------------------------------------------

/// Ensure the scratch repo exists and is a git repo, so the lead operates on real
/// version control with zero blast radius. Idempotent.
fn ensure_scratch_repo() -> Result<PathBuf, String> {
    let repo = scratch_repo();
    std::fs::create_dir_all(&repo).map_err(|e| format!("create scratch repo: {e}"))?;
    if !repo.join(".git").exists() {
        let ok = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&repo)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            eprintln!("fleet: could not `git init` the scratch repo at {repo:?}");
        }
    }
    Ok(repo)
}

/// The current git branch of `repo`, or a sensible default when git is silent.
fn git_branch(repo: &Path) -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(repo)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "main".to_string())
}

fn fleet_config_for(backend: &WorkerBackend, repo: &Path) -> FleetConfig {
    let target = repo
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| repo.to_string_lossy().into_owned());
    FleetConfig {
        target,
        branch: git_branch(repo),
        worker_backend: backend.label().to_string(),
        lead_model: "opus (operator)".to_string(),
        gate: "shell gate + peer review".to_string(),
    }
}

/// Append a notice on the feed (best-effort; a store error is only logged).
fn note(store: &Arc<dyn Store>, level: NoticeLevel, text: &str) {
    if let Err(e) = store.append_event(&FleetEvent::Notice { level, text: text.into() }) {
        eprintln!("fleet: could not append notice: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real factory produces a fully-wired worker: a config carrying the shim
    /// + socket so the runner materializes its MCP/Stop-hook files before spawn,
    /// in the ticket's slot.
    #[test]
    fn real_factory_wires_the_worker_to_the_hub() {
        let factory = build_factory(WorkerBackend::Real { api_key: "sk-test".into() });
        let ticket = Ticket { slot: Some(3), ..Ticket::new("T-1", "t", "b") };

        let spec = factory(&ticket);
        assert_eq!(spec.slot, 3, "worker runs in the ticket's slot");
        let config = spec.config.expect("a real worker carries a config for the runner to materialize");
        let wiring = config.wiring.expect("a real worker is wired to the hub");
        assert_eq!(wiring.slot, 3);
        assert_eq!(wiring.socket_path, socket_path());
        assert_eq!(config.api_key, "sk-test");
    }

    /// The fake factory produces a config-less worker (nothing for the runner to
    /// materialize) that still carries the socket identity in its env, so it can
    /// dial the hub itself — the free proof path.
    #[test]
    fn fake_factory_carries_socket_identity_without_a_config() {
        let factory = build_factory(WorkerBackend::Fake { script: "/tmp/fake.mjs".into() });
        let ticket = Ticket { slot: Some(2), ..Ticket::new("T-2", "t", "b") };

        let spec = factory(&ticket);
        assert_eq!(spec.slot, 2);
        assert!(spec.config.is_none(), "a fake worker has no fleet config to materialize");

        // The command carries FLEET_SOCKET + FLEETOR_SLOT so the fake dials the hub.
        let cmd = spec.agent.command();
        let envs: Vec<_> = cmd.get_envs().collect();
        let has = |k: &str, v: &str| {
            envs.iter()
                .any(|(ek, ev)| *ek == k && ev.map(|x| x == v).unwrap_or(false))
        };
        assert!(has("FLEETOR_SLOT", "2"), "fake worker knows its slot");
        assert!(
            has("FLEET_SOCKET", &socket_path().to_string_lossy()),
            "fake worker knows the socket to dial"
        );
    }
}
