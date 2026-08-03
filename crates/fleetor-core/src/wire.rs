//! The socket wire contract (BUILDING §4.4) — the payloads that cross the
//! `Transport` seam between the `fleet` CLI running inside a pane and the hub.
//! Pure serde, no I/O: the framing and sockets live in `fleetor-ipc`; the
//! *shapes* are frozen here so the two sides can never drift.
//!
//! Framing is newline-delimited JSON — one [`Request`] or [`Response`] per line.
//! Each connection carries **one in-flight request at a time**: the client writes
//! a `Request`, the server writes back the matching `Response`, then the
//! connection is free for the next one.
//!
//! **Phase 5 emptied this file of the headless surface.** `AskLead`, `Dm`,
//! `DrainMail`, `Report`, `ClaimFile`, `Assign`, `FleetStatus`, `Interrupt` and
//! the rest went with the supervisor that served them, and the four `Pane*` ops
//! took their plain names — so the wire tag now matches the CLI verb exactly.
//! `Assign` and `FleetStatus` in particular were not kept "just in case": they
//! are the seed of the ticket system growing back.

use crate::pane::{PaneEntry, PaneId};
use serde::{Deserialize, Serialize};

pub const WIRE_VERSION: u32 = 1;

/// The first frame a client sends after connecting: it declares which pane it is,
/// so the hub can attribute every later message without threading identity
/// through each op.
///
/// There is no anonymous connection. A message from nobody could not be replied
/// to, and `fleet reply` — the verb the worker brief calls their usual move —
/// routes on exactly this.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub pane: PaneId,
    pub v: u32,
}

impl Hello {
    pub fn for_pane(pane: PaneId) -> Self {
        Self { pane, v: WIRE_VERSION }
    }
}

/// A client→server call. `id` is client-assigned and correlates the [`Response`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub id: String,
    #[serde(flatten)]
    pub op: Op,
}

/// The whole fleet tool surface: four ops, one per `fleet` verb (`whoami` needs
/// no round trip — a pane knows its own name from its environment).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    /// `fleet send <pane> "<text>"` — write into one live pane's terminal.
    /// → [`OpResult::Delivered`], with `accepted: false` if the pane is not live.
    Send { to: PaneId, text: String },
    /// `fleet broadcast "<text>"` — fan out to every pane except the sender. All
    /// legs share one `group` id. → [`OpResult::Delivered`] carrying that group id.
    Broadcast { text: String },
    /// `fleet reply "<text>"` — route to whoever last messaged this pane. Errors
    /// if nobody has. → [`OpResult::Delivered`].
    Reply { text: String },
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
    /// The outcome of a send/broadcast/reply. `accepted` is true when the target
    /// pane was live and the bytes were queued to its pty — never a claim that
    /// the agent there read them (L3). The `fleet` CLI exits non-zero and prints
    /// `detail` to stderr whenever this is false, which is how a model finds out
    /// its own message went nowhere.
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
    /// The op could not be served (unknown pane, nothing to reply to, no app).
    Error { message: String },
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
    fn every_frame_survives_the_flattened_round_trip() {
        let requests = [
            Op::Send { to: PaneId::Worker(2), text: "take the parser".into() },
            Op::Broadcast { text: "rebasing".into() },
            Op::Reply { text: "on it".into() },
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

    /// The op tag is what a `fleet` verb becomes on the wire, and after Phase 5
    /// they are the same word. Pinned so a rename is a visible protocol change.
    #[test]
    fn the_wire_tag_is_the_cli_verb() {
        let tag = |op: Op| {
            serde_json::to_value(Request::new(op)).unwrap()["op"].as_str().unwrap().to_string()
        };
        assert_eq!(tag(Op::Send { to: PaneId::Orch, text: "x".into() }), "send");
        assert_eq!(tag(Op::Broadcast { text: "x".into() }), "broadcast");
        assert_eq!(tag(Op::Reply { text: "x".into() }), "reply");
        assert_eq!(tag(Op::Roster), "roster");
    }

    #[test]
    fn a_hello_names_its_pane_and_round_trips() {
        for pane in [PaneId::Orch, PaneId::Worker(3)] {
            let hello = Hello::for_pane(pane);
            assert_eq!(hello.pane, pane);
            let line = serde_json::to_string(&hello).unwrap();
            assert_eq!(serde_json::from_str::<Hello>(&line).unwrap(), hello);
        }
    }
}
