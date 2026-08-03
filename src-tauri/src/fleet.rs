//! Phase 2 of the TUI pivot — **the old fleet, unwired** (D-030).
//!
//! What this module was: a shell that spawned four headless standby workers and
//! bound a *dynamic* hub, so the lead's `assign` over the socket turned into a
//! supervised `fake-claude` or DeepSeek Flash process. That whole apparatus is
//! gone. In the TUI fleet there are no headless workers — every agent is a live
//! `claude` terminal, and the only thing crossing the socket is a message.
//!
//! What is left is deliberately small:
//!
//!  - the **store** and the live **event bus**, pumped to the webview by
//!    [`spawn_follower`] (unchanged, and the reason the feed still streams);
//!  - a plain [`Hub::with_app`] bound to the unix socket, whose pane ops
//!    ([`AppCommand`]) are answered by the fleet app itself;
//!  - the **target** the fleet works on: the operator's repo if
//!    `~/.fleetor/config.json` names one, else a seeded [`testbed`].
//!
//! The pane registry that makes an [`AppCommand::Deliver`] land in a real
//! terminal is Phase 3 (`deliver.rs`). Until it exists, [`spawn_pane_seam`]
//! answers every command with an honest refusal — never a silent drop, and never
//! a park, because nothing between `fleet send` and the pty has a timeout (D-034).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use fleetor_core::event::{FleetEvent, NoticeLevel, TicketState};
use fleetor_core::ticket::Ticket;
use fleetor_core::Store;
use fleetor_db::SqliteStore;
use fleetor_ipc::UnixTransport;
use fleetor_server::{AppCommand, BroadcastStore, DeliveryResult, Hub, HubConfig};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, Notify};

use crate::testbed;

/// Emitted for every appended event, in `seq` order, the moment it persists.
const EVENT_FLEET: &str = "fleet://event";

/// The Phase-2 answer to any pane op: there is no registry yet, so nothing can be
/// live. Phase 3 replaces this with a real lookup.
const NO_PANES: &str = "no panes are running yet — the pane registry lands in Phase 3";

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
/// rather than placeholders.
#[derive(Serialize, Clone)]
pub struct FleetConfig {
    /// The repo the fleet operates on (display name — the target's directory).
    target: String,
    /// That repo's current git branch (best-effort).
    branch: String,
    /// What fills the worker seats. `"none"` until Phase 3 spawns worker panes —
    /// the headless backends this field used to name are gone.
    worker_backend: String,
    /// The model in the orchestrator seat — the operator's own Opus.
    lead_model: String,
    /// A short, honest label for the quality gate workers pass through.
    gate: String,
}

/// The live backend, created once by [`fleet_bootstrap`] and kept for the app's
/// lifetime. Owns the tokio runtime the follower, hub, and pane seam run on.
struct Fleet {
    _rt: Runtime,
    store: Arc<dyn Store>,
    /// Fired on window close so the hub stops serving and unlinks its socket.
    shutdown: Arc<Notify>,
    config: FleetConfig,
}

/// Managed Tauri state: at most one embedded fleet.
#[derive(Default)]
pub struct FleetState(Mutex<Option<Fleet>>);

// --- locations ----------------------------------------------------------------

/// The operator-facing root: holds `config.json` and the seeded testbed.
fn fleetor_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".fleetor")
}

/// State root, out of the user's repo so `rm -rf ~/.fleetor/_shell` fully undoes
/// it (Tier-1 boundary).
pub(crate) fn shell_dir() -> PathBuf {
    fleetor_dir().join("_shell")
}

/// The seeded project the fleet falls back to when no target is configured.
fn testbed_dir() -> PathBuf {
    fleetor_dir().join("testbed")
}

/// Where the operator names the repo the fleet should work on.
fn config_path() -> PathBuf {
    fleetor_dir().join("config.json")
}

/// The fleet unix socket the `fleet` CLI dials.
pub(crate) fn socket_path() -> PathBuf {
    shell_dir().join("fleet.sock")
}

/// The directory the fleet works in, resolved fresh. Used by the pty spawn path
/// for a pane's cwd; [`fleet_bootstrap`] is what actually seeds the testbed, so
/// this only ever *reads* the decision.
pub(crate) fn target_dir() -> PathBuf {
    configured_target().ok().flatten().unwrap_or_else(testbed_dir)
}

