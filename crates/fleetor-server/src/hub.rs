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
use fleetor_core::message::Message;
use fleetor_core::pane::{PaneEntry, PaneId};
use fleetor_core::wire::{Hello, LeadEvent, LeadEventKind, Op, OpResult, Request, Response};
use fleetor_core::{ids, Store};
use fleetor_ipc::{Conn, Transport};
use fleetor_core::Ticket;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, Notify};

/// A dispatch request the lead's `assign` forwards to the runner (Phase 4d). The
/// runner turns the ticket into a spawned worker; the hub only routes it, so it
/// never needs to know how a worker is built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignCommand {
    pub ticket: Ticket,
}

/// A lead→runner control command (Phase 4d/4f). The hub only routes these over
/// the one dispatcher channel; the runner decides how to act on them. `Assign`
/// spawns a worker; `Interrupt`/`Restart` act on a slot's live worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunnerCommand {
    /// Spawn and drive a worker for this ticket (the lead's `assign`).
    Assign(AssignCommand),
    /// Kill the worker on `slot`, ending its run as `Interrupted` (the lead's
    /// `interrupt`).
    Interrupt { slot: u8 },
    /// Kill the worker on `slot` and re-dispatch its ticket fresh (the lead's
    /// `worker_restart`).
    Restart { slot: u8 },
}

/// What the hub asks the fleet app to do on its behalf (D-030). This is the
/// `RunnerCommand` seam again — the hub routes, something else owns the
/// machinery — but **request/response** rather than fire-and-forget, because
/// `fleet send 3` needs a real yes/no and only the pty registry knows whether
/// pane 3 is alive.
///
/// The hub awaits the ack and *then* logs the message with the real outcome, so
/// the feed can never claim a delivery that did not happen.
#[derive(Debug)]
pub enum AppCommand {
    /// Type `text` into `to`'s terminal. Answer with whether the bytes were
    /// queued to a live pty.
    Deliver { to: PaneId, text: String, ack: oneshot::Sender<DeliveryResult> },
    /// Every pane the app currently holds, with its state.
    Roster { ack: oneshot::Sender<Vec<PaneEntry>> },
}

/// The app's answer to a [`AppCommand::Deliver`]. `accepted` means the pane was
/// live and the bytes went to its pty — deliberately not "delivered", because
/// nothing on this side of the pty knows whether the model read them (L3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryResult {
    pub accepted: bool,
    pub detail: Option<String>,
}

impl DeliveryResult {
    pub fn accepted() -> Self {
        Self { accepted: true, detail: None }
    }

    pub fn rejected(detail: impl Into<String>) -> Self {
        Self { accepted: false, detail: Some(detail.into()) }
    }
}

/// Knobs for the pane surface, kept out of [`HubConfig`] so the pre-D-030 call
/// sites keep compiling untouched. Phase 5 folds these two structs together.
#[derive(Debug, Clone, Copy)]
pub struct PaneConfig {
    /// Ceiling on the app's delivery ack. The round-trip is a HashMap lookup and
    /// a pty write — sub-millisecond — so this exists only so a wedged app cannot
    /// park the CLI forever. It is not a delay; nothing in the path sleeps.
    pub ack_timeout: Duration,
}

impl Default for PaneConfig {
    fn default() -> Self {
        Self { ack_timeout: Duration::from_secs(2) }
    }
}

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
    /// Who last got a message *through* to each pane — the target `fleet reply`
    /// resolves to. Only successful deliveries land here: replying to a pane that
    /// never actually heard from you is not a reply.
    last_inbound_from: Mutex<HashMap<PaneId, PaneId>>,
    config: HubConfig,
    pane_config: PaneConfig,
}

