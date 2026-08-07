//! The embedded fleet: store, event bus, hub, and where the panes run (D-030).
//!
//! What this module was through Phase 2: a shell that spawned four headless
//! standby workers and bound a *dynamic* hub, so the lead's `assign` turned into a
//! supervised process. That apparatus is gone. In the TUI fleet there are no
//! headless workers — every agent is a live `claude` terminal, and the only thing
//! crossing the socket is a message.
//!
//! What is here now:
//!
//!  - the **store** and the live **event bus**, pumped to the webview by
//!    [`spawn_follower`] (unchanged, and the reason the feed still streams);
//!  - a plain [`Hub`] bound to the unix socket, whose pane ops are
//!    served by [`crate::deliver`] out of the real pty registry;
//!  - the **target** the fleet works on: the operator's repo if
//!    `~/.fleetor/config.json` names one, else a seeded [`testbed`];
//!  - [`spawn_pane`] — the one place a pane's cwd, config seed and command come
//!    together, which is why the L1 re-seed requirement is met structurally
//!    rather than by remembering to do it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use fleetor_core::event::{FleetEvent, NoticeLevel};
use fleetor_core::pane::{PaneEntry, PaneId, WORKER_SLOTS};
use fleetor_core::wire::{Op, OpResult};
use fleetor_core::Store;
use fleetor_db::SqliteStore;
use fleetor_ipc::UnixTransport;
use fleetor_server::{AppCommand, BroadcastStore, Hub};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, oneshot, Notify};

use crate::context_gauge::{self, GaugeSources, TranscriptSource};
use crate::prompts::PaneContext;
use crate::pty::PaneRegistry;
use crate::{deliver, prompts, runs, spawn, testbed};

/// Emitted for every appended event, in `seq` order, the moment it persists.
const EVENT_FLEET: &str = "fleet://event";

/// One event as the webview sees it: the `seq` cursor plus the flattened
/// [`FleetEvent`] (its `#[serde(tag = "type")]` discriminator carries through, so
/// the UI matches on `type`).
#[derive(Serialize, Clone)]
pub struct WireEvent {
    seq: i64,
    #[serde(flatten)]
    event: FleetEvent,
}

/// What a freshly-mounted UI gets back from bootstrap. The feed itself arrives
/// entirely over [`EVENT_FLEET`] — the follower replays history from 0 — so all
/// this carries is the cursor. It used to carry the board too; there is no board.
#[derive(Serialize)]
pub struct BootSnapshot {
    latest_seq: i64,
}

/// The fleet's live configuration, surfaced to the top bar so it shows *facts*
/// rather than placeholders.
#[derive(Serialize, Clone)]
pub struct FleetConfig {
    /// The repo the fleet operates on (display name — the target's directory).
    target: String,
    /// Its full path, for the spend gate and the target picker.
    target_path: String,
    /// That repo's current git branch (best-effort).
    branch: String,
    /// What fills the worker seats: the model, or `"none"` when no key is
    /// available and the fleet can only run its orchestrator.
    worker_backend: String,
    /// The model in the orchestrator seat — the operator's own Opus.
    lead_model: String,
    /// A short, honest label for the quality gate workers pass through.
    gate: String,
}

/// The live backend, created once by [`fleet_bootstrap`] and kept for the app's
/// lifetime. Owns the tokio runtime the follower, hub, and delivery loop run on.
struct Fleet {
    rt: Runtime,
    store: Arc<dyn Store>,
    /// Fired on window close so the hub stops serving and unlinks its socket.
    shutdown: Arc<Notify>,
    config: FleetConfig,
    /// Resolved once at bootstrap. Panes spawn against *this*, not a re-read of
    /// config.json — a target that changed mid-session would otherwise put half
    /// the fleet in one repo and half in another.
    target: PathBuf,
    /// The briefs and launch settings every pane spawns with, from `prompts/`
    /// and the operator's `~/.fleetor/prompts/`. Resolved once for the same
    /// reason the target is: a fleet whose panes were briefed from two revisions
    /// of a file being edited is not a fleet anyone can reason about.
    context: PaneContext,
    /// Where each worker's own transcript lives, recorded at spawn — the WP-04
    /// live gauge's source of truth. Empty until a worker has actually spawned.
    gauges: Arc<GaugeSources>,
    /// A clone of the sender [`Hub`] was built with. `fleet_roster` sends
    /// [`AppCommand::Roster`] into it directly — the identical op the CLI's
    /// `fleet roster` reaches over the socket — so the UI's poll and the CLI
    /// converge on the one place ([`deliver::spawn_delivery`]'s `Roster` arm)
    /// that samples gauges and guards the once-per-session Notice.
    app: mpsc::UnboundedSender<AppCommand>,
    /// The routing hub, held so the operator's composer can call it (WP-07).
    ///
    /// The human has no pane and therefore no socket to dial, but their
    /// messages must take the same route a pane's do or "operator → pane rides
    /// the existing path unmodified" is a claim rather than a fact.
    /// [`fleet_send`] calls `Hub::handle` with `from: PaneId::Operator` — the
    /// identical function `Hub::serve_conn` calls after reading a `Hello`, with
    /// the socket the only thing missing.
    hub: Arc<Hub>,
}