/// The target named in `~/.fleetor/config.json`, if there is a usable one.
/// `Ok(None)` means nothing is configured (the ordinary first-run case); `Err`
/// means something *is* configured and can't be used, which the operator has to
/// be told about rather than silently working somewhere else.
fn configured_target() -> Result<Option<PathBuf>, String> {
    let path = config_path();
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("could not read {}: {e}", path.display())),
    };
    let Some(target) = parse_target(&text).map_err(|e| format!("{}: {e}", path.display()))? else {
        return Ok(None);
    };
    if !target.is_dir() {
        return Err(format!("target {} is not a directory", target.display()));
    }
    Ok(Some(target))
}

/// Pull the `target` out of config text. `Ok(None)` for a config that simply
/// doesn't set one; `Err` only for text that isn't JSON at all — a typo'd config
/// must not read as "no target configured". Kept pure so it is unit-tested
/// without touching the filesystem.
fn parse_target(text: &str) -> Result<Option<PathBuf>, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("not valid JSON ({e})"))?;
    let Some(target) = value.get("target") else { return Ok(None) };
    let Some(s) = target.as_str() else {
        return Err("\"target\" must be a path string".to_string());
    };
    let s = s.trim();
    Ok((!s.is_empty()).then(|| PathBuf::from(s)))
}

// --- bootstrap ----------------------------------------------------------------

/// Start the embedded fleet (idempotent) and return the board snapshot.
///
/// First call: opens the store, wraps it in the live bus, spawns the follower
/// pump, resolves the target, and binds the hub. Later calls (e.g. React
/// StrictMode's double-mount) find it already running and just return a fresh
/// snapshot.
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

    let target = resolve_target(&store)?;
    let config = fleet_config_for(&target);

    // The hub↔app seam: the hub routes, the app owns the terminals. Unbounded on
    // purpose — a bounded channel would make a busy fleet block a send (D-034).
    let (app_tx, app_rx) = mpsc::unbounded_channel();
    spawn_pane_seam(&rt, app_rx);
    let shutdown = spawn_hub(&rt, store.clone(), app_tx, socket_path());

    let snap = snapshot(&store)?;
    *guard = Some(Fleet { _rt: rt, store, shutdown, config });
    Ok(snap)
}

/// Decide where the fleet works, announcing the choice on the feed so it is never
/// a silent surprise. A configured target wins; anything else falls back to the
/// seeded testbed, which is the only case that has to create anything.
fn resolve_target(store: &Arc<dyn Store>) -> Result<PathBuf, String> {
    match configured_target() {
        Ok(Some(target)) => {
            note(store, NoticeLevel::Info, &format!("target: {}", target.display()));
            Ok(target)
        }
        Ok(None) => fall_back_to_testbed(store, None),
        Err(e) => fall_back_to_testbed(store, Some(e)),
    }
}

fn fall_back_to_testbed(store: &Arc<dyn Store>, problem: Option<String>) -> Result<PathBuf, String> {
    let testbed = testbed::ensure(&testbed_dir())?;
    let (level, why) = match problem {
        Some(e) => (NoticeLevel::Warn, format!("{e} — ")),
        None => (NoticeLevel::Info, String::new()),
    };
    note(
        store,
        level,
        &format!(
            "{why}working in the seeded testbed at {}. Set \"target\" in {} to point the fleet at your own repo.",
            testbed.display(),
            config_path().display()
        ),
    );
    Ok(testbed)
}

