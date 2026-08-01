//! Phase 4e-1 — the **fleet server, embedded in the shell**.
//!
//! Phases 1–4d built the whole fleet (hub, store, quality loop, dynamic runner)
//! and proved it headless through `fleetor-cli`. 4e-1 brings the observability
//! core of that into the Tauri backend so a *window* can watch it live, with the
//! same seam the CLI used: a [`BroadcastStore`] over the real SQLite store, and a
//! [follower](fleetor_server::EventFollower) that pushes every appended event to
//! the webview as it lands (the 4c push path, now feeding React instead of
//! `println!`).
//!
//! **What is real here:** the store, the live event bus, the follower→webview
//! pump, and the hub bound on the fleet socket (idle until a client connects).
//! Board mutations (`fleet_assign`) go through the same store, so they stream to
//! the UI for free. **What is a stand-in:** [`demo`] — a scripted ticket
//! lifecycle that animates the shell with *zero tokens and no child processes*.
//! 4e-2 replaces `demo` with the real orchestrator pty driving `run_dynamic_fleet`
//! over the hub already bound here; the follower→UI seam does not change.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fleetor_core::event::{
    FleetEvent, GateOutcome, NoticeLevel, ReviewOutcome, TicketState, WorkerState,
};
use fleetor_core::report::ReportStatus;
use fleetor_core::ticket::Ticket;
use fleetor_core::Store;
use fleetor_db::SqliteStore;
use fleetor_ipc::{Transport, UnixTransport};
use fleetor_server::{BroadcastStore, Hub, HubConfig};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

/// Emitted for every appended event, in `seq` order, the moment it persists.
const EVENT_FLEET: &str = "fleet://event";

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

/// The live backend, created once by [`fleet_bootstrap`] and kept for the app's
/// lifetime. Owns the tokio runtime the follower and hub run on.
struct Fleet {
    _rt: tokio::runtime::Runtime,
    store: Arc<dyn Store>,
}

/// Managed Tauri state: at most one embedded fleet.
#[derive(Default)]
pub struct FleetState(Mutex<Option<Fleet>>);

/// State root, out of the user's repo so `rm -rf ~/.fleetor/_shell` fully undoes
/// it (Tier-1 boundary).
fn shell_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".fleetor").join("_shell")
}

/// Start the embedded fleet (idempotent) and return the board snapshot.
///
/// First call: opens the store, wraps it in the live bus, binds the hub on the
/// fleet socket, and spawns the follower pump. Later calls (e.g. React
/// StrictMode's double-mount) find it already running and just return a fresh
/// snapshot — never a second follower.
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
    bind_hub_idle(&rt, store.clone(), &dir.join("fleet.sock"));

    let snap = snapshot(&store)?;
    *guard = Some(Fleet { _rt: rt, store });
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

/// Bind and serve the hub on the fleet socket. Idle in 4e-1 (no clients yet); it
/// is here so 4e-2's lead/worker shims connect with zero backend change. A bind
/// failure degrades gracefully — the observable shell does not depend on it.
fn bind_hub_idle(rt: &tokio::runtime::Runtime, store: Arc<dyn Store>, sock: &std::path::Path) {
    let _ = std::fs::remove_file(sock); // clear a stale socket from a prior run
    let transport = UnixTransport::new(sock);
    match rt.block_on(async move { transport.bind().await }) {
        Ok(listener) => {
            let hub = Hub::new(store, HubConfig::default());
            rt.spawn(async move {
                if let Err(e) = hub.serve(listener).await {
                    eprintln!("fleet: hub stopped serving: {e}");
                }
            });
        }
        Err(e) => eprintln!("fleet: hub not bound (shell still observable): {e}"),
    }
}

