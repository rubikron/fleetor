//! The routing hub — the server side of the fleet socket.
//!
//! One [`Hub`] serves every `fleet` CLI invocation in the fleet. It does exactly
//! two things: it decides *where* a message goes, and it writes down what
//! happened to it. It does not own the terminals — the app does, behind
//! [`AppCommand`] — and it does not hold anything.
//!
//! **There is no queue here.** Phase 5 removed the mail table, the turn boundary,
//! the ask/reply waiters and the lead event long-poll along with the headless
//! fleet that needed them. A live `claude` TUI is always writable, so a message
//! has nowhere to wait: it is typed into the target terminal, and whether that
//! worked is the answer the sender gets.
//!
//! **Persist-then-emit, after the ack.** `deliver` asks the app first and logs
//! second, so the feed records the real outcome and never an intention. That
//! ordering is the whole reason the app seam is request/response rather than
//! fire-and-forget.

use anyhow::{Context, Result};
use fleetor_core::command::Command;
use fleetor_core::event::FleetEvent;
use fleetor_core::message::Message;
use fleetor_core::pane::{PaneEntry, PaneId};
use fleetor_core::wire::{Hello, Op, OpResult, Request, Response};
use fleetor_core::{ids, Store};
use fleetor_ipc::{Conn, Transport};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};

/// What the hub asks the fleet app to do on its behalf. The hub routes,
/// something else owns the machinery — but **request/response** rather than
/// fire-and-forget, because `fleet send 3` needs a real yes/no and only the pty
/// registry knows whether pane 3 is alive.
#[derive(Debug)]
pub enum AppCommand {
    /// Type `text` into `to`'s terminal. Answer with whether the bytes were
    /// queued to a live pty.
    Deliver { to: PaneId, text: String, ack: oneshot::Sender<DeliveryResult> },
    /// Type an already-allowlisted slash command into `to`'s terminal (D-045).
    ///
    /// A variant of its own rather than a flag on `Deliver`, for two reasons that
    /// are both correctness rather than taste: `Deliver`'s batch-and-join
    /// contract (D-039) must stay exactly as it is, and a command that got joined
    /// into a batch would no longer have its `/` in column 0 — which is the whole
    /// difference between a command and a sentence about one.
    Command { to: PaneId, command: String, ack: oneshot::Sender<DeliveryResult> },
    /// Every pane the app currently holds, with its state.
    Roster { ack: oneshot::Sender<Vec<PaneEntry>> },
}

/// The app's answer to an [`AppCommand::Deliver`]. `accepted` means the pane was
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

/// Each of these becomes a sentence on the sending model's stderr, so each says
/// what to do about it rather than only what went wrong.
const APP_GONE: &str = "the fleet app is not accepting commands — its window may have closed";
const APP_DROPPED: &str =
    "the fleet app took the message but never answered — treat it as not delivered";

struct HubState {
    /// Who last got a message *through* to each pane — the target `fleet reply`
    /// resolves to. Only successful deliveries land here: replying to a pane that
    /// never actually heard from you is not a reply.
    last_inbound_from: Mutex<HashMap<PaneId, PaneId>>,
}

/// The routing hub. Cheap to clone (everything shared behind `Arc`).
pub struct Hub {
    state: Arc<HubState>,
    store: Arc<dyn Store>,
    app: mpsc::UnboundedSender<AppCommand>,
}