/// Pump every appended event to the webview, oldest-first then live. Started once;
/// runs for the app's lifetime.
fn spawn_follower(rt: &Runtime, bcast: Arc<BroadcastStore>, app: AppHandle) {
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

/// Bind the hub on `sock` and serve until the app closes. Returns the shutdown
/// gate. (The socket is a parameter, not [`socket_path`], so this is exercisable
/// over a real unix socket in a test.)
///
/// A bind failure is the one error that takes messaging down completely, so it is
/// reported on the feed rather than only to stderr — a shell that looks fine while
/// every `fleet send` fails is the worst version of this failure.
fn spawn_hub(
    rt: &Runtime,
    store: Arc<dyn Store>,
    app: mpsc::UnboundedSender<AppCommand>,
    sock: PathBuf,
) -> Arc<Notify> {
    let _ = std::fs::remove_file(&sock); // clear a stale socket from a prior run
    let transport = Arc::new(UnixTransport::new(&sock));

    let shutdown = Arc::new(Notify::new());
    let gate = shutdown.clone();
    let for_note = store.clone();
    rt.spawn(async move {
        let hub = Hub::with_app(store, HubConfig::default(), app);
        tokio::select! {
            result = hub.run(transport) => {
                if let Err(e) = result {
                    eprintln!("fleet: hub stopped: {e}");
                    note(&for_note, NoticeLevel::Error, &format!("fleet socket unavailable — messaging is down: {e}"));
                }
            }
            _ = gate.notified() => {}
        }
        let _ = std::fs::remove_file(&sock);
    });
    shutdown
}

/// The Phase-2 stand-in for `deliver.rs`: answer every [`AppCommand`], honestly.
///
/// It must answer *something* for each one. The hub awaits the ack with no
/// deadline (D-034), so a command received and left unanswered parks the caller's
/// `fleet send` forever — the exact failure the unbounded design accepts in
/// exchange for never lying about a delivery.
fn spawn_pane_seam(rt: &Runtime, mut commands: mpsc::UnboundedReceiver<AppCommand>) {
    rt.spawn(async move {
        while let Some(command) = commands.recv().await {
            match command {
                AppCommand::Deliver { ack, .. } => {
                    let _ = ack.send(DeliveryResult::rejected(NO_PANES));
                }
                AppCommand::Roster { ack } => {
                    let _ = ack.send(Vec::new());
                }
            }
        }
    });
}

// --- commands -----------------------------------------------------------------

/// The board as it stands now, straight from the store (the source of truth the
/// UI refetches whenever it sees a ticket move).
#[tauri::command]
pub fn fleet_board(state: State<'_, FleetState>) -> Result<Vec<Ticket>, String> {
    let guard = state.0.lock().map_err(|e| e.to_string())?;
    let fleet = guard.as_ref().ok_or("fleet not bootstrapped")?;
    fleet.store.tickets().map_err(|e| e.to_string())
}

/// The live fleet configuration for the top bar (real target/branch).
#[tauri::command]
pub fn fleet_config(state: State<'_, FleetState>) -> Result<FleetConfig, String> {
    let guard = state.0.lock().map_err(|e| e.to_string())?;
    let fleet = guard.as_ref().ok_or("fleet not bootstrapped")?;
    Ok(fleet.config.clone())
}

/// Put a ticket on the board as `Assigned`. A real action through the store, so
/// the move streams to the UI over the event bus like any other.
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

/// Stop the hub and unlink its socket. Best-effort, called on window close
/// alongside the pty teardown.
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

// --- worker credentials -------------------------------------------------------

/// Read `DEEPSEEK_API_KEY` from the env or the nearest `.env` on the path from the
/// cwd up to the filesystem root. Under `tauri dev` the cwd is `src-tauri/`, so a
/// repo-root `.env` is found by walking up (not just `<cwd>/.env`) — otherwise a
/// key sitting at the repo root silently goes unseen. Never logged.
///
/// Unused until Phase 3, which spawns the worker panes: it is what they set
/// `ANTHROPIC_AUTH_TOKEN` from. Never `ANTHROPIC_API_KEY` — with that set, the
/// interactive TUI blocks on api-key approval and never reaches its prompt (L2).
#[allow(dead_code)]
fn load_api_key(repo_root: &Path) -> Result<String, String> {
    if let Ok(k) = std::env::var("DEEPSEEK_API_KEY") {
        if !k.is_empty() {
            return Ok(k);
        }
    }
    let start = std::env::current_dir().unwrap_or_else(|_| repo_root.to_path_buf());
    for dir in start.ancestors() {
        let Ok(text) = std::fs::read_to_string(dir.join(".env")) else { continue };
        if let Some(key) = parse_deepseek_key(&text) {
            return Ok(key);
        }
    }
    Err(format!("DEEPSEEK_API_KEY not found in env or any .env from {start:?} upward"))
}

/// Pull the `DEEPSEEK_API_KEY` value out of `.env` text, tolerating surrounding
/// quotes; `None` if absent or empty. Kept separate so the parse is unit-tested
/// without touching the filesystem.
fn parse_deepseek_key(text: &str) -> Option<String> {
    for line in text.lines() {
        if let Some(rest) = line.trim().strip_prefix("DEEPSEEK_API_KEY=") {
            let val = rest.trim().trim_matches('"').trim_matches('\'');
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

// --- display ------------------------------------------------------------------

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

fn fleet_config_for(target: &Path) -> FleetConfig {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| target.to_string_lossy().into_owned());
    FleetConfig {
        target: name,
        branch: git_branch(target),
        worker_backend: "none".to_string(),
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
    use fleetor_core::pane::PaneId;
    use fleetor_core::wire::{Hello, Op, OpResult};
    use fleetor_ipc::Client;
    use std::time::Duration;

    /// The Phase-2 wiring, end to end over a **real unix socket**: bootstrap's
    /// hub + seam, dialled by a real client the way the `fleet` CLI will.
    ///
    /// Everything proven so far was in-process against a fake app. This is the
    /// first thing that shows the socket binds, the hub serves a pane op, and an
    /// answer comes back — the mechanism itself, minus the pty.
    #[test]
    fn a_pane_op_crosses_the_real_socket_and_is_answered() {
        let dir = std::env::temp_dir().join(format!("fleetor-hub-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sock = dir.join("fleet.sock");

        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let store: Arc<dyn Store> =
            Arc::new(SqliteStore::open(&dir.join("state.db")).unwrap());

        let (app_tx, app_rx) = mpsc::unbounded_channel();
        spawn_pane_seam(&rt, app_rx);
        let shutdown = spawn_hub(&rt, store.clone(), app_tx, sock.clone());

        let result = rt.block_on(async {
            let transport = UnixTransport::new(&sock);
            let mut client = connect(&transport, PaneId::Orch).await.expect("hub never came up");
            client
                .call(Op::PaneSend { to: PaneId::Worker(1), text: "take T-4".into() })
                .await
                .expect("the hub answered")
        });

        // No panes exist yet, so the honest answer is a refusal that says why —
        // not a park, and not a success the feed would then have to walk back.
        let OpResult::Delivered { accepted, detail, .. } = result else {
            panic!("expected a delivery result, got {result:?}");
        };
        assert!(!accepted, "nothing can be accepted before the pane registry exists");
        assert_eq!(detail.as_deref(), Some(NO_PANES));

        // And the attempt is on the record, body included.
        let logged = store
            .events_since(0)
            .unwrap()
            .into_iter()
            .any(|(_, e)| matches!(&e, FleetEvent::Message { body, accepted, .. } if body == "take T-4" && !accepted));
        assert!(logged, "a refused send must still reach the feed with its body");

        shutdown.notify_one();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The hub binds a beat after it is spawned, so a client that dials once loses
    /// a race the real CLI doesn't (it is started by hand, long after boot).
    async fn connect(transport: &UnixTransport, pane: PaneId) -> Option<Client> {
        for _ in 0..100 {
            if let Ok(c) = Client::connect(transport, Hello::for_pane(pane)).await {
                return Some(c);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        None
    }

    #[test]
    fn reads_the_configured_target_from_config_text() {
        assert_eq!(
            parse_target(r#"{"target": "/Users/me/code/thing"}"#).unwrap(),
            Some(PathBuf::from("/Users/me/code/thing")),
        );
    }

    /// A config that simply doesn't name a target is the ordinary case, not an
    /// error — it falls back to the testbed with a notice.
    #[test]
    fn a_config_without_a_target_is_not_an_error() {
        assert_eq!(parse_target("{}").unwrap(), None);
        assert_eq!(parse_target(r#"{"target": ""}"#).unwrap(), None);
        assert_eq!(parse_target(r#"{"target": "   "}"#).unwrap(), None);
    }

    /// A broken config must be loud. Reading it as "nothing configured" would put
    /// the fleet in the testbed while the operator believes it is in their repo.
    #[test]
    fn a_malformed_config_is_reported_rather_than_ignored() {
        assert!(parse_target("not json at all").is_err());
        assert!(parse_target(r#"{"target": 7}"#).is_err(), "a non-string target is a mistake, not an absence");
    }

    /// The `.env` parse tolerates quotes/comments and ignores an empty value, so a
    /// repo-root key is picked up (via the walk-up in `load_api_key`) not skipped.
    #[test]
    fn parses_deepseek_key_from_env_text() {
        assert_eq!(parse_deepseek_key("DEEPSEEK_API_KEY=sk-abc\n").as_deref(), Some("sk-abc"));
        assert_eq!(
            parse_deepseek_key("# comment\nOTHER=1\nDEEPSEEK_API_KEY=\"sk-xyz\"\n").as_deref(),
            Some("sk-xyz"),
        );
        assert_eq!(parse_deepseek_key("OTHER=1\n"), None);
        assert_eq!(parse_deepseek_key("DEEPSEEK_API_KEY=\n"), None);
    }
}