/// The routing hub. Cheap to clone (everything shared behind `Arc`).
pub struct Hub {
    state: Arc<HubState>,
    store: Arc<dyn Store>,
    /// Where a lead's control commands (`assign`/`interrupt`/`worker_restart`) are
    /// forwarded (Phase 4d/4f). `None` on a static fleet (Phases 1–4c) — a control
    /// op there is answered with an error rather than silently dropped.
    dispatcher: Option<mpsc::UnboundedSender<RunnerCommand>>,
    /// Where pane deliveries go (D-030). `None` when no fleet app is attached, in
    /// which case a pane op is rejected loudly rather than logged as a success.
    app: Option<mpsc::UnboundedSender<AppCommand>>,
}

impl Hub {
    pub fn new(store: Arc<dyn Store>, config: HubConfig) -> Arc<Hub> {
        Self::build(store, config, PaneConfig::default(), None, None)
    }

    /// A hub whose control ops forward to `dispatcher` — the dynamic fleet (4d/4f).
    pub fn with_dispatcher(
        store: Arc<dyn Store>,
        config: HubConfig,
        dispatcher: mpsc::UnboundedSender<RunnerCommand>,
    ) -> Arc<Hub> {
        Self::build(store, config, PaneConfig::default(), Some(dispatcher), None)
    }

    /// A hub whose pane ops are served by a fleet app holding the pty registry
    /// (D-030) — the only constructor the TUI fleet uses.
    pub fn with_app(
        store: Arc<dyn Store>,
        config: HubConfig,
        pane_config: PaneConfig,
        app: mpsc::UnboundedSender<AppCommand>,
    ) -> Arc<Hub> {
        Self::build(store, config, pane_config, None, Some(app))
    }