/// Managed Tauri state: at most one embedded fleet.
#[derive(Default)]
pub struct FleetState(Mutex<Option<Fleet>>);

// --- locations ----------------------------------------------------------------

/// The operator-facing root: holds `config.json` and the seeded testbed.
pub(crate) fn fleetor_dir() -> PathBuf {
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

/// A worker's isolated `CLAUDE_CONFIG_DIR`. Deliberately *not* the Phase-2
/// `cc-config/worker-*` dirs: those were built by headless `-p` runs and carry no
/// onboarding keys at all, which is precisely L1 (`docs/notes/tui-spawn-notes.md` §1).
fn worker_config_dir(slot: u8) -> PathBuf {
    shell_dir().join("pane-config").join(format!("worker-{slot}"))
}

/// A worker's private `HOME` (WP-08, the Fence): `~/.ssh`, the operator's real
/// Claude config and shell profiles stop being reachable *by name* once this is
/// what `HOME` resolves to instead. A natural sibling of `pane-config` and
/// `worktrees` — same `_shell` root, same per-slot layout — and, like both of
/// those, still under `~/.fleetor`, so `rm -rf ~/.fleetor` still removes
/// everything FLEETOR made (Tier 1.1).
fn worker_home_dir(slot: u8) -> PathBuf {
    shell_dir().join("home").join(format!("worker-{slot}"))
}

/// A worker's own checkout of the target.
fn worktree_dir(slot: u8) -> PathBuf {
    shell_dir().join("worktrees").join(format!("worker-{slot}"))
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

/// Start the embedded fleet (idempotent) and return the boot snapshot.
///
/// First call: opens the store, wraps it in the live bus, spawns the follower
/// pump, resolves the target, and binds the hub. Later calls (e.g. React
/// StrictMode's double-mount) find it already running and just return a fresh
/// snapshot.
#[tauri::command]
pub fn fleet_bootstrap(
    app: AppHandle,
    state: State<'_, FleetState>,
    registry: State<'_, Arc<PaneRegistry>>,
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

    // Cut the run boundary before anything opens the log (WP-11, D-058). This is
    // the whole of run isolation: the previous run becomes a frozen file under
    // `runs/` and this one opens an empty database. Held rather than emitted —
    // there is no store to append a notice to yet.
    let started_ms = fleetor_core::time::now_ms();
    let rotation = runs::rotate(&dir, &runs::runs_dir(&fleetor_dir()), started_ms);

    // The observability core: real store, wrapped once so every append publishes.
    let bcast = Arc::new(BroadcastStore::new(Arc::new(
        SqliteStore::open(&dir.join("state.db")).map_err(|e| format!("open store: {e}"))?,
    )));
    let store: Arc<dyn Store> = bcast.clone();

    spawn_follower(&rt, bcast.clone(), app);

    for (level, text) in &rotation {
        note(&store, *level, text);
    }

    let target = resolve_target(&store)?;
    let config = fleet_config_for(&target);

    // Stamp what this run is, for the History row it becomes at the next start.
    // The target is only ever prose inside a notice in the log, so a run that
    // ended without this marker lists with an unknown target rather than a guess.
    runs::begin(&dir, started_ms, &target);

    // Prompts and launch settings, before any pane exists. Every notice the
    // resolver produced goes on the feed here — an override that silently did
    // nothing is the one failure the whole override path is built to avoid.
    let context = PaneContext::resolve(&prompts::override_dir(&fleetor_dir()));
    for (level, text) in &context.notices {
        note(&store, *level, text);
    }

    // The hub↔app seam: the hub routes, the app owns the terminals. Unbounded on
    // purpose — a bounded channel would make a busy fleet block a send (D-034).
    let (app_tx, app_rx) = mpsc::unbounded_channel();
    let gauges = Arc::new(GaugeSources::default());
    deliver::spawn_delivery(&rt, registry.inner().clone(), app_rx, store.clone(), gauges.clone());
    // The hub takes its own clone; `fleet_roster` sends into the original so
    // the UI's poll reaches the identical `AppCommand::Roster` arm the CLI's
    // `fleet roster` does over the socket (see the `Fleet::app` doc).
    let (hub, shutdown) = spawn_hub(&rt, store.clone(), app_tx.clone(), socket_path());

    let snap = snapshot(&store)?;
    *guard =
        Some(Fleet { rt, store, shutdown, config, target, context, gauges, app: app_tx, hub });
    Ok(snap)
}

// --- panes --------------------------------------------------------------------

/// Bring one pane up: resolve its cwd, seed its config for that exact cwd, build
/// its command, and hand it to the registry.
///
/// **The L1 re-seed requirement lives here, structurally.**
/// `hasTrustDialogAccepted` is keyed by absolute project path, so a fleet pointed
/// at a new target needs its seed re-applied for every pane cwd — otherwise all
/// four workers sit on a trust dialog while every `fleet send` reports success.
/// Seeding at the spawn site rather than at the target picker means that can only
/// be got wrong by deleting this line, not by forgetting a code path.
pub(crate) fn spawn_pane(
    fleet: &FleetState,
    registry: &Arc<PaneRegistry>,
    pane: PaneId,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    let (target, store, context, gauges) = {
        let guard = fleet.0.lock().map_err(|e| e.to_string())?;
        let f = guard.as_ref().ok_or("fleet not bootstrapped")?;
        (f.target.clone(), f.store.clone(), f.context.clone(), f.gauges.clone())
    };

    // A pane with no `fleet` on its PATH is a pane that looks alive and cannot
    // talk. Say so once, loudly, rather than letting the model discover it as
    // `command not found` mid-turn (L4).
    if spawn::fleet_bin_path().is_none() {
        note(
            &store,
            NoticeLevel::Warn,
            "the `fleet` binary was not found — panes will spawn but cannot message each other. \
             Build it with `cargo build -p fleetor-cli --bin fleet`.",
        );
    }

    let socket = socket_path();
    let command = match pane {
        // Never spawnable, and refused here rather than left to fail somewhere
        // deeper: there is no command to run for a human, no cwd that is
        // theirs, and no config dir to seed. WP-07's "the operator is never
        // spawnable or killable" is this arm plus the fact that nothing in the
        // UI offers the button (`pty_kill` on a name with no pty already
        // answers "operator is not running").
        PaneId::Operator => {
            return Err("the operator is a participant, not a pane — there is nothing to spawn"
                .to_string())
        }
        // The operator's own `claude`: already onboarded, already trusted, in the
        // target itself. Nothing to seed — seeding would touch *their* config dir.
        PaneId::Orch => {
            std::fs::create_dir_all(&target).map_err(|e| format!("create orchestrator cwd: {e}"))?;
            note_spawn_estimate(&store, &context, pane, &target, None);
            spawn::orch_command(&target, &socket, &context)
        }
        PaneId::Worker(slot) => {
            let key = load_api_key()?;
            let cwd = worker_cwd(&store, &target, slot);
            let config_dir = worker_config_dir(slot);
            spawn::seed_config_dir(&config_dir, &cwd)?;
            // The Fence (WP-08): a private HOME, created and seeded before the
            // process exists — same reason the config dir is seeded here rather
            // than at the target picker (see this function's doc comment).
            let home = worker_home_dir(slot);
            spawn::seed_worker_home(&home, slot)?;
            note_spawn_estimate(&store, &context, pane, &cwd, Some(context_gauge::WORKER_WINDOW_TOKENS));
            // The WP-04 live gauge's source of truth: where to find this
            // worker's own transcript once it has one. Recorded before the
            // process exists — a `fleet roster` that lands before the pane's
            // first turn samples the (not-yet-there) file as absent, never a
            // stale or wrong pane's numbers.
            gauges.record(pane, TranscriptSource { config_dir: config_dir.clone(), cwd: cwd.clone() });
            spawn::worker_command(slot, &cwd, &home, &config_dir, &socket, &key, &context)
        }
    };

    registry.spawn(pane, command, rows, cols)
}

/// One Activity line per pane launch (WP-04's spawn-time "Loadout" counter):
/// the size of the brief this pane was just handed, estimated from text
/// already in memory — never a file read, never a tokenizer call.
fn note_spawn_estimate(
    store: &Arc<dyn Store>,
    context: &PaneContext,
    pane: PaneId,
    cwd: &Path,
    window_tokens: Option<u32>,
) {
    let roster = PaneId::roster(&WORKER_SLOTS);
    let cwd_str = cwd.display().to_string();
    let rendered = match pane {
        // Unreachable — `spawn_pane` refuses the operator before it gets here —
        // but a brief for somebody with no terminal is nothing, not an empty
        // string dressed as one.
        PaneId::Operator => return,
        PaneId::Orch => fleetor_core::brief::render_orch(&context.orch_template, &roster, &cwd_str),
        PaneId::Worker(_) => {
            fleetor_core::brief::render_worker(&context.worker_template, pane, &roster, &cwd_str)
        }
    };
    note(store, NoticeLevel::Info, &context_gauge::spawn_estimate_notice_text(pane, &rendered, window_tokens));
}

/// A worker's checkout: its own git worktree, so four workers editing at once do
/// not fight over one index.
///
/// Falls back to the target itself when git cannot oblige — the target may not be
/// a repo at all, and a fleet that refuses to start because of a worktree is worse
/// than one sharing a checkout. The fallback is announced, because "who else is
/// editing this file" is a very different question in the two arrangements.
///
/// **WP-06 raised what the fallback costs, so it raised the notice with it.** The
/// worktrees are not only an editing convenience: peer review is `git diff
/// fleet/worker-N` from a reviewer's *own* worktree, over the object database all
/// of them share (D-048). In the shared checkout there are no per-worker branches
/// and there is one working tree, so a reviewer asked to look at a peer's branch
/// is looking at the same files it is editing itself — one checkout reported five
/// times. The receipts still work; the review step does not, and the operator has
/// to know that before they trust a `done`.
fn worker_cwd(store: &Arc<dyn Store>, target: &Path, slot: u8) -> PathBuf {
    match ensure_worktree(target, slot) {
        Ok(dir) => dir,
        Err(e) => {
            note(store, NoticeLevel::Warn, &shared_checkout_warning(slot, &e, target));
            target.to_path_buf()
        }
    }
}

/// What the operator is told when a worker ends up in the shared checkout. Kept
/// separate from [`worker_cwd`] so the wording is pinned by a test — this is the
/// one notice whose absence would let a fleet look like it is reviewing itself.
fn shared_checkout_warning(slot: u8, why: &str, target: &Path) -> String {
    format!(
        "worker-{slot}: {why} — it will share the checkout at {}. \
         Peer review is degraded there: the workers have no branches of their own, \
         so `git diff fleet/worker-N` has nothing to compare and a reviewer sees the \
         same working tree it is editing. Receipts still report honestly; treat a \
         reviewed `done` as unreviewed until the target is a git repository.",
        target.display()
    )
}

fn ensure_worktree(target: &Path, slot: u8) -> Result<PathBuf, String> {
    let dir = worktree_dir(slot);
    if dir.join(".git").exists() {
        return Ok(dir);
    }
    let parent = dir.parent().ok_or("worktree path has no parent")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("create worktree root: {e}"))?;
    // Clear registrations left by a worktree directory someone deleted by hand;
    // without this, `worktree add` refuses the path it already knows about.
    let _ = git(target, &["worktree", "prune"]);

    let branch = format!("fleet/worker-{slot}");
    let dir_str = dir.to_string_lossy().into_owned();
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(target)
        .args(["worktree", "add", "-B", &branch, &dir_str])
        .output()
        .map_err(|e| format!("could not run git: {e}"))?;
    if !output.status.success() {
        let why = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git worktree add failed: {}", why.trim().replace('\n', "; ")));
    }
    Ok(dir)
}

fn git(repo: &Path, args: &[&str]) -> bool {
    std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
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
/// Returns the hub itself alongside the gate, because the operator's composer
/// calls it in-process (WP-07) — it is built here rather than inside the served
/// task so there is exactly one, shared by the socket and the UI.
fn spawn_hub(
    rt: &Runtime,
    store: Arc<dyn Store>,
    app: mpsc::UnboundedSender<AppCommand>,
    sock: PathBuf,
) -> (Arc<Hub>, Arc<Notify>) {
    let _ = std::fs::remove_file(&sock); // clear a stale socket from a prior run
    let transport = Arc::new(UnixTransport::new(&sock));

    let shutdown = Arc::new(Notify::new());
    let gate = shutdown.clone();
    let for_note = store.clone();
    let hub = Hub::new(store, app);
    let serving = hub.clone();
    rt.spawn(async move {
        let hub = serving;
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
    (hub, shutdown)
}

// --- commands -----------------------------------------------------------------

/// The live fleet configuration for the top bar and the spend gate.
#[tauri::command]
pub fn fleet_config(state: State<'_, FleetState>) -> Result<FleetConfig, String> {
    let guard = state.0.lock().map_err(|e| e.to_string())?;
    let fleet = guard.as_ref().ok_or("fleet not bootstrapped")?;
    Ok(fleet.config.clone())
}

/// The full path of the repo the running fleet is working in.
#[tauri::command]
pub fn fleet_target(state: State<'_, FleetState>) -> Result<String, String> {
    let guard = state.0.lock().map_err(|e| e.to_string())?;
    let fleet = guard.as_ref().ok_or("fleet not bootstrapped")?;
    Ok(fleet.target.to_string_lossy().into_owned())
}

/// Every pane and its state, with each worker's context gauge if one could be
/// sampled (WP-04). The UI's slow band poll and the `fleet` CLI's `fleet
/// roster` are two callers of the *same* op: this sends `AppCommand::Roster`
/// into the identical channel [`spawn_hub`] gave the [`Hub`], so both paths
/// converge on `deliver::spawn_delivery`'s one roster-answering arm — the one
/// place gauges are sampled and the once-per-session Notice is guarded.
///
/// A plain sync command, like [`fleet_pick_target`]'s blocking dialog call:
/// `oneshot::Receiver::blocking_recv` parks this call's own thread (Tauri
/// runs sync commands off its own pool), not the tokio runtime the hub and
/// the delivery loop run on, so there is nothing to deadlock.
#[tauri::command]
pub fn fleet_roster(state: State<'_, FleetState>) -> Result<Vec<PaneEntry>, String> {
    let app = {
        let guard = state.0.lock().map_err(|e| e.to_string())?;
        let fleet = guard.as_ref().ok_or("fleet not bootstrapped")?;
        fleet.app.clone()
    };
    let (ack, rx) = oneshot::channel();
    app.send(AppCommand::Roster { ack })
        .map_err(|_| "the fleet app is not accepting commands — its window may have closed".to_string())?;
    rx.blocking_recv()
        .map_err(|_| "the fleet app took the roster request but never answered".to_string())
}

// --- the operator's own messages (WP-07) --------------------------------------

/// What the composer's target select can be set to, beyond a pane's own name.
/// `all` and `reply` are spellings of a *verb*, not of a participant, which is
/// why they are resolved here into `Op::Broadcast`/`Op::Reply` rather than being
/// added to `PaneId` — a `PaneId` that meant "everyone" or "whoever spoke last"
/// would be a different participant on each side of the socket.
const TARGET_ALL: &str = "all";
const TARGET_REPLY: &str = "reply";

/// The outcome of one message the operator sent, in the fleet's own three
/// words. There is no fourth, and none of them is `delivered` (Tier 1.5).
#[derive(Serialize, Debug)]
pub struct OperatorSend {
    /// `accepted` — the bytes reached a live pty, which is not a claim the
    /// agent read them (L3). `undelivered` — they did not, and `detail` says
    /// why. `recorded` — it entered the log and no pty exists; unreachable from
    /// this composer today, since the only pty-less name is the sender.
    outcome: &'static str,
    /// The message id, or a broadcast's shared group id.
    id: String,
    /// The reason, when there is one — the pane that refused, or the legs of a
    /// fan-out that missed.
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

/// Send one message **as the operator**, through the hub the panes use.
///
/// The composer is the operator's `fleet send` / `fleet broadcast` / `fleet
/// reply`, and it is those verbs rather than a fourth thing: what crosses into
/// [`Hub::handle`] is an ordinary [`Op`] with `from: PaneId::Operator`. Nothing
/// about the delivery path knows the difference, which is the requirement —
/// "operator → pane rides the existing path unmodified" — held as a property of
/// the code rather than as a promise about it.
///
/// Sync rather than `async` for [`fleet_roster`]'s reason: Tauri runs sync
/// commands on its own pool, off the tokio runtime, so blocking this call's own
/// thread on the hub parks nothing the hub needs to finish.
#[tauri::command]
pub fn fleet_send(
    state: State<'_, FleetState>,
    target: String,
    text: String,
) -> Result<OperatorSend, String> {
    let op = operator_op(&target, &text)?;
    let (hub, handle) = {
        let guard = state.0.lock().map_err(|e| e.to_string())?;
        let fleet = guard.as_ref().ok_or("fleet not bootstrapped")?;
        (fleet.hub.clone(), fleet.rt.handle().clone())
    };
    Ok(operator_result(handle.block_on(hub.handle(PaneId::Operator, op)))?)
}

/// The composer's target and text as a wire op. Pure, so the mapping is tested
/// without a fleet — and refused here, before the hub, for the same reason the
/// CLI parses a pane name locally: a mistake should cost a sentence, not a
/// round trip.
fn operator_op(target: &str, text: &str) -> Result<Op, String> {
    let text = text.trim();
    if text.is_empty() {
        // An empty body would be typed into a live terminal as a bare newline —
        // a submitted empty turn. Clap refuses this for a pane; the composer is
        // the same boundary and owes the same refusal.
        return Err("write something first".to_string());
    }
    let text = text.to_string();
    match target.trim() {
        TARGET_ALL => Ok(Op::Broadcast { text }),
        TARGET_REPLY => Ok(Op::Reply { text }),
        name => Ok(Op::Send { to: name.parse::<PaneId>().map_err(|e| e.to_string())?, text }),
    }
}

/// The hub's answer in the operator's words. Pure, and separate from
/// [`fleet_send`], so the vocabulary is pinned by a test rather than by reading
/// the UI.
fn operator_result(result: OpResult) -> Result<OperatorSend, String> {
    match result {
        OpResult::Delivered { msg_id, accepted: true, .. } => {
            Ok(OperatorSend { outcome: "accepted", id: msg_id, detail: None })
        }
        // "undelivered", never "failed to send" — the send happened; it is the
        // arrival that did not. The same word the message feed uses.
        OpResult::Delivered { msg_id, accepted: false, detail } => {
            Ok(OperatorSend { outcome: "undelivered", id: msg_id, detail })
        }
        OpResult::Recorded { record_id } => {
            Ok(OperatorSend { outcome: "recorded", id: record_id, detail: None })
        }
        OpResult::Error { message } => Err(message),
        // The composer sends messages; a board or a roster coming back would be
        // a wiring mistake, and saying so beats rendering a blank success.
        other => Err(format!("the hub answered a message with something else: {other:?}")),
    }
}

/// Ask the operator for a repo and record it in `~/.fleetor/config.json`.
///
/// Deliberately does **not** move a running fleet: panes already have a cwd, and
/// four workers silently relocated mid-session would be reporting on files they
/// no longer hold. The new target is announced and takes effect on next launch.
/// Returns `Ok(None)` when the picker was dismissed.
#[tauri::command]
pub fn fleet_pick_target(
    app: AppHandle,
    state: State<'_, FleetState>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let Some(picked) = app.dialog().file().blocking_pick_folder() else { return Ok(None) };
    let path = picked
        .into_path()
        .map_err(|e| format!("that folder has no usable path: {e}"))?;
    if !path.is_dir() {
        return Err(format!("{} is not a directory", path.display()));
    }

    write_target(&path)?;

    if let Ok(guard) = state.0.lock() {
        if let Some(fleet) = guard.as_ref() {
            note(
                &fleet.store,
                NoticeLevel::Info,
                &format!(
                    "target set to {} — it takes effect the next time the fleet starts.",
                    path.display()
                ),
            );
        }
    }
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// Record a target the operator **typed** rather than picked.
///
/// Same contract as `fleet_pick_target`: it writes the config and announces the
/// change, and deliberately does not move a running fleet.
///
/// A typed path is untrusted in a way a picked one is not — the folder picker
/// can only hand back a directory that exists, whereas this accepts whatever
/// was in the box. So it is trimmed, `~` is expanded, and it has to resolve to
/// a real directory before anything is written. Returning the canonical form
/// matters: the operator should see what was actually recorded, not the
/// shorthand they typed, or they cannot tell a typo from a working path.
#[tauri::command]
pub fn fleet_set_target(path: String, state: State<'_, FleetState>) -> Result<String, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("enter a folder path".into());
    }
    let expanded = expand_home(trimmed)?;
    if !expanded.exists() {
        return Err(format!("{} does not exist", expanded.display()));
    }
    if !expanded.is_dir() {
        return Err(format!("{} is not a directory", expanded.display()));
    }
    // Resolves `..`, symlinks and relative segments, so the config records one
    // canonical spelling of a directory rather than however it was reached.
    let canonical = expanded
        .canonicalize()
        .map_err(|e| format!("resolve {}: {e}", expanded.display()))?;

    write_target(&canonical)?;

    if let Ok(guard) = state.0.lock() {
        if let Some(fleet) = guard.as_ref() {
            note(
                &fleet.store,
                NoticeLevel::Info,
                &format!(
                    "target set to {} — it takes effect the next time the fleet starts.",
                    canonical.display()
                ),
            );
        }
    }
    Ok(canonical.to_string_lossy().into_owned())
}

// --- past runs (WP-11) --------------------------------------------------------
//
// Four commands, all read-or-relabel. There is deliberately no command that
// resumes, re-runs or replays a past run into a live one: the panes that made it
// are gone and their context died with them, so anything shaped like "continue
// this run" would be inventing a fleet that never existed (D-030's regrowth
// warning). History is readable and nothing else.
//
// None of these touch `FleetState`, so they work before the fleet is started —
// which is the case that matters, since the History view is most useful on the
// start gate, deciding what to do next.

/// Every archived run, newest first.
#[tauri::command]
pub fn runs_list() -> Result<Vec<runs::RunRecord>, String> {
    Ok(runs::list(&runs::runs_dir(&fleetor_dir())))
}

/// Replay one archived run's log, for the read-only History views.
#[tauri::command]
pub fn run_events(id: String, after: i64) -> Result<Vec<WireEvent>, String> {
    Ok(runs::events(&runs::runs_dir(&fleetor_dir()), &id, after)?
        .into_iter()
        .map(|(seq, event)| WireEvent { seq, event })
        .collect())
}

/// Give a run a name that means something to the operator.
#[tauri::command]
pub fn run_rename(id: String, label: String) -> Result<(), String> {
    runs::rename(&runs::runs_dir(&fleetor_dir()), &id, &label)
}

/// Delete a run and its directory. Nothing else in the app refers to a run by
/// id, so this needs no cascade — the index is rebuilt from what is left.
#[tauri::command]
pub fn run_delete(id: String) -> Result<(), String> {
    runs::delete(&runs::runs_dir(&fleetor_dir()), &id)
}

/// Save a run's JSON export wherever the operator points.
///
/// The dialog lives here rather than in the webview so no npm plugin has to be
/// added for it — `fleet_pick_target` set the pattern. `Ok(None)` means the
/// operator dismissed the dialog, which is not an error and must not be shown
/// as one.
#[tauri::command]
pub fn run_export(app: AppHandle, id: String) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let suggested = format!("{id}-events.json");
    let Some(chosen) = app
        .dialog()
        .file()
        .set_file_name(&suggested)
        .add_filter("JSON", &["json"])
        .blocking_save_file()
    else {
        return Ok(None);
    };
    let path = chosen.into_path().map_err(|e| format!("that location has no usable path: {e}"))?;
    runs::export(&runs::runs_dir(&fleetor_dir()), &id, &path)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// `~` and `~/…` are what an operator types; `std::path` treats them as literal
/// directory names, so a typed home-relative path would silently miss.
fn expand_home(input: &str) -> Result<PathBuf, String> {
    if input != "~" && !input.starts_with("~/") {
        return Ok(PathBuf::from(input));
    }
    let home = std::env::var_os("HOME").ok_or("HOME is not set, so ~ cannot be expanded")?;
    let home = PathBuf::from(home);
    Ok(if input == "~" { home } else { home.join(&input[2..]) })
}

/// Set `target` in the config without disturbing anything else the operator has
/// put there. Merge-not-clobber for the same reason the config seed is.
fn write_target(target: &Path) -> Result<(), String> {
    let file = config_path();
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let existing = std::fs::read_to_string(&file).ok();
    let text = merge_target(existing.as_deref(), target)?;
    std::fs::write(&file, text).map_err(|e| format!("write {}: {e}", file.display()))
}

/// The merge itself, kept pure so it is tested without writing to the operator's
/// real home directory. Unreadable or non-object config text is replaced rather
/// than treated as fatal — refusing to record a target the operator just picked
/// would leave the picker looking broken.
fn merge_target(existing: Option<&str>, target: &Path) -> Result<String, String> {
    let mut root = existing
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .filter(|v| v.is_object())
        .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));
    root.as_object_mut()
        .expect("just filtered to an object")
        .insert("target".into(), target.to_string_lossy().into_owned().into());
    serde_json::to_string_pretty(&root).map_err(|e| format!("encode config: {e}"))
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
    Ok(BootSnapshot { latest_seq: store.latest_seq().map_err(|e| e.to_string())? })
}

// --- worker credentials -------------------------------------------------------

/// Read `DEEPSEEK_API_KEY` from the env or the nearest `.env` on the path from the
/// cwd up to the filesystem root. Under `tauri dev` the cwd is `src-tauri/`, so a
/// repo-root `.env` is found by walking up (not just `<cwd>/.env`) — otherwise a
/// key sitting at the repo root silently goes unseen. Never logged.
///
/// This is what a worker pane sets `ANTHROPIC_AUTH_TOKEN` from — and only that.
/// Never `ANTHROPIC_API_KEY`: with it set, the interactive TUI blocks on api-key
/// approval and never reaches its prompt (L2).
fn load_api_key() -> Result<String, String> {
    if let Ok(k) = std::env::var("DEEPSEEK_API_KEY") {
        if !k.is_empty() {
            return Ok(k);
        }
    }
    let start = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    for dir in start.ancestors() {
        let Ok(text) = std::fs::read_to_string(dir.join(".env")) else { continue };
        if let Some(key) = parse_deepseek_key(&text) {
            return Ok(key);
        }
    }
    Err(format!(
        "no DEEPSEEK_API_KEY in the environment or any .env from {} upward — \
         the orchestrator runs without one, worker panes cannot",
        start.display()
    ))
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
        target_path: target.to_string_lossy().into_owned(),
        branch: git_branch(target),
        // What a click will actually spend. Without a key the worker panes cannot
        // start at all, and the gate must say so rather than offering four seats
        // that fail on spawn.
        worker_backend: match load_api_key() {
            Ok(_) => "deepseek-v4-flash".to_string(),
            Err(_) => "none".to_string(),
        },
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

    /// The wiring end to end over a **real unix socket**: bootstrap's hub and the
    /// real delivery loop over a real (empty) pane registry, dialled by a real
    /// client exactly the way the `fleet` CLI does.
    ///
    /// The pane here is genuinely not running, so the honest answer is a refusal
    /// that names it — not a park, and not a success the feed would have to walk
    /// back. `src-tauri/tests/panes.rs` runs the same path with live ptys.
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
        let registry = Arc::new(PaneRegistry::new(Arc::new(|_, _| {}), dir.join("panes.pids")));
        deliver::spawn_delivery(&rt, registry, app_rx, store.clone(), Arc::new(GaugeSources::default()));
        let (_hub, shutdown) = spawn_hub(&rt, store.clone(), app_tx, sock.clone());

        let result = rt.block_on(async {
            let transport = UnixTransport::new(&sock);
            let mut client = connect(&transport, PaneId::Orch).await.expect("hub never came up");
            client
                .call(Op::Send { to: PaneId::Worker(1), text: "take T-4".into() })
                .await
                .expect("the hub answered")
        });

        let OpResult::Delivered { accepted, detail, .. } = result else {
            panic!("expected a delivery result, got {result:?}");
        };
        assert!(!accepted, "a pane that is not running cannot accept anything");
        assert!(
            detail.as_deref().unwrap_or_default().contains("worker-1"),
            "the refusal must name the pane: {detail:?}"
        );

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

    /// The shared-checkout fallback is the one arrangement where a `done` can be
    /// reviewed by somebody looking at their own edits (WP-06). The notice has to
    /// say that in words, not only that a worktree failed — an operator reading
    /// "sharing the target checkout instead" has no way to know the review step
    /// stopped meaning anything.
    #[test]
    fn the_shared_checkout_warning_says_review_is_what_breaks() {
        let text = shared_checkout_warning(2, "git worktree add failed: not a repository", Path::new("/tmp/target"));
        assert!(text.contains("worker-2"), "{text}");
        assert!(text.contains("not a repository"), "the cause survives: {text}");
        assert!(text.contains("/tmp/target"), "and where it landed: {text}");
        assert!(text.contains("Peer review is degraded"), "{text}");
        assert!(text.contains("git diff fleet/worker-N"), "names the move that stops working: {text}");
        assert!(
            text.contains("treat a reviewed `done` as unreviewed"),
            "the operator needs what to do about it, not only what happened: {text}",
        );
    }

    /// A picked target must land in the config without costing the operator
    /// whatever else they had put there by hand.
    #[test]
    fn recording_a_target_replaces_only_the_target() {
        let merged = merge_target(Some(r#"{"theme":"warm","target":"/old"}"#), Path::new("/new"));
        let config: serde_json::Value = serde_json::from_str(&merged.unwrap()).unwrap();
        assert_eq!(config["target"], serde_json::json!("/new"));
        assert_eq!(config["theme"], serde_json::json!("warm"), "an unrelated setting survived");
    }

    /// The picker must still work on a first run, and on a config someone has
    /// broken — refusing to record the folder they just chose would read as a
    /// broken picker rather than as a broken file.
    #[test]
    fn a_missing_or_unreadable_config_still_records_the_target() {
        for existing in [None, Some("not json at all"), Some("[]")] {
            let merged = merge_target(existing, Path::new("/picked")).unwrap();
            let config: serde_json::Value = serde_json::from_str(&merged).unwrap();
            assert_eq!(config["target"], serde_json::json!("/picked"), "{existing:?}");
        }
    }

    /// The round trip that matters: what the picker writes is what the next boot
    /// reads. Two functions on opposite ends of a restart, pinned together.
    #[test]
    fn what_the_picker_writes_is_what_bootstrap_reads_back() {
        let merged = merge_target(None, Path::new("/Users/me/code/thing")).unwrap();
        assert_eq!(parse_target(&merged).unwrap(), Some(PathBuf::from("/Users/me/code/thing")));
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

    // --- the operator's composer (WP-07) ---------------------------------------

    /// The composer's three targets are the fleet's three message verbs. A pane
    /// name is a `send`, `all` is the ordinary broadcast, `reply` is the
    /// ordinary reply — nothing here is a fourth kind of message.
    #[test]
    fn the_composers_target_picks_the_verb_and_never_invents_one() {
        assert_eq!(
            operator_op("worker-2", "ship it").unwrap(),
            Op::Send { to: PaneId::Worker(2), text: "ship it".into() },
        );
        assert_eq!(
            operator_op("2", "ship it").unwrap(),
            Op::Send { to: PaneId::Worker(2), text: "ship it".into() },
            "every spelling the CLI takes, the composer takes",
        );
        assert_eq!(operator_op("orch", "hi").unwrap(), Op::Send { to: PaneId::Orch, text: "hi".into() });
        assert_eq!(operator_op("all", "stop").unwrap(), Op::Broadcast { text: "stop".into() });
        assert_eq!(operator_op("reply", "yes").unwrap(), Op::Reply { text: "yes".into() });
    }

    /// An empty body would be typed into a live terminal as a bare newline — a
    /// submitted empty turn in somebody's `claude`. Refused at the boundary,
    /// exactly as clap refuses it for a pane.
    #[test]
    fn the_composer_refuses_an_empty_message_and_an_unknown_target() {
        assert!(operator_op("worker-2", "   ").is_err());
        assert!(operator_op("all", "").is_err());
        let why = operator_op("sidebar", "hi").expect_err("not a participant");
        assert!(why.contains("sidebar"), "the refusal names what it was handed: {why}");
    }

    /// The vocabulary, at the seam where the UI reads it. Three words, and the
    /// one that does not exist is `delivered`.
    #[test]
    fn the_composer_reports_the_fleets_three_words_and_no_others() {
        let accepted =
            operator_result(OpResult::Delivered { msg_id: "msg-1".into(), accepted: true, detail: None })
                .unwrap();
        assert_eq!(accepted.outcome, "accepted");
        assert_eq!(accepted.detail, None);

        let refused = operator_result(OpResult::Delivered {
            msg_id: "grp-1".into(),
            accepted: false,
            detail: Some("worker-3: pane worker-3 is dead".into()),
        })
        .unwrap();
        assert_eq!(refused.outcome, "undelivered", "the send happened; the arrival did not");
        assert!(refused.detail.unwrap().contains("worker-3"));

        assert_eq!(operator_result(OpResult::Recorded { record_id: "msg-2".into() }).unwrap().outcome, "recorded");

        let error = operator_result(OpResult::Error { message: "nobody has messaged operator yet".into() })
            .expect_err("an error is an error");
        assert!(error.contains("nobody has messaged operator"), "{error}");
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
