//! The socket wire contract (BUILDING §4.4) — the payloads that cross the
//! `Transport` seam between a fleet client (a worker's MCP shim, the Stop-hook
//! drainer, or the lead) and the server. Pure serde, no I/O: the framing and
//! sockets live in `fleetor-ipc`; the *shapes* are frozen here so the shim and
//! server can never drift (handoff §4).
//!
//! Framing is newline-delimited JSON — one [`Request`] or [`Response`] per line.
//! Each connection carries **one in-flight request at a time**: the client
//! writes a `Request`, the server writes back the matching `Response` (possibly
//! after a long wait, for the blocking ops), then the connection is free for the
//! next request. Blocking ops (`AskLead`, `AwaitEvents`) simply hold their
//! response — the worker parked in `ask_lead` has nothing else to do, and the
//! lead's long-poll is exactly that.

use crate::envelope::{Envelope, Party};
use crate::pane::{PaneEntry, PaneId};
use serde::{Deserialize, Serialize};

pub const WIRE_VERSION: u32 = 1;

/// The first frame a client sends after connecting: it declares its identity so
/// the server can attribute every later request without threading it through
/// each op. Slot identity for workers comes from the shim's env (Phase 2 step 3).
///
/// `pane` is the D-030 identity: a `fleet` CLI client sets it (via
/// [`Hello::for_pane`]) and the hub refuses every pane op on a connection that
/// did not. `party` is the pre-D-030 identity and goes away with the rest of the
/// headless surface in Phase 5.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub party: Party,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    pub v: u32,
}

impl Hello {
    pub fn new(party: Party) -> Self {
        Self { party, pane: None, v: WIRE_VERSION }
    }

    /// Identify as a pane — how the `fleet` CLI connects.
    pub fn for_pane(pane: PaneId) -> Self {
        let party = match pane {
            PaneId::Orch => Party::Lead,
            PaneId::Worker(n) => Party::Worker(n),
        };
        Self { party, pane: Some(pane), v: WIRE_VERSION }
    }
}

/// A client→server call. `id` is client-assigned and correlates the [`Response`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub id: String,
    #[serde(flatten)]
    pub op: Op,
}