    fn build(
        store: Arc<dyn Store>,
        config: HubConfig,
        pane_config: PaneConfig,
        dispatcher: Option<mpsc::UnboundedSender<RunnerCommand>>,
        app: Option<mpsc::UnboundedSender<AppCommand>>,
    ) -> Arc<Hub> {
        Arc::new(Hub {
            state: Arc::new(HubState {
                lead_events: Mutex::new(VecDeque::new()),
                lead_notify: Notify::new(),
                waiters: Mutex::new(HashMap::new()),
                last_inbound_from: Mutex::new(HashMap::new()),
                config,
                pane_config,
            }),
            store,
            dispatcher,
            app,
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
        let pane = hello.pane;
        while let Some(req) = conn.read_json::<Request>().await? {
            let id = req.id.clone();
            let result = self.handle(party.clone(), pane, req.op).await;
            conn.write_json(&Response::new(id, result)).await?;
        }
        Ok(())
    }

    /// Dispatch one op, enforcing the worker/lead split (Tier-1.5 is structural:
    /// a worker connection cannot invoke a lead op, and only `ask_lead` blocks).
    ///
    /// The pane ops (D-030) are the exception to the split: they are dispatched on
    /// the `Hello`'s pane identity, not its party, and a connection that did not
    /// declare a pane cannot invoke them at all.
    async fn handle(&self, party: Party, pane: Option<PaneId>, op: Op) -> OpResult {
        match (&party, op) {
            // ---- pane-facing (D-030): identity comes from `pane`, not `party` ----
            (_, Op::PaneSend { to, text }) => self.pane_send(pane, to, text).await,
            (_, Op::PaneBroadcast { text }) => self.pane_broadcast(pane, text).await,
            (_, Op::PaneReply { text }) => self.pane_reply(pane, text).await,
            (_, Op::Roster) => self.roster().await,

            (Party::Worker(slot), Op::AskLead { question, options }) => {
                self.ask_lead(*slot, question, options).await
            }
            (Party::Worker(slot), Op::NotifyLead { text }) => self.notify_lead(*slot, text),
            (Party::Worker(slot), Op::Dm { to, text }) => self.dm(Party::Worker(*slot), to, text),
            (Party::Worker(slot), Op::Broadcast { text }) => self.broadcast(*slot, text),
            (Party::Worker(slot), Op::DrainMail) => self.drain(Party::Worker(*slot)),
            (Party::Worker(slot), Op::Report { report }) => self.report(*slot, report),
            (Party::Worker(_), Op::WhosWorkingOn { path }) => self.whos_working_on(&path),
            (Party::Worker(slot), Op::ClaimFile { path, ticket }) => self.claim_file(*slot, &path, &ticket),
            (Party::Worker(slot), Op::BacklogAdd { text }) => self.backlog_add(*slot, &text),

            (Party::Lead, Op::Assign { ticket }) => self.assign(ticket),
            (Party::Lead, Op::AwaitEvents { timeout_ms }) => self.await_events(timeout_ms).await,
            (Party::Lead, Op::Inbox) => self.inbox(),
            (Party::Lead, Op::FleetStatus) => self.fleet_status(),
            (Party::Lead, Op::Reply { event_id, text }) => self.reply(event_id, text),
            (Party::Lead, Op::Send { to, text }) => self.dm(Party::Lead, to, text),
            (Party::Lead, Op::LeadBroadcast { text }) => self.lead_broadcast(text),
            (Party::Lead, Op::Interrupt { slot }) => {
                self.control(slot, RunnerCommand::Interrupt { slot }, "interrupt")
            }
            (Party::Lead, Op::WorkerRestart { slot }) => {
                self.control(slot, RunnerCommand::Restart { slot }, "worker_restart")
            }

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

    /// A worker files a structured report over MCP (D-008 promoted). Persist it,
    /// log a `ReportFiled` event, and surface a notice to the lead so a lead
    /// long-polling `await_events` learns a report landed.
    fn report(&self, slot: u8, report: fleetor_core::Report) -> OpResult {
        if let Err(e) = self.store.save_report(&report.ticket, slot, &report) {
            return OpResult::Error { message: format!("could not persist report: {e}") };
        }
        self.emit(FleetEvent::ReportFiled {
            ticket: report.ticket.clone(),
            slot,
            status: report.status,
        });
        self.push_lead_event(LeadEvent {
            id: ids::new_id("r"),
            from: slot,
            kind: LeadEventKind::Notice {
                text: format!("worker-{slot} filed a report on {}: {:?}", report.ticket, report.status),
            },
        });
        OpResult::Ack
    }

    fn whos_working_on(&self, path: &str) -> OpResult {
        match self.store.who_owns(path) {
            Ok(owners) => OpResult::Owners { owners },
            Err(e) => OpResult::Error { message: format!("whos_working_on failed: {e}") },
        }
    }

    fn claim_file(&self, slot: u8, path: &str, ticket: &str) -> OpResult {
        match self.store.claim_lease(path, slot, ticket) {
            Ok(grant) => OpResult::Claim { grant },
            Err(e) => OpResult::Error { message: format!("claim_file failed: {e}") },
        }
    }

    fn backlog_add(&self, slot: u8, text: &str) -> OpResult {
        let item = fleetor_core::BacklogItem::new(text.to_string(), Party::Worker(slot), None);
        match self.store.add_backlog(&item) {
            Ok(()) => OpResult::Ack,
            Err(e) => OpResult::Error { message: format!("backlog_add failed: {e}") },
        }
    }

    // ---- lead-facing ----

    /// Dispatch a ticket to the runner (Phase 4d). The ticket must name its
    /// target slot. The hub persists the ticket row so it shows up in
    /// `fleet_status` immediately, then forwards the command; the runner spawns
    /// the worker and its supervisor owns the state transitions from there.
    fn assign(&self, ticket: Ticket) -> OpResult {
        let slot = match ticket.slot {
            Some(s) => s,
            None => {
                return OpResult::Error {
                    message: "assign requires the ticket to name a target slot".to_string(),
                }
            }
        };
        if !self.state.config.slots.contains(&slot) {
            return OpResult::Error { message: format!("unknown worker slot {slot}") };
        }
        let Some(dispatcher) = &self.dispatcher else {
            return OpResult::Error {
                message: "this fleet does not accept dynamic assignment (no runner attached)"
                    .to_string(),
            };
        };
        if let Err(e) = self.store.upsert_ticket(&ticket) {
            return OpResult::Error { message: format!("could not persist ticket: {e}") };
        }
        match dispatcher.send(RunnerCommand::Assign(AssignCommand { ticket })) {
            Ok(()) => OpResult::Ack,
            Err(_) => OpResult::Error {
                message: "the runner is no longer accepting assignments".to_string(),
            },
        }
    }

    /// Route a lead control command (Phase 4f `interrupt`/`worker_restart`) to the
    /// runner. Validates the slot and requires a dynamic fleet, mirroring `assign`.
    fn control(&self, slot: u8, command: RunnerCommand, verb: &str) -> OpResult {
        if !self.state.config.slots.contains(&slot) {
            return OpResult::Error { message: format!("unknown worker slot {slot}") };
        }
        let Some(dispatcher) = &self.dispatcher else {
            return OpResult::Error {
                message: format!("this fleet does not accept {verb} (no runner attached)"),
            };
        };
        match dispatcher.send(command) {
            Ok(()) => OpResult::Ack,
            Err(_) => OpResult::Error {
                message: "the runner is no longer accepting commands".to_string(),
            },
        }
    }

    /// The board: every ticket with its state and slot (Phase 4d). How the lead
    /// sees fleet progress without reading transcripts.
    fn fleet_status(&self) -> OpResult {
        match self.store.tickets() {
            Ok(board) => OpResult::Status { board },
            Err(e) => OpResult::Error { message: format!("fleet_status failed: {e}") },
        }
    }

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

    /// Lead→all: fan one message out as mail to every configured slot (Phase 4i).
    fn lead_broadcast(&self, text: String) -> OpResult {
        for to in self.state.config.slots.clone() {
            let r = self.enqueue_mail(Party::Lead, Party::Worker(to), MessageKind::Dm, text.clone());
            if let OpResult::Error { .. } = r {
                return r;
            }
        }
        OpResult::Ack
    }

    // ---- pane-facing (D-030) ----

    /// `fleet send <pane> "<text>"`.
    async fn pane_send(&self, from: Option<PaneId>, to: PaneId, text: String) -> OpResult {
        let from = match self.sender(from) {
            Ok(p) => p,
            Err(e) => return e,
        };
        if !self.roster_ids().contains(&to) {
            return OpResult::Error { message: format!("unknown pane {to}") };
        }
        if to == from {
            return OpResult::Error { message: format!("{from} cannot message itself") };
        }
        self.deliver(Message::direct(from, to, text)).await
    }

    /// `fleet broadcast "<text>"` — one gesture, N−1 legs sharing a `group` id.
    /// Costs one token per leg, so a fan-out is exactly as expensive as sending to
    /// each pane by hand and a fountain runs out of budget quickly (L5).
    async fn pane_broadcast(&self, from: Option<PaneId>, text: String) -> OpResult {
        let from = match self.sender(from) {
            Ok(p) => p,
            Err(e) => return e,
        };
        let targets: Vec<PaneId> =
            self.roster_ids().into_iter().filter(|p| *p != from).collect();
        if targets.is_empty() {
            return OpResult::Error { message: "there is nobody else in the fleet".to_string() };
        }

        let group = ids::new_id("grp");
        let mut any_accepted = false;
        let mut failures: Vec<String> = Vec::new();
        for to in targets {
            let msg = Message::in_group(from, to, text.clone(), group.clone());
            let outcome = self.ask_app(to, msg.framed()).await;
            if outcome.accepted {
                any_accepted = true;
            } else {
                failures.push(format!("{to}: {}", outcome.detail.as_deref().unwrap_or("rejected")));
            }
            self.log_message(msg, &outcome);
        }
        OpResult::Delivered {
            msg_id: group,
            accepted: any_accepted,
            detail: (!failures.is_empty()).then(|| failures.join("; ")),
        }
    }

    /// `fleet reply "<text>"` — to whoever last got through to this pane.
    async fn pane_reply(&self, from: Option<PaneId>, text: String) -> OpResult {
        let from = match self.sender(from) {
            Ok(p) => p,
            Err(e) => return e,
        };
        let Some(to) = self.state.last_inbound_from.lock().unwrap().get(&from).copied() else {
            return OpResult::Error {
                message: format!(
                    "nobody has messaged {from} yet — there is nothing to reply to; \
                     name a pane with `fleet send <pane> \"…\"`"
                ),
            };
        };
        self.deliver(Message::direct(from, to, text)).await
    }

    /// `fleet roster` — asks the app, because only the pty registry knows which
    /// panes are actually alive right now.
    async fn roster(&self) -> OpResult {
        let Some(app) = &self.app else {
            return OpResult::Error { message: NO_APP.to_string() };
        };
        let (ack, rx) = oneshot::channel();
        if app.send(AppCommand::Roster { ack }).is_err() {
            return OpResult::Error { message: APP_GONE.to_string() };
        }
        match tokio::time::timeout(self.state.pane_config.ack_timeout, rx).await {
            Ok(Ok(panes)) => OpResult::Roster { panes },
            Ok(Err(_)) => OpResult::Error { message: APP_DROPPED.to_string() },
            Err(_) => OpResult::Error { message: self.ack_timed_out() },
        }
    }

    /// Ask the app to type one message into its target, then log what actually
    /// happened. Persist-then-emit-*after*-ack is the whole reason the app seam is
    /// request/response: the feed records the real outcome, never an intention.
    async fn deliver(&self, msg: Message) -> OpResult {
        let outcome = self.ask_app(msg.to, msg.framed()).await;
        let msg_id = msg.id.clone();
        let accepted = outcome.accepted;
        let detail = outcome.detail.clone();
        self.log_message(msg, &outcome);
        OpResult::Delivered { msg_id, accepted, detail }
    }

    fn log_message(&self, msg: Message, outcome: &DeliveryResult) {
        if outcome.accepted {
            self.state.last_inbound_from.lock().unwrap().insert(msg.to, msg.from);
        }
        self.emit(msg.into_event(outcome.accepted, outcome.detail.clone()));
    }

    /// One `Deliver` round-trip. Every failure mode answers with a sentence the
    /// `fleet` CLI can print to stderr for the model to read and act on.
    async fn ask_app(&self, to: PaneId, text: String) -> DeliveryResult {
        let Some(app) = &self.app else {
            return DeliveryResult::rejected(NO_APP);
        };
        let (ack, rx) = oneshot::channel();
        if app.send(AppCommand::Deliver { to, text, ack }).is_err() {
            return DeliveryResult::rejected(APP_GONE);
        }
        match tokio::time::timeout(self.state.pane_config.ack_timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => DeliveryResult::rejected(APP_DROPPED),
            Err(_) => DeliveryResult::rejected(self.ack_timed_out()),
        }
    }

    /// The sending pane, or the error a connection that never declared one gets.
    fn sender(&self, pane: Option<PaneId>) -> Result<PaneId, OpResult> {
        pane.ok_or_else(|| OpResult::Error {
            message: "this connection did not identify a pane — set FLEETOR_PANE and retry"
                .to_string(),
        })
    }

    fn roster_ids(&self) -> Vec<PaneId> {
        PaneId::roster(&self.state.config.slots)
    }

    fn ack_timed_out(&self) -> String {
        format!(
            "the fleet app did not answer within {:?} — it may be wedged",
            self.state.pane_config.ack_timeout
        )
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

// The three ways the app seam can fail, worded for a model reading its own Bash
// stderr — each says what happened and what it means for the message.
const NO_APP: &str = "no fleet app is attached to this hub, so nothing can be delivered";
const APP_GONE: &str = "the fleet app is shutting down and is no longer accepting deliveries";
const APP_DROPPED: &str = "the fleet app dropped the delivery without answering";

/// Short human/UI label for a party in the event log ("worker-2", "lead").
fn party_label(p: &Party) -> String {
    match p {
        Party::Lead => "lead".to_string(),
        Party::Worker(n) => format!("worker-{n}"),
        Party::User => "user".to_string(),
    }
}
