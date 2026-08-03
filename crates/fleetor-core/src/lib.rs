//! `fleetor-core` — the frozen contracts every other crate speaks (BUILDING §4).
//!
//! Pure domain types, no I/O beyond serde. Defined and versioned *before* any
//! supervision logic so the wire surface (report schema, message envelope, event
//! stream, DB shape) can't drift as internals churn. Phase 1 exercises the
//! ticket / report / event / store types; the envelope and gate fields are
//! defined now but not driven until Phases 2–3.
//!
//! D-030 adds the TUI-fleet contract alongside the above: [`pane`] (identity),
//! [`message`] (what one pane says to another, and how it is framed on arrival)
//! and [`brief`] (what each pane is told about the fleet at spawn). The old
//! headless surface — envelope/mail/ticket/report/review/gate/ownership — is
//! deleted wholesale in Phase 5; until then both live here and neither knows
//! about the other.

pub mod brief;
pub mod envelope;
pub mod event;
mod fenced;
pub mod gate;
pub mod ids;
pub mod mail;
pub mod message;
pub mod ownership;
pub mod pane;
pub mod report;
pub mod review;
pub mod store;
pub mod ticket;
pub mod time;
pub mod wire;

pub use brief::{orch_brief, worker_brief, VERBS};
pub use envelope::{Envelope, MessageKind, Party, Ref, ENVELOPE_VERSION};
pub use event::{FleetEvent, GateOutcome, ReviewOutcome, TicketState, WorkerState};
pub use gate::{CheckResult, GateCheck, GateReport, GateRunner, GateSpec};
pub use mail::{frame_mail_for_injection, sender_label};
pub use message::{frame_broadcast_for_pane, frame_for_pane, Message};
pub use ownership::{BacklogItem, LeaseGrant, Owner};
pub use pane::{PaneEntry, PaneId, PaneState, ParsePaneIdError, WORKER_SLOTS};
pub use report::{GateResults, Report, ReportStatus};
pub use review::{ReviewDecision, ReviewVerdict};
pub use store::Store;
pub use ticket::{Budget, Ticket};
pub use wire::{
    Hello, LeadEvent, LeadEventKind, Op, OpResult, Request, Response, WIRE_VERSION,
};
