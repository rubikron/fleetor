//! The Phase 4 **multi-worker fleet runner** (BUILDING §6): the piece that finally
//! unifies the two halves Phases 1–3 built deliberately apart — the sync
//! stdin/stdout supervisor ([`crate::supervisor`]) and the async socket
//! [`hub`](crate::hub) — into one running fleet.
//!
//! What it wires together, per the phase2-spikes "wiring checklist":
//!  - one [`Hub`] bound on the fleet socket, serving every worker's MCP shim and
//!    the lead;
//!  - N real workers, each spawned with [`FleetWiring`] so its `claude` process
//!    launches the shim (MCP → hub) and runs the `Stop` hook (mid-turn mail);
//!  - a **lead loop** that long-polls `await_events`, answers a blocked
//!    `ask_lead`, and (4a) sends mid-turn mail — the stand-in for the real
//!    orchestrator TUI, which takes over this seat in Phase 4d.
//!
//! **The sync↔async seam:** the hub and lead loop live on the tokio runtime;
//! each worker's *supervisor* loop is synchronous (`std::process` + threads), so
//! it runs on a [`tokio::task::spawn_blocking`] thread. The two channels are
//! bridged through the shared, persisted event log — **no cross-runtime channel**:
//! the worker's `fleet.report` MCP call travels the socket to the hub, which
//! appends `ReportFiled`; the sync supervisor reads that as its **primary**
//! done-signal (4b/D-019), with the transcript scrape demoted to a backstop.
//! (Idle→stdin / opportunistic-piggyback mail delivery — D-015 — land with the
//! real orchestrator in 4c/4d; turn-boundary Stop-hook drain covers 4a/4b.)

use crate::hub::{AssignCommand, Hub, HubConfig};
use crate::supervisor::{run_ticket, Outcome, SuperviseOptions};
use anyhow::{Context, Result};
use fleetor_cc::spawn::WorkerConfig;
use fleetor_cc::{AgentProcess, RealClaude};
use fleetor_core::wire::{Hello, LeadEvent, LeadEventKind, Op, OpResult};
use fleetor_core::{Party, Store, Ticket};
use fleetor_ipc::{Client, Transport};
use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, Notify};

/// One worker's assignment: which process to drive, and the ticket it runs.
///
/// Built through [`WorkerSpec::real`] (a wired `claude`, whose MCP + Stop-hook
/// files the runner materializes before spawn) or [`WorkerSpec::fake`] (a
/// `fake-claude` that dials the socket itself — the free integration path). The
/// seam is [`AgentProcess`], so the runner drives both identically.
pub struct WorkerSpec {
    /// The process the supervisor drives over stdin/stdout.
    pub agent: Box<dyn AgentProcess + Send>,
    /// The ticket this worker is assigned.
    pub ticket: Ticket,
    /// Logical slot (1..=N) — labels events, the transcript, and socket identity.
    pub slot: u8,
    /// Where to tee this worker's raw transcript. `None` disables capture.
    pub raw_log: Option<PathBuf>,
    /// Per-ticket wall-clock budget (seconds).
    pub wall_secs: u64,
    /// A real worker's config, so the runner writes its `fleet` MCP config and
    /// Stop-hook `settings.json` before spawn (phase2-spikes checklist). `None`
    /// for a fake worker that connects to the socket directly.
    pub config: Option<WorkerConfig>,
}

impl WorkerSpec {
    /// A real, fully-wired `claude` worker. `config` **must** carry
    /// [`FleetWiring`](fleetor_cc::spawn::FleetWiring) for the shim + Stop hook to
    /// reach the hub.
    pub fn real(config: WorkerConfig, ticket: Ticket, slot: u8, raw_log: Option<PathBuf>, wall_secs: u64) -> Self {
        Self {
            agent: Box::new(RealClaude { config: config.clone() }),
            ticket,
            slot,
            raw_log,
            wall_secs,
            config: Some(config),
        }
    }

    /// A fake worker driven over stdin/stdout that dials `FLEET_SOCKET` itself —
    /// the free, deterministic integration path. No fleet config to materialize.
    pub fn fake(agent: Box<dyn AgentProcess + Send>, ticket: Ticket, slot: u8, raw_log: Option<PathBuf>, wall_secs: u64) -> Self {
        Self { agent, ticket, slot, raw_log, wall_secs, config: None }
    }
}

/// How the lead loop answers the fleet during a run. A deliberately thin
/// stand-in for the orchestrator TUI (Phase 4d): enough to prove the messaging
/// wiring end-to-end (the 4a exit test) without a human in the seat.
#[derive(Clone)]
pub struct LeadPolicy {
    /// Reply sent for any `ask_lead` question.
    pub answer: String,
    /// If set, mail the runner sends to `(slot, text)` right after answering a
    /// question — the mid-turn delivery the worker's Stop hook drains at its
    /// next turn boundary (handoff §5).
    pub mail_after_answer: Option<(u8, String)>,
}

