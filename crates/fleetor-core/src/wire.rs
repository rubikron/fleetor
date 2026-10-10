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
use crate::task::{TaskRecord, TaskStatus};
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

/// The whole fleet tool surface: six ops, one per `fleet` verb that needs the
/// hub. `whoami` needs no round trip — a pane knows its own name from its
/// environment — and `done` composes an ordinary [`Op::Send`] after its local
/// half runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    /// `fleet send <pane> "<text>"` — write into one live pane's terminal.
    /// → [`OpResult::Delivered`], with `accepted: false` if the pane is not live.
    ///
    /// `fleet send operator "…"` is the same op with the one addressee that has
    /// no terminal (WP-07), and it is the only thing that changes the answer:
    /// → [`OpResult::Recorded`], because nothing was typed anywhere.
    Send { to: PaneId, text: String },
    /// `fleet broadcast "<text>"` — fan out to every pane except the sender. All
    /// legs share one `group` id. → [`OpResult::Delivered`] carrying that group id.
    Broadcast { text: String },
    /// `fleet reply "<text>"` — route to whoever last messaged this pane. Errors
    /// if nobody has. → [`OpResult::Delivered`].
    Reply { text: String },
    /// `fleet cmd <pane> "/compact …" --why "…"` — run an allowed slash command
    /// in a pane's terminal, including this pane's own (D-045).
    ///
    /// Deliberately **not** a flavour of [`Op::Send`]. A command is delivered
    /// unframed so its `/` lands in column 0, it may target the sender, and it is
    /// refused at accept time against [`ALLOWED_COMMANDS`](crate::command::ALLOWED_COMMANDS)
    /// — three properties a message must never have. → [`OpResult::Delivered`],
    /// or [`OpResult::Error`] when the command or the `why` does not pass.
    Cmd { to: PaneId, command: String, why: String },
    /// `fleet task post|update|list` — the blackboard (WP-05). One op for the
    /// whole verb family, so the wire tag is still the CLI verb.
    ///
    /// **This op reaches the store and never the app.** It is the only one here
    /// that does not, which is the point: the board is a record the fleet keeps,
    /// not a thing that makes anything happen. → [`OpResult::Recorded`] for a
    /// post or an update, [`OpResult::Board`] for a list.
    Task { action: TaskAction },
    /// `fleet handoff --built "…" --evidence "…"` — `orch` declaring the whole
    /// goal met (WP-13). → [`OpResult::Recorded`], carrying the handoff's id.
    ///
    /// **The second op that reaches the store and never the app**, and it is the
    /// same argument [`Op::Task`] carries: what happened is that a claim was
    /// written down. Nothing is typed anywhere, so `accepted` would be a word
    /// about a pty that was never opened (Tier 1.5), and nothing reads a handoff
    /// back to permit, order or refuse anything the fleet does afterwards.
    ///
    /// Deliberately not a flavour of [`Op::Send`] to `operator`. A receipt-style
    /// message would be `orch` *saying* something to an addressee; this is a
    /// declaration about the mission, at a different altitude from the
    /// block-level `fleet done` and with fields of its own that a body of prose
    /// would flatten.
    Handoff {
        built: String,
        evidence: Vec<String>,
        open: Vec<String>,
        /// The goal this closes, when orch names one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        goal: Option<u64>,
    },
    /// `fleet roster` — every pane and its state. → [`OpResult::Roster`].
    Roster,
}

