//! The Phase 2 routing hub — the server side of the fleet socket (handoff §2, §4).
//!
//! One [`Hub`] serves every client connection: workers (via their MCP shim) and
//! the lead (the CLI now, the real TUI in Phase 4). It routes the fleet tool
//! surface:
//!
//!  - `ask_lead` — the one worker→lead **blocking** call: the question is queued
//!    for the lead and the worker's response is held until `reply` arrives or the
//!    ask times out (a "park" answer, never a deadlock). Tier-1.5.
//!  - `notify_lead` — fire-and-forget worker→lead notice.
//!  - `dm` / `broadcast` — async worker→peer mail, persisted and drained at the
//!    peer's turn boundary. Never blocks — there is no worker↔worker blocking
//!    primitive (Tier-1.5).
//!  - `drain_mail` — the Stop-hook / turn-boundary pull of queued mail.
//!  - `await_events` / `inbox` — the lead's long-poll and non-blocking drain.
//!  - `reply` / `send` — lead→worker reply (unblocks an ask) and steering (mail).
//!
//! Mail is persisted through the [`Store`] seam (the source of truth, so it
//! survives a crash); questions/notices and reply waiters are in-memory and
//! transient (a crash simply drops an in-flight ask, which times out).

use anyhow::{Context, Result};
use fleetor_core::envelope::{Envelope, MessageKind, Party};
use fleetor_core::event::{FleetEvent, WorkerState};
use fleetor_core::wire::{Hello, LeadEvent, LeadEventKind, Op, OpResult, Request, Response};
use fleetor_core::{ids, Store};
use fleetor_ipc::{Conn, Transport};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{oneshot, Notify};

/// Answer returned to a blocked `ask_lead` when the ask times out with no human
/// reply — so a worker never deadlocks on an AFK lead (handoff §5).
pub const ASK_TIMEOUT_ANSWER: &str =
    "No answer within the window — use your judgment, or park this and move on.";

/// Hub knobs (Tier-2 defaults).
#[derive(Debug, Clone)]
pub struct HubConfig {
    /// The worker slots the fleet knows about — used to fan out `broadcast` and
    /// to validate a `dm`/`send` target.
    pub slots: Vec<u8>,
    /// How long a worker's `ask_lead` blocks before returning the park answer.
    pub ask_timeout: Duration,
}

impl Default for HubConfig {
    fn default() -> Self {
        Self { slots: vec![1, 2, 3, 4], ask_timeout: Duration::from_secs(600) }
    }
}

struct HubState {
    /// Actionable worker→lead traffic awaiting the lead (questions, notices).
    lead_events: Mutex<VecDeque<LeadEvent>>,
    /// Wakes a lead parked in `await_events` when an event is enqueued.
    lead_notify: Notify,
    /// Pending `ask_lead` waiters: question id → the channel that delivers the
    /// reply text back to the blocked worker handler.
    waiters: Mutex<HashMap<String, oneshot::Sender<String>>>,
    config: HubConfig,
}

/// The routing hub. Cheap to clone (everything shared behind `Arc`).
pub struct Hub {
    state: Arc<HubState>,
    store: Arc<dyn Store>,
}

impl Hub {
    pub fn new(store: Arc<dyn Store>, config: HubConfig) -> Arc<Hub> {
        Arc::new(Hub {
            state: Arc::new(HubState {
                lead_events: Mutex::new(VecDeque::new()),
                lead_notify: Notify::new(),
                waiters: Mutex::new(HashMap::new()),
                config,
            }),
            store,
        })
    }

    /// Bind the transport, then serve until cancelled.
    pub async fn run(self: Arc<Self>, transport: Arc<dyn Transport>) -> Result<()> {
        let listener = transport.bind().await.context("binding hub socket")?;
        self.serve(listener).await
    }