/// The fleet tool surface as it crosses the wire. Worker-facing and lead-facing
/// ops share one enum; the server rejects a lead op from a worker connection and
/// vice-versa. Tier-1.5 (blocking is worker→lead only) is structural here: the
/// sole worker blocking op is `AskLead`; there is no worker→worker blocking op.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    // ---- worker-facing ----
    /// Blocks until the lead replies or the server times out (handoff §5).
    AskLead {
        question: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        options: Option<Vec<String>>,
    },
    /// Fire-and-forget progress to the lead.
    NotifyLead { text: String },
    /// Async direct message to a peer worker; delivered at the peer's next turn
    /// boundary (never blocks — Tier-1.5).
    Dm { to: u8, text: String },
    /// Async message to all peer workers.
    Broadcast { text: String },
    /// Pull this client's queued mail (the Stop-hook drainer / turn boundary).
    /// Returns [`OpResult::Mail`] — empty if nothing is waiting.
    DrainMail,
    /// File a structured completion (handoff §4). Promotes D-008's
    /// transcript-scrape onto the MCP surface; the scrape stays the backstop.
    Report { report: crate::report::Report },
    /// Cheap conflict check: who currently holds `path`? → [`OpResult::Owners`].
    WhosWorkingOn { path: String },
    /// Request a lease on `path` for `ticket`; may be denied if another slot
    /// holds it (handoff §4). The worker passes its own ticket id (a Tier-2
    /// signature tweak over handoff's `claim_file(path)`). → [`OpResult::Claim`].
    ClaimFile { path: String, ticket: String },
    /// Park an out-of-scope discovery instead of widening the diff (handoff §9).
    BacklogAdd { text: String },

    // ---- lead-facing ----
    /// Dispatch a ticket to its named slot (Phase 4d). The ticket carries its
    /// target `slot`; the hub forwards it to the runner, which spawns and drives
    /// a worker. Only meaningful on a dynamic fleet (a runner listening for
    /// assignments); a static fleet answers with an error. → [`OpResult::Ack`].
    Assign { ticket: crate::ticket::Ticket },
    /// Blocks up to `timeout_ms` for actionable worker→lead traffic (questions,
    /// notices). This is how the lead listens without burning turns (handoff §4).
    AwaitEvents { timeout_ms: u64 },
    /// Non-blocking drain of the same lead-event queue.
    Inbox,
    /// The board as the fleet server holds it (Phase 4d): every ticket with its
    /// state and slot. → [`OpResult::Status`].
    FleetStatus,
    /// Unblock a worker parked in `AskLead`. `event_id` is the question's id.
    Reply { event_id: String, text: String },
    /// Lead→worker steering; async, queued as mail like a `Dm` from the lead.
    Send { to: u8, text: String },
    /// Lead→**all** workers in one call (Phase 4i): queued as mail to every slot.
    /// → [`OpResult::Ack`].
    LeadBroadcast { text: String },
    /// Yank a busy worker's in-flight turn (Phase 4f): the runner kills the
    /// worker's process, ending its run as [`Interrupted`]. Only meaningful on a
    /// dynamic fleet. → [`OpResult::Ack`].
    ///
    /// [`Interrupted`]: crate::wire — see `fleetor_server::Outcome::Interrupted`
    Interrupt { slot: u8 },
    /// Restart a worker (Phase 4f): kill the current process on `slot` and
    /// re-dispatch its ticket as a fresh worker. Only meaningful on a dynamic
    /// fleet. → [`OpResult::Ack`].
    WorkerRestart { slot: u8 },

    // ---- pane-facing (D-030) ----
    //
    // These four are the whole `fleet` CLI surface. They are named `Pane*`
    // only because `Send`/`Broadcast`/`Reply` above still exist; **Phase 5
    // deletes those and renames these** to the bare verbs, at which point the
    // wire tag matches the CLI verb exactly. Nothing but the tag changes.
    /// `fleet send <pane> "<text>"` — write into one live pane's terminal.
    /// → [`OpResult::Delivered`], with `accepted: false` if the pane is not live.
    PaneSend { to: PaneId, text: String },
    /// `fleet broadcast "<text>"` — fan out to every pane except the sender. All
    /// legs share one `group` id. → [`OpResult::Delivered`] carrying that group id.
    PaneBroadcast { text: String },
    /// `fleet reply "<text>"` — route to whoever last messaged this pane. Errors
    /// if nobody has. → [`OpResult::Delivered`].
    PaneReply { text: String },
    /// `fleet roster` — every pane and its state. → [`OpResult::Roster`].
    Roster,
}