impl LeadPolicy {
    /// A lead that answers every question the same way and sends no mail.
    pub fn answering(answer: impl Into<String>) -> Self {
        Self { answer: answer.into(), mail_after_answer: None }
    }

    /// Also send one piece of mid-turn mail to `slot` after answering.
    pub fn with_mail(mut self, slot: u8, text: impl Into<String>) -> Self {
        self.mail_after_answer = Some((slot, text.into()));
        self
    }
}

/// What a fleet run produced.
#[derive(Debug, Clone, PartialEq)]
pub struct FleetOutcome {
    /// Each worker's supervisor outcome, by slot.
    pub workers: Vec<(u8, Outcome)>,
    /// How many `ask_lead` questions the lead answered.
    pub questions_answered: usize,
    /// How many pieces of mid-turn mail the lead sent.
    pub mail_sent: usize,
}

/// Boot the hub, spawn every wired worker, run the lead loop, and drive every
/// ticket to its terminal outcome. Returns once all workers finish (the lead
/// loop and hub are torn down with the run).
pub async fn run_fleet(
    store: Arc<dyn Store>,
    transport: Arc<dyn Transport>,
    workers: Vec<WorkerSpec>,
    hub_config: HubConfig,
    lead: LeadPolicy,
) -> Result<FleetOutcome> {
    // --- hub on the socket ---
    let hub = Hub::new(store.clone(), hub_config);
    let listener = transport.bind().await.context("binding fleet socket")?;
    let hub_task = tokio::spawn(hub.serve(listener));

    // --- lead loop (stand-in orchestrator) ---
    let stop = Arc::new(AtomicBool::new(false));
    let lead_client = Client::connect(&*transport, Hello::new(Party::Lead))
        .await
        .context("lead connecting to the fleet socket")?;
    let lead_task = tokio::spawn(run_lead_loop(lead_client, lead, stop.clone()));

    // --- workers, each on a blocking thread (sync supervisor) ---
    let mut worker_tasks = Vec::with_capacity(workers.len());
    for spec in workers {
        // Real workers need their MCP + Stop-hook files on disk before spawn.
        if let Some(config) = &spec.config {
            config
                .write_fleet_config()
                .with_context(|| format!("writing fleet config for worker-{}", spec.slot))?;
        }
        let store = store.clone();
        let handle = tokio::task::spawn_blocking(move || {
            let opts = SuperviseOptions {
                slot: spec.slot,
                raw_log: spec.raw_log.clone(),
                max_turns: 2,
                idle_timeout: Duration::from_secs(spec.wall_secs.max(1)),
            };
            let outcome = run_ticket(spec.agent.as_ref(), &spec.ticket, &*store, &opts);
            (spec.slot, outcome)
        });
        worker_tasks.push(handle);
    }

    // --- join workers ---
    let mut results = Vec::with_capacity(worker_tasks.len());
    for task in worker_tasks {
        let (slot, outcome) = task.await.context("worker task panicked")?;
        results.push((slot, outcome.with_context(|| format!("worker-{slot} supervisor"))?));
    }

    // --- tear down the lead loop and hub ---
    stop.store(true, Ordering::SeqCst);
    let (questions_answered, mail_sent) = lead_task.await.context("lead task panicked")?;
    hub_task.abort();

    Ok(FleetOutcome { workers: results, questions_answered, mail_sent })
}

/// Long-poll the hub for lead events until `stop` is set, answering questions
/// per `policy`. Returns the (answered, mail-sent) tallies. A short poll window
/// lets the loop notice `stop` promptly once the workers are done.
async fn run_lead_loop(mut lead: Client, policy: LeadPolicy, stop: Arc<AtomicBool>) -> (usize, usize) {
    const POLL_MS: u64 = 400;
    let mut answered = 0usize;
    let mut mail_sent = 0usize;

    while !stop.load(Ordering::SeqCst) {
        let events = match lead.call(Op::AwaitEvents { timeout_ms: POLL_MS }).await {
            Ok(OpResult::Events { events }) => events,
            Ok(_) => Vec::new(),
            // The hub went away (run tearing down) — stop cleanly.
            Err(_) => break,
        };
        for ev in events {
            if let Some((a, m)) = handle_lead_event(&mut lead, &policy, &ev).await {
                answered += a;
                mail_sent += m;
            }
        }
    }
    (answered, mail_sent)
}