impl Hub {
    /// A hub whose pane ops are served by the fleet app holding the pty registry.
    /// There is no other kind — a hub with nowhere to deliver could only ever
    /// refuse, and pretending otherwise is what a `None` app would have bought.
    pub fn new(store: Arc<dyn Store>, app: mpsc::UnboundedSender<AppCommand>) -> Arc<Hub> {
        Arc::new(Hub {
            state: Arc::new(HubState { last_inbound_from: Mutex::new(HashMap::new()) }),
            store,
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
        while let Some(req) = conn.read_json::<Request>().await? {
            let id = req.id.clone();
            let result = self.handle(hello.pane, req.op).await;
            conn.write_json(&Response::new(id, result)).await?;
        }
        Ok(())
    }

    async fn handle(&self, from: PaneId, op: Op) -> OpResult {
        match op {
            Op::Send { to, text } => self.send(from, to, text).await,
            Op::Broadcast { text } => self.broadcast(from, text).await,
            Op::Reply { text } => self.reply(from, text).await,
            Op::Cmd { to, command, why } => self.cmd(from, to, command, why).await,
            Op::Roster => self.roster().await,
        }
    }

    /// `fleet cmd <pane|self> "<slash command>" --why "<reason>"` (D-045).
    ///
    /// The parallel arm to [`Hub::send`], and deliberately not a branch inside
    /// it. Three things differ, and each is a thing a message must never do:
    ///
    ///  - **It may target the sender.** `fleet cmd self "/compact …"` is the
    ///    ordinary use. The self-send guard in `send` above is untouched — a pane
    ///    messaging itself is still nonsense, a pane compacting itself is not.
    ///  - **It is refused at accept time against a constant.** `Command::new`
    ///    checks the allowlist and the `why` and answers with a sentence for the
    ///    sender's stderr. That refusal happens *before* anything enters the
    ///    delivery path, which is the class of refusal D-034 keeps. After it,
    ///    nothing delays, drops or alters the command.
    ///  - **It is delivered unframed.** No `[fleet · …]`, because the `/` has to
    ///    be the first character in the input box.
    ///
    /// A refused command is an error to its sender and **not** a log entry, for
    /// the same reason a refused self-send is not: nothing happened to a pane.
    /// A command that was accepted and then failed at the pty *is* logged, with
    /// `accepted: false` — that one is a fact about a terminal.
    async fn cmd(&self, from: PaneId, to: PaneId, command: String, why: String) -> OpResult {
        let cmd = match Command::new(from, to, command, why) {
            Ok(cmd) => cmd,
            Err(message) => return OpResult::Error { message },
        };

        let outcome = self.ask_app_command(to, cmd.keystrokes().to_string()).await;
        let msg_id = cmd.id.clone();
        let accepted = outcome.accepted;
        let detail = outcome.detail.clone();
        // Log after the write, like every other outcome (Tier 1.6). A command is
        // never a reply target: `fleet reply` answers whoever *said* something,
        // and nobody said anything here.
        self.emit(cmd.into_event(accepted, detail.clone()));
        OpResult::Delivered { msg_id, accepted, detail }
    }

    /// `fleet send <pane> "<text>"`.
    async fn send(&self, from: PaneId, to: PaneId, text: String) -> OpResult {
        if from == to {
            return OpResult::Error { message: format!("{from} cannot message itself") };
        }
        self.deliver(Message::direct(from, to, text)).await
    }

    /// `fleet broadcast "<text>"` — one gesture, N−1 legs sharing a `group` id.
    ///
    /// Targets come from the **app's** roster, not from config: a fan-out should
    /// reach the panes that exist, not the panes someone once declared.
    async fn broadcast(&self, from: PaneId, text: String) -> OpResult {
        let targets: Vec<PaneId> = match self.app_roster().await {
            Ok(panes) => panes.into_iter().map(|e| e.pane).filter(|p| *p != from).collect(),
            Err(e) => return e,
        };
        if targets.is_empty() {
            return OpResult::Error { message: "there is nobody else in the fleet".to_string() };
        }

        let group = ids::new_id("grp");
        let mut failures: Vec<String> = Vec::new();
        for to in targets {
            let msg = Message::in_group(from, to, text.clone(), group.clone());
            let outcome = self.ask_app(to, msg.framed()).await;
            if !outcome.accepted {
                failures.push(format!("{to}: {}", outcome.detail.as_deref().unwrap_or("rejected")));
            }
            self.log_message(msg, &outcome);
        }
        // Accepted means *every* leg landed. Anything weaker exits the `fleet`
        // CLI zero, and the brief has taught the model that zero means the bytes
        // reached a terminal — so a partial fan-out would read as a full one and
        // the panes that missed it would never be followed up.
        OpResult::Delivered {
            msg_id: group,
            accepted: failures.is_empty(),
            detail: (!failures.is_empty()).then(|| failures.join("; ")),
        }
    }

    /// `fleet reply "<text>"` — to whoever last got through to this pane.
    async fn reply(&self, from: PaneId, text: String) -> OpResult {
        let last = self.last_inbound().get(&from).copied();
        let Some(to) = last else {
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
    /// panes are actually running right now.
    async fn roster(&self) -> OpResult {
        match self.app_roster().await {
            Ok(panes) => OpResult::Roster { panes },
            Err(e) => e,
        }
    }

    /// The one place the fleet's membership is read from. There is deliberately no
    /// second answer derived from config: a pane the app is running must be
    /// reachable even if config disagrees, and a config entry the app never
    /// spawned must not look reachable.
    async fn app_roster(&self) -> Result<Vec<PaneEntry>, OpResult> {
        let (ack, rx) = oneshot::channel();
        if self.app.send(AppCommand::Roster { ack }).is_err() {
            return Err(OpResult::Error { message: APP_GONE.to_string() });
        }
        match rx.await {
            Ok(panes) => Ok(panes),
            Err(_) => Err(OpResult::Error { message: APP_DROPPED.to_string() }),
        }
    }

    /// Ask the app to type one message into its target, then log what actually
    /// happened.
    async fn deliver(&self, msg: Message) -> OpResult {
        let outcome = self.ask_app(msg.to, msg.framed()).await;
        let msg_id = msg.id.clone();
        let accepted = outcome.accepted;
        let detail = outcome.detail.clone();
        self.log_message(msg, &outcome);
        OpResult::Delivered { msg_id, accepted, detail }
    }

    fn log_message(&self, msg: Message, outcome: &DeliveryResult) {
        // A broadcast leg must not become the recipient's reply target. It was
        // not addressed to them — the brief tells them not to answer it — and
        // letting it overwrite `last_inbound_from` silently redirects their next
        // `fleet reply`, which the brief calls their usual move, to a pane that
        // never spoke to them. That is the one failure where a message arrives
        // somewhere it was never meant to go.
        if outcome.accepted && !msg.is_broadcast() {
            self.last_inbound().insert(msg.to, msg.from);
        }
        self.emit(msg.into_event(outcome.accepted, outcome.detail.clone()));
    }

    /// One `Deliver` round-trip. Every failure mode answers with a sentence the
    /// `fleet` CLI can print to stderr for the model to read and act on.
    async fn ask_app(&self, to: PaneId, text: String) -> DeliveryResult {
        let (ack, rx) = oneshot::channel();
        if self.app.send(AppCommand::Deliver { to, text, ack }).is_err() {
            return DeliveryResult::rejected(APP_GONE);
        }
        // No deadline. The round trip is a HashMap lookup and a pty write; a
        // ceiling here could only ever turn a slow delivery into a *reported
        // failure for a message that still arrives* — the command is already in
        // the channel when a timer would fire, so the timeout cannot cancel it.
        // Duplicate sends and a log that lies are worse than waiting (D-034).
        match rx.await {
            Ok(result) => result,
            Err(_) => DeliveryResult::rejected(APP_DROPPED),
        }
    }

    /// [`Hub::ask_app`]'s sibling for commands.
    ///
    /// Six duplicated lines rather than a shared helper taking a closure,
    /// on purpose: the whole argument of D-045 is that the command path is
    /// *parallel* to the message path rather than a mode of it, and the message
    /// path's diff for this package has to be empty. A shared helper would put
    /// the two back in one piece of code that has to reason about which it is
    /// serving — which is the drift this package exists to avoid.
    async fn ask_app_command(&self, to: PaneId, command: String) -> DeliveryResult {
        let (ack, rx) = oneshot::channel();
        if self.app.send(AppCommand::Command { to, command, ack }).is_err() {
            return DeliveryResult::rejected(APP_GONE);
        }
        // No deadline, for the same reason `ask_app` has none (D-034).
        match rx.await {
            Ok(result) => result,
            Err(_) => DeliveryResult::rejected(APP_DROPPED),
        }
    }

    fn last_inbound(&self) -> std::sync::MutexGuard<'_, HashMap<PaneId, PaneId>> {
        self.state.last_inbound_from.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Append to the log. A store failure is reported to stderr and nowhere else:
    /// the message itself has already been delivered or refused, and failing the
    /// caller's send over a logging problem would make it resend something that
    /// already arrived.
    fn emit(&self, event: FleetEvent) {
        if let Err(e) = self.store.append_event(&event) {
            eprintln!("hub: could not append {}: {e}", event.kind());
        }
    }
}