/// The server→client reply to one [`Request`]. `id` echoes the request's id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response {
    pub id: String,
    #[serde(flatten)]
    pub result: OpResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum OpResult {
    /// Op accepted, nothing to return (notify, dm, broadcast, reply, send).
    Ack,
    /// An `AskLead` was answered. `answered` is false when the answer is the
    /// timeout fallback ("no answer — use your judgment"), so the worker can
    /// tell a real reply from a park.
    Answer { text: String, answered: bool },
    /// Queued mail (`DrainMail`) — oldest first.
    Mail { messages: Vec<Envelope> },
    /// Actionable worker→lead traffic (`AwaitEvents` / `Inbox`) — oldest first.
    Events { events: Vec<LeadEvent> },
    /// The board (`FleetStatus`): every ticket with its state and assigned slot.
    Status { board: Vec<crate::ticket::Ticket> },
    /// Who holds a path (`WhosWorkingOn`) — empty if nobody.
    Owners { owners: Vec<crate::ownership::Owner> },
    /// The outcome of a `ClaimFile`: granted, or denied with the holder.
    Claim { grant: crate::ownership::LeaseGrant },
    /// The outcome of a pane send/broadcast/reply (D-030). `accepted` is true
    /// when the target pane was live and the bytes were queued to its pty —
    /// never a claim that the agent there read them (L3). The `fleet` CLI exits
    /// non-zero and prints `detail` to stderr whenever this is false, which is
    /// how a model finds out its own message went nowhere.
    ///
    /// The id field is `msg_id`, not `id`: [`Response`] flattens this enum and
    /// already carries the request `id`, so a second `id` here would collide into
    /// one JSON object and fail to deserialize.
    Delivered {
        /// The message id, or the shared group id for a broadcast.
        msg_id: String,
        accepted: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    /// Every pane and its state (`Roster`).
    Roster { panes: Vec<PaneEntry> },
    /// The op could not be served (bad party, unknown event id, etc.).
    Error { message: String },
}

/// One item the lead pulls from `await_events` / `inbox`: worker→lead traffic
/// that wants the lead's attention. `id` is the reply target for a `Question`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeadEvent {
    pub id: String,
    pub from: u8,
    #[serde(flatten)]
    pub kind: LeadEventKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum LeadEventKind {
    /// A blocking `ask_lead`; reply with `Op::Reply { event_id: id, .. }`.
    Question {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        options: Option<Vec<String>>,
    },
    /// A `notify_lead` — informational, no reply expected.
    Notice { text: String },
}

impl Request {
    pub fn new(op: Op) -> Self {
        Self { id: crate::ids::new_id("req"), op }
    }
}

impl Response {
    pub fn new(id: impl Into<String>, result: OpResult) -> Self {
        Self { id: id.into(), result }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pane::{PaneEntry, PaneId, PaneState};

    /// Both `Request` and `Response` `#[serde(flatten)]` their payload enum into
    /// the same JSON object as their own `id`, so any variant field also called
    /// `id` silently produces a duplicate key that fails to deserialize. This
    /// caught exactly that on `Delivered`; it exists so the next variant can't
    /// reintroduce it.
    #[test]
    fn every_pane_frame_survives_the_flattened_round_trip() {
        let requests = [
            Op::PaneSend { to: PaneId::Worker(2), text: "take the parser".into() },
            Op::PaneBroadcast { text: "rebasing".into() },
            Op::PaneReply { text: "on it".into() },
            Op::Roster,
        ];
        for op in requests {
            let req = Request::new(op);
            let line = serde_json::to_string(&req).unwrap();
            assert_eq!(serde_json::from_str::<Request>(&line).unwrap(), req, "{line}");
        }

        let results = [
            OpResult::Delivered { msg_id: "msg-1".into(), accepted: true, detail: None },
            OpResult::Delivered {
                msg_id: "grp-1".into(),
                accepted: false,
                detail: Some("worker-3: not live".into()),
            },
            OpResult::Roster {
                panes: vec![
                    PaneEntry::new(PaneId::Orch, PaneState::Live),
                    PaneEntry::new(PaneId::Worker(1), PaneState::Dead),
                ],
            },
        ];
        for result in results {
            let resp = Response::new("req-1", result);
            let line = serde_json::to_string(&resp).unwrap();
            assert_eq!(serde_json::from_str::<Response>(&line).unwrap(), resp, "{line}");
        }
    }

    /// The op tag is what a `fleet` verb becomes on the wire; pin it so a variant
    /// rename is a visible change rather than a silent protocol break.
    #[test]
    fn pane_ops_carry_the_tags_the_hub_dispatches_on() {
        let tag = |op: Op| {
            serde_json::to_value(Request::new(op)).unwrap()["op"].as_str().unwrap().to_string()
        };
        assert_eq!(tag(Op::PaneSend { to: PaneId::Orch, text: "x".into() }), "pane_send");
        assert_eq!(tag(Op::PaneBroadcast { text: "x".into() }), "pane_broadcast");
        assert_eq!(tag(Op::PaneReply { text: "x".into() }), "pane_reply");
        assert_eq!(tag(Op::Roster), "roster");
    }

    /// A pane hello also carries the legacy party, so one connection can serve
    /// both surfaces until Phase 5 deletes the older one.
    #[test]
    fn a_pane_hello_round_trips_and_keeps_a_matching_party() {
        for (pane, party) in
            [(PaneId::Orch, Party::Lead), (PaneId::Worker(3), Party::Worker(3))]
        {
            let hello = Hello::for_pane(pane);
            assert_eq!(hello.party, party);
            let line = serde_json::to_string(&hello).unwrap();
            assert_eq!(serde_json::from_str::<Hello>(&line).unwrap(), hello);
        }
        // A pre-D-030 hello has no pane and stays wire-compatible.
        let legacy = serde_json::to_string(&Hello::new(Party::Worker(1))).unwrap();
        assert!(!legacy.contains("pane"), "{legacy}");
        assert_eq!(serde_json::from_str::<Hello>(&legacy).unwrap().pane, None);
    }
}