/// Answer one lead event. Returns `(answered, mail_sent)` deltas, or `None` for a
/// notice the lead just observes. A socket error is swallowed (the run is likely
/// tearing down); the tally simply doesn't advance.
async fn handle_lead_event(lead: &mut Client, policy: &LeadPolicy, ev: &LeadEvent) -> Option<(usize, usize)> {
    match &ev.kind {
        LeadEventKind::Question { .. } => {
            lead.call(Op::Reply { event_id: ev.id.clone(), text: policy.answer.clone() }).await.ok()?;
            let mut mail = 0;
            if let Some((slot, text)) = &policy.mail_after_answer {
                if lead.call(Op::Send { to: *slot, text: text.clone() }).await.is_ok() {
                    mail = 1;
                }
            }
            Some((1, mail))
        }
        LeadEventKind::Notice { .. } => None,
    }
}

// ============================ Phase 4d: dynamic fleet ============================

/// Builds a spawnable worker from an assigned ticket. The runner owns this
/// because only it knows how to construct a wired real worker vs. a fake — the
/// hub just routes the [`AssignCommand`]. The returned [`WorkerSpec`]'s `slot`
/// should match `ticket.slot`.
pub type WorkerFactory = Arc<dyn Fn(&Ticket) -> WorkerSpec + Send + Sync>;

/// The Phase 4d **dynamic fleet**: workers are spawned on demand as the lead
/// calls `assign` over the hub — not from a fixed list up front ([`run_fleet`]).
///
/// The lead seat is now *external*: a real orchestrator (or, in tests, a fake one)
/// drives the fleet through the lead MCP tools, replacing the scripted
/// [`LeadPolicy`] loop. `driver` runs that session to completion; when it
/// returns, the runner stops accepting new work, waits for every spawned worker
/// to finish (the hub stays up so they can still report over the socket), and
/// returns each worker's outcome by slot.
///
/// The hub↔runner bridge is a plain mpsc channel: the hub's `assign` forwards an
/// [`AssignCommand`]; this dispatch loop turns each into a supervised worker via
/// `factory` on a [`tokio::task::spawn_blocking`] thread — the same sync↔async
/// seam `run_fleet` uses (D-018), now fed dynamically.
pub async fn run_dynamic_fleet<D, Fut>(
    store: Arc<dyn Store>,
    transport: Arc<dyn Transport>,
    hub_config: HubConfig,
    factory: WorkerFactory,
    driver: D,
) -> Result<Vec<(u8, Outcome)>>
where
    D: FnOnce() -> Fut,
    Fut: Future<Output = Result<()>>,
{
    // --- hub on the socket, wired to accept assignments ---
    let (assign_tx, mut assign_rx) = mpsc::unbounded_channel::<AssignCommand>();
    let hub = Hub::with_dispatcher(store.clone(), hub_config, assign_tx);
    let listener = transport.bind().await.context("binding fleet socket")?;
    let hub_task = tokio::spawn(hub.serve(listener));

    // --- dispatch loop: one supervised worker per assign, on a blocking thread ---
    let handles = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(Notify::new());
    let dispatch = tokio::spawn({
        let handles = handles.clone();
        let stop = stop.clone();
        let store = store.clone();
        async move {
            loop {
                let cmd = tokio::select! {
                    biased;
                    _ = stop.notified() => break,
                    cmd = assign_rx.recv() => match cmd {
                        Some(c) => c,
                        None => break, // all senders (the hub) gone
                    },
                };
                let spec = factory(&cmd.ticket);
                // A real worker needs its MCP + Stop-hook files on disk before spawn.
                if let Some(config) = &spec.config {
                    if let Err(e) = config.write_fleet_config() {
                        eprintln!("runner: writing fleet config for worker-{}: {e}", spec.slot);
                        continue;
                    }
                }
                let store = store.clone();
                let handle = tokio::task::spawn_blocking(move || {
                    let opts = SuperviseOptions {
                        slot: spec.slot,
                        raw_log: spec.raw_log.clone(),
                        max_turns: 2,
                        idle_timeout: Duration::from_secs(spec.wall_secs.max(1)),
                    };
                    let outcome = run_ticket(spec.agent.as_ref(), &spec.ticket, &*store, &opts);
                    (spec.slot, outcome)
                });
                handles.lock().unwrap().push(handle);
            }
        }
    });

    // --- run the lead session, then drain ---
    let driver_res = driver().await;
    // Stop accepting new work and let the dispatch loop exit; queued-but-unstarted
    // assigns at this point are dropped (a real orchestrator isn't mid-assign at
    // session end). Then join the workers with the hub still serving.
    stop.notify_one();
    let _ = dispatch.await;
    driver_res?;

    let handles = std::mem::take(&mut *handles.lock().unwrap());
    let mut results = Vec::with_capacity(handles.len());
    for handle in handles {
        let (slot, outcome) = handle.await.context("worker task panicked")?;
        results.push((slot, outcome.with_context(|| format!("worker-{slot} supervisor"))?));
    }
    hub_task.abort();
    Ok(results)
}