    /// Serve an already-bound listener until the task is cancelled. Each
    /// connection gets its own task; a connection error drops that client only,
    /// never the hub. (Taking a bound listener lets a caller connect without
    /// racing the bind.)
    pub async fn serve(self: Arc<Self>, listener: fleetor_ipc::Listener) -> Result<()> {
        loop {
            let conn = match listener.accept().await {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("hub: accept failed: {e}");
                    continue;
                }
            };
            let hub = self.clone();
            tokio::spawn(async move {
                if let Err(e) = hub.serve_conn(conn).await {
                    eprintln!("hub: connection ended: {e}");
                }
            });
        }
    }

    /// One connection: read the `Hello`, then loop request→response.
    async fn serve_conn(self: Arc<Self>, mut conn: Conn) -> Result<()> {
        let hello: Hello = match conn.read_json().await? {
            Some(h) => h,
            None => return Ok(()), // client hung up before saying hello
        };
        let party = hello.party;
        while let Some(req) = conn.read_json::<Request>().await? {
            let id = req.id.clone();
            let result = self.handle(party.clone(), req.op).await;
            conn.write_json(&Response::new(id, result)).await?;
        }
        Ok(())
    }

    /// Dispatch one op, enforcing the worker/lead split (Tier-1.5 is structural:
    /// a worker connection cannot invoke a lead op, and only `ask_lead` blocks).
    async fn handle(&self, party: Party, op: Op) -> OpResult {
        match (&party, op) {
            (Party::Worker(slot), Op::AskLead { question, options }) => {
                self.ask_lead(*slot, question, options).await
            }
            (Party::Worker(slot), Op::NotifyLead { text }) => self.notify_lead(*slot, text),
            (Party::Worker(slot), Op::Dm { to, text }) => self.dm(Party::Worker(*slot), to, text),
            (Party::Worker(slot), Op::Broadcast { text }) => self.broadcast(*slot, text),
            (Party::Worker(slot), Op::DrainMail) => self.drain(Party::Worker(*slot)),

            (Party::Lead, Op::AwaitEvents { timeout_ms }) => self.await_events(timeout_ms).await,
            (Party::Lead, Op::Inbox) => self.inbox(),
            (Party::Lead, Op::Reply { event_id, text }) => self.reply(event_id, text),
            (Party::Lead, Op::Send { to, text }) => self.dm(Party::Lead, to, text),

            (party, op) => OpResult::Error {
                message: format!("{op:?} is not permitted from {party:?}"),
            },
        }
    }

    // ---- worker-facing ----

    async fn ask_lead(&self, slot: u8, question: String, options: Option<Vec<String>>) -> OpResult {
        let event_id = ids::new_id("q");
        let (tx, rx) = oneshot::channel();
        self.state.waiters.lock().unwrap().insert(event_id.clone(), tx);
        self.push_lead_event(LeadEvent {
            id: event_id.clone(),
            from: slot,
            kind: LeadEventKind::Question { text: question, options },
        });
        self.worker_state(slot, WorkerState::Working, WorkerState::Blocked);

        let answered = tokio::time::timeout(self.state.config.ask_timeout, rx).await;
        self.worker_state(slot, WorkerState::Blocked, WorkerState::Working);
        match answered {
            Ok(Ok(text)) => OpResult::Answer { text, answered: true },
            _ => {
                // Timed out or the waiter was dropped — return the park answer
                // and clear any stale waiter entry.
                self.state.waiters.lock().unwrap().remove(&event_id);
                OpResult::Answer { text: ASK_TIMEOUT_ANSWER.to_string(), answered: false }
            }
        }
    }

    fn notify_lead(&self, slot: u8, text: String) -> OpResult {
        self.push_lead_event(LeadEvent {
            id: ids::new_id("n"),
            from: slot,
            kind: LeadEventKind::Notice { text },
        });
        OpResult::Ack
    }

    fn dm(&self, from: Party, to: u8, text: String) -> OpResult {
        if !self.state.config.slots.contains(&to) {
            return OpResult::Error { message: format!("unknown worker slot {to}") };
        }
        self.enqueue_mail(from, Party::Worker(to), MessageKind::Dm, text)
    }

    fn broadcast(&self, from_slot: u8, text: String) -> OpResult {
        let peers: Vec<u8> =
            self.state.config.slots.iter().copied().filter(|s| *s != from_slot).collect();
        for to in peers {
            let r = self.enqueue_mail(Party::Worker(from_slot), Party::Worker(to), MessageKind::Fyi, text.clone());
            if let OpResult::Error { .. } = r {
                return r;
            }
        }
        OpResult::Ack
    }

    fn drain(&self, to: Party) -> OpResult {
        match self.store.take_mail(&to) {
            Ok(messages) => OpResult::Mail { messages },
            Err(e) => OpResult::Error { message: format!("drain failed: {e}") },
        }
    }

    // ---- lead-facing ----

    async fn await_events(&self, timeout_ms: u64) -> OpResult {
        let deadline = Duration::from_millis(timeout_ms);
        loop {
            let drained = self.take_lead_events();
            if !drained.is_empty() {
                return OpResult::Events { events: drained };
            }
            // Nothing queued — wait for a nudge, up to the deadline.
            match tokio::time::timeout(deadline, self.state.lead_notify.notified()).await {
                Ok(()) => continue,           // woke on an enqueue; re-drain
                Err(_) => return OpResult::Events { events: Vec::new() }, // timed out empty
            }
        }
    }

    fn inbox(&self) -> OpResult {
        OpResult::Events { events: self.take_lead_events() }
    }

    fn reply(&self, event_id: String, text: String) -> OpResult {
        let waiter = self.state.waiters.lock().unwrap().remove(&event_id);
        match waiter {
            Some(tx) => {
                // A dropped receiver means the ask already timed out; treat the
                // reply as harmlessly late.
                let _ = tx.send(text);
                OpResult::Ack
            }
            None => OpResult::Error {
                message: format!("no worker is waiting on {event_id} (expired or already answered)"),
            },
        }
    }

    // ---- shared helpers ----

    fn enqueue_mail(&self, from: Party, to: Party, kind: MessageKind, text: String) -> OpResult {
        let env = Envelope::new(from, to, kind, text);
        if let Err(e) = self.store.save_mail(&env) {
            return OpResult::Error { message: format!("could not persist mail: {e}") };
        }
        self.emit(FleetEvent::Mail {
            id: env.id.clone(),
            from: party_label(&env.from),
            to: party_label(&env.to),
            kind: format!("{:?}", env.kind).to_lowercase(),
        });
        OpResult::Ack
    }

    fn push_lead_event(&self, event: LeadEvent) {
        self.state.lead_events.lock().unwrap().push_back(event);
        self.state.lead_notify.notify_one();
    }

    fn take_lead_events(&self) -> Vec<LeadEvent> {
        self.state.lead_events.lock().unwrap().drain(..).collect()
    }

    fn worker_state(&self, slot: u8, from: WorkerState, to: WorkerState) {
        self.emit(FleetEvent::WorkerState { slot, from, to });
    }

    fn emit(&self, event: FleetEvent) {
        if let Err(e) = self.store.append_event(&event) {
            eprintln!("hub: failed to persist event {}: {e}", event.kind());
        }
    }
}

/// Short human/UI label for a party in the event log ("worker-2", "lead").
fn party_label(p: &Party) -> String {
    match p {
        Party::Lead => "lead".to_string(),
        Party::Worker(n) => format!("worker-{n}"),
        Party::User => "user".to_string(),
    }
}