/// The board as it stands now, straight from the store (the source of truth the
/// UI refetches whenever it sees a ticket move).
#[tauri::command]
pub fn fleet_board(state: State<'_, FleetState>) -> Result<Vec<Ticket>, String> {
    let guard = state.0.lock().map_err(|e| e.to_string())?;
    let fleet = guard.as_ref().ok_or("fleet not bootstrapped")?;
    fleet.store.tickets().map_err(|e| e.to_string())
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

/// Animate the shell with a scripted ticket lifecycle — the 4e-1 stand-in for a
/// live fleet. Zero tokens, no processes: it walks [`demo::plan`] through the
/// same store, so the board, worker band, and feed all move exactly as a real
/// run would drive them.
#[tauri::command]
pub fn fleet_demo(state: State<'_, FleetState>) -> Result<(), String> {
    let guard = state.0.lock().map_err(|e| e.to_string())?;
    let fleet = guard.as_ref().ok_or("fleet not bootstrapped")?;
    let store = fleet.store.clone();
    // Runs on the fleet runtime; the follower streams each step to the UI live.
    fleet
        ._rt
        .spawn(async move { demo::run(store, Duration::from_millis(650)).await });
    Ok(())
}

fn snapshot(store: &Arc<dyn Store>) -> Result<BootSnapshot, String> {
    Ok(BootSnapshot {
        board: store.tickets().map_err(|e| e.to_string())?,
        latest_seq: store.latest_seq().map_err(|e| e.to_string())?,
    })
}

/// The scripted lifecycle that stands in for a live fleet in 4e-1. Kept as a
/// pure [`plan`] (a `Vec` of [`DemoStep`]) so it is testable headless, with a
/// thin async [`run`] that applies each step through the store on a timer.
pub mod demo {
    use super::*;

    /// One scripted move. Applying it writes to the store exactly as the real
    /// supervisor/hub would — a ticket transition, a worker state change, a tool
    /// call, mail, a gate/review verdict, a report, or a note.
    #[derive(Debug, Clone)]
    pub enum DemoStep {
        Board { ticket: &'static str, from: TicketState, to: TicketState },
        Worker { slot: u8, from: WorkerState, to: WorkerState },
        Tool { slot: u8, ticket: &'static str, tool: &'static str },
        Mail { from: &'static str, to: &'static str, kind: &'static str },
        Gate { ticket: &'static str, slot: u8, outcome: GateOutcome },
        Review { ticket: &'static str, reviewer_slot: u8, outcome: ReviewOutcome },
        Report { ticket: &'static str, slot: u8, status: ReportStatus },
        Note { level: NoticeLevel, text: &'static str },
    }

    /// The two demo tickets and the ordered steps that carry them to done — a
    /// worker asking, being steered, passing the gate, and clearing review.
    pub struct DemoPlan {
        pub tickets: Vec<Ticket>,
        pub steps: Vec<DemoStep>,
    }

    pub fn plan() -> DemoPlan {
        use DemoStep::*;
        let tickets = vec![
            Ticket::new("T-101", "wire the event bus to the UI", "Stream appended events to the webview.")
                .with_files(vec!["src-tauri/src/fleet.rs".into()]),
            Ticket::new("T-102", "ticket board columns", "Render tickets grouped by state.")
                .with_files(vec!["ui/src/board".into()]),
        ];
        let steps = vec![
            Board { ticket: "T-101", from: TicketState::Backlog, to: TicketState::Assigned },
            Worker { slot: 1, from: WorkerState::Idle, to: WorkerState::Booting },
            Worker { slot: 1, from: WorkerState::Booting, to: WorkerState::Working },
            Board { ticket: "T-101", from: TicketState::Assigned, to: TicketState::InProgress },
            Tool { slot: 1, ticket: "T-101", tool: "Read" },
            Board { ticket: "T-102", from: TicketState::Backlog, to: TicketState::Assigned },
            Worker { slot: 2, from: WorkerState::Idle, to: WorkerState::Working },
            Board { ticket: "T-102", from: TicketState::Assigned, to: TicketState::InProgress },
            Tool { slot: 2, ticket: "T-102", tool: "Edit" },
            Worker { slot: 1, from: WorkerState::Working, to: WorkerState::Blocked },
            Note { level: NoticeLevel::Info, text: "worker-1 asked the lead which channel to push over" },
            Mail { from: "lead", to: "1", kind: "reply" },
            Worker { slot: 1, from: WorkerState::Blocked, to: WorkerState::Working },
            Mail { from: "lead", to: "1", kind: "dm" },
            Tool { slot: 1, ticket: "T-101", tool: "Write" },
            Gate { ticket: "T-101", slot: 1, outcome: GateOutcome::Pass },
            Board { ticket: "T-101", from: TicketState::InProgress, to: TicketState::InReview },
            Review { ticket: "T-101", reviewer_slot: 3, outcome: ReviewOutcome::Approved },
            Report { ticket: "T-101", slot: 1, status: ReportStatus::Done },
            Board { ticket: "T-101", from: TicketState::InReview, to: TicketState::Done },
            Worker { slot: 1, from: WorkerState::Working, to: WorkerState::Idle },
            Tool { slot: 2, ticket: "T-102", tool: "Bash" },
            Gate { ticket: "T-102", slot: 2, outcome: GateOutcome::Pass },
            Board { ticket: "T-102", from: TicketState::InProgress, to: TicketState::InReview },
        ];
        DemoPlan { tickets, steps }
    }

    /// Apply one step to the store. TicketState steps also move the board row so a
    /// board refetch reflects the change; every step appends its feed event.
    pub fn apply(store: &Arc<dyn Store>, step: &DemoStep) -> anyhow::Result<()> {
        match *step {
            DemoStep::Board { ticket, from, to } => {
                store.set_ticket_state(ticket, to)?;
                store.append_event(&FleetEvent::TicketState { ticket: ticket.into(), from, to })?;
            }
            DemoStep::Worker { slot, from, to } => {
                store.append_event(&FleetEvent::WorkerState { slot, from, to })?;
            }
            DemoStep::Tool { slot, ticket, tool } => {
                store.append_event(&FleetEvent::ToolActivity {
                    slot,
                    ticket: ticket.into(),
                    tool: tool.into(),
                })?;
            }
            DemoStep::Mail { from, to, kind } => {
                store.append_event(&FleetEvent::Mail {
                    id: format!("m-{from}-{to}-{kind}"),
                    from: from.into(),
                    to: to.into(),
                    kind: kind.into(),
                })?;
            }
            DemoStep::Gate { ticket, slot, outcome } => {
                store.append_event(&FleetEvent::GateResult { ticket: ticket.into(), slot, outcome })?;
            }
            DemoStep::Review { ticket, reviewer_slot, outcome } => {
                store.append_event(&FleetEvent::ReviewResult {
                    ticket: ticket.into(),
                    reviewer_slot,
                    outcome,
                })?;
            }
            DemoStep::Report { ticket, slot, status } => {
                store.append_event(&FleetEvent::ReportFiled { ticket: ticket.into(), slot, status })?;
            }
            DemoStep::Note { level, text } => {
                store.append_event(&FleetEvent::Notice { level, text: text.into() })?;
            }
        }
        Ok(())
    }

    /// Seed the two tickets, then apply each step on a timer so the shell animates.
    pub async fn run(store: Arc<dyn Store>, tick: Duration) {
        let plan = plan();
        for ticket in &plan.tickets {
            if let Err(e) = store.upsert_ticket(ticket) {
                eprintln!("demo: seed ticket {}: {e}", ticket.id);
                return;
            }
        }
        for step in &plan.steps {
            tokio::time::sleep(tick).await;
            if let Err(e) = apply(&store, step) {
                eprintln!("demo: apply step {step:?}: {e}");
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fleetor_db::SqliteStore;

    /// The demo plan is a real, ordered lifecycle: applied to a store it appends
    /// one event per step and leaves the board where the script says — proving the
    /// stand-in exercises the same store paths the live fleet will.
    #[test]
    fn demo_plan_drives_the_store_to_the_scripted_board() {
        let store: Arc<dyn Store> = Arc::new(SqliteStore::open_in_memory().unwrap());
        let plan = demo::plan();
        for t in &plan.tickets {
            store.upsert_ticket(t).unwrap();
        }
        for step in &plan.steps {
            demo::apply(&store, step).unwrap();
        }

        // One appended event per step, in order, all readable back from the log.
        let events = store.events_since(0).unwrap();
        assert_eq!(events.len(), plan.steps.len(), "every step appends exactly one event");

        // The board ended where the script drives it: T-101 done, T-102 in review.
        let board = store.tickets().unwrap();
        let state = |id: &str| board.iter().find(|t| t.id == id).map(|t| t.state);
        assert_eq!(state("T-101"), Some(TicketState::Done));
        assert_eq!(state("T-102"), Some(TicketState::InReview));
    }
}