/// Which of the three things `fleet task` does. Nested under [`Op::Task`] rather
/// than promoted to three ops, because the requirements spend the verb budget
/// **once**: `task` is one word the briefs teach and one clap subcommand tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum TaskAction {
    /// Put a block on the board. The fields are the vision's shape — see
    /// [`TaskBlock`](crate::task::TaskBlock); they arrive flat because a weak
    /// model emits flat flags far more reliably than JSON.
    Post {
        /// Open a goal rather than a task.
        #[serde(default)]
        goal: bool,
        outcome: String,
        #[serde(default)]
        technical: Vec<String>,
        vision: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        owner: Option<PaneId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instructions: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        converges_on: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reviewer: Option<PaneId>,
    },
    /// Append a claim to a block already on the board. Anyone may; the `from` on
    /// the resulting event is the accountability.
    Update {
        task: u64,
        status: TaskStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    /// Add a comment; status and owner stay as they are.
    Comment { task: u64, text: String },
    /// Replace whole fields. An empty list means that field is not edited.
    Edit {
        task: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outcome: Option<String>,
        #[serde(default)]
        technical: Vec<String>,
        #[serde(default)]
        vision: Vec<String>,
        /// Name the reviewer. orch and the operator only, whoever opened it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reviewer: Option<PaneId>,
        /// Put the task under this goal. orch and the operator only.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<u64>,
    },
    /// Say a task is not worth doing. Anyone; it changes nothing else.
    Flag { task: u64, reason: String },
    /// Take a task off the working board. orch (not the operator's) and the operator.
    Remove { task: u64, reason: String },
    /// Undo a removal.
    Restore {
        task: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// A verdict on a task's work. Anyone but its owner.
    Review {
        task: u64,
        met: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// Hand a task on: it returns to planned with no owner.
    Release {
        task: u64,
        why: String,
        done: String,
        left: String,
        /// `--where`, as typed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        place: Option<String>,
        /// The sender's own checkout, read by the CLI. Used only when the
        /// sender is the owner.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        here: Option<String>,
    },
    /// Put a `fleet done` receipt on the task's chain. Sent after the receipt
    /// message, in a call of its own.
    Receipt {
        task: u64,
        check: String,
        status: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        commit: Option<String>,
        #[serde(default)]
        uncommitted: bool,
        accepted: bool,
    },
    /// One goal or task with its whole chain.
    Show { task: u64 },
    /// Every goal and task in the target's store.
    List,
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
    /// **Something entered the log, and no pty was written to.** One variant,
    /// one word, three callers: a task claim (`task post`, `task update`,
    /// WP-05), a message addressed to the operator (WP-07), and `orch`'s
    /// handoff (WP-13).
    ///
    /// Deliberately not [`OpResult::Delivered`]: nothing was delivered to a pane
    /// and `accepted` would be a claim about a pty that was never written to.
    /// What happened is exactly that a record was appended, and the word says so.
    ///
    /// It is equally deliberately **not one variant per caller**. `recorded` is the
    /// requirement's outcome word for "entered the log, no pty exists", and a
    /// second variant meaning the same thing under a different name would be
    /// two spellings of one fact — the drift `PaneId`'s bare-string serde and
    /// the wire-tag-is-the-verb rule exist to prevent. The callers differ in
    /// what the id names, not in what happened, so the field is `record_id` and
    /// its doc says which. (`record_id` rather than `id`, for the reason
    /// `Delivered` uses `msg_id`: [`Response`] flattens this enum next to its
    /// own `id`.)
    Recorded {
        /// The task block's id for a `fleet task`, the message's id for a
        /// message to the operator, the handoff's id for a `fleet handoff`.
        record_id: String,
    },
    /// The board, replayed from the event log (`task list`).
    Board {
        tasks: Vec<TaskRecord>,
        /// The session answering, so a reader can tell an owner from an
        /// earlier run from one beside it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lineage: Option<String>,
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
    use crate::task::{board, ChainEntry, Kind, TaskBlock, TaskRecord};

    fn sample_record() -> TaskRecord {
        let block = TaskBlock::new(
            Kind::Task,
            "the parser accepts nested groups",
            &["cargo test -p parser".to_string()],
            &["one grammar".to_string()],
            Some(PaneId::Worker(2)),
            Some("start from the tokenizer"),
            Some(11),
            None,
        )
        .unwrap();
        let log = [
            ChainEntry::Opened { block }.into_event(14, PaneId::Orch, "run-1", "lin-1"),
            ChainEntry::status(TaskStatus::InProgress, Some("on it"))
                .into_event(14, PaneId::Worker(2), "run-1", "lin-1"),
        ];
        board(&log).remove(0)
    }

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
            Op::Cmd {
                to: PaneId::Worker(2),
                command: "/compact keep the parser".into(),
                why: "finished task block 3".into(),
            },
            Op::Task {
                action: TaskAction::Post {
                    goal: false,
                    outcome: "the parser accepts nested groups".into(),
                    technical: vec!["cargo test -p parser".into()],
                    vision: vec!["one grammar".into()],
                    owner: Some(PaneId::Worker(2)),
                    instructions: Some("start from the tokenizer".into()),
                    parent: Some(11),
                    converges_on: None,
                    reviewer: None,
                },
            },
            Op::Task {
                action: TaskAction::Update {
                    task: 14,
                    status: TaskStatus::Done,
                    note: Some("cargo test passes".into()),
                },
            },
            Op::Task { action: TaskAction::Show { task: 14 } },
            Op::Task { action: TaskAction::Comment { task: 14, text: "found a leak".into() } },
            Op::Task {
                action: TaskAction::Edit {
                    task: 14,
                    outcome: Some("nested groups parse".into()),
                    technical: vec![],
                    vision: vec!["one grammar".into()],
                    reviewer: None,
                    parent: None,
                },
            },
            Op::Task { action: TaskAction::List },
            Op::Handoff {
                built: "the parser accepts nested groups".into(),
                evidence: vec!["cargo test -p parser".into()],
                open: vec!["the error messages are still the tokenizer's".into()],
                goal: None,
            },
            Op::Handoff {
                built: "the CLI ships".into(),
                evidence: vec!["cargo test --workspace".into()],
                open: vec![],
                goal: None,
            },
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
            OpResult::Recorded { record_id: "task-1-0".into() },
            OpResult::Recorded { record_id: "msg-1".into() },
            OpResult::Board { tasks: vec![sample_record()], lineage: Some("lin-1".into()) },
            OpResult::Board { tasks: vec![], lineage: None },
            OpResult::Roster {
                panes: vec![PaneEntry::new(PaneId::Operator, PaneState::Present)],
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
        assert_eq!(
            tag(Op::Cmd { to: PaneId::Orch, command: "/clear".into(), why: "y".into() }),
            "cmd"
        );
        // One op for `post`, `update` and `list` — the verb budget is spent once,
        // so the tag is the verb the briefs teach and the action rides inside it.
        assert_eq!(tag(Op::Task { action: TaskAction::List }), "task");
        assert_eq!(
            tag(Op::Task {
                action: TaskAction::Update { task: 14, status: TaskStatus::Done, note: None }
            }),
            "task"
        );
        assert_eq!(
            tag(Op::Handoff {
                built: "it is done".into(),
                evidence: vec!["cargo test".into()],
                open: vec![],
                goal: None,
            }),
            "handoff"
        );
        assert_eq!(tag(Op::Roster), "roster");
    }

    /// The three callers of `recorded` are one variant, so the tag they all
    /// serialize under is the one word the requirement names — and a second
    /// variant added later under the same word would fail to deserialize
    /// rather than quietly shadow this one.
    #[test]
    fn everything_that_only_reaches_the_log_answers_with_the_same_word() {
        let tag = |result: OpResult| {
            serde_json::to_value(Response::new("req-1", result)).unwrap()["result"]
                .as_str()
                .unwrap()
                .to_string()
        };
        assert_eq!(tag(OpResult::Recorded { record_id: "task-1-0".into() }), "recorded");
        assert_eq!(tag(OpResult::Recorded { record_id: "msg-9".into() }), "recorded");
        assert_eq!(tag(OpResult::Recorded { record_id: "handoff-1".into() }), "recorded");
        assert_eq!(
            tag(OpResult::Delivered { msg_id: "msg-1".into(), accepted: true, detail: None }),
            "delivered",
            "the word a pty write answers with is still its own",
        );
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
