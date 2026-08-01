//! `fleetor-core` — the frozen contracts every other crate speaks (BUILDING §4).
//!
//! Pure domain types, no I/O beyond serde. Defined and versioned *before* any
//! supervision logic so the wire surface (report schema, message envelope, event
//! stream, DB shape) can't drift as internals churn. Phase 1 exercises the
//! ticket / report / event / store types; the envelope and gate fields are
//! defined now but not driven until Phases 2–3.

pub mod envelope;
pub mod event;
mod fenced;
pub mod gate;
pub mod ids;
pub mod ownership;
pub mod report;
pub mod review;
pub mod store;
pub mod ticket;
pub mod time;
pub mod wire;

pub use envelope::{Envelope, MessageKind, Party, Ref, ENVELOPE_VERSION};
pub use event::{FleetEvent, GateOutcome, ReviewOutcome, TicketState, WorkerState};
pub use gate::{CheckResult, GateCheck, GateReport, GateRunner, GateSpec};
pub use ownership::{BacklogItem, LeaseGrant, Owner};
pub use report::{GateResults, Report, ReportStatus};
pub use review::{ReviewDecision, ReviewVerdict};
pub use store::Store;
pub use ticket::{Budget, Ticket};
pub use wire::{
    Hello, LeadEvent, LeadEventKind, Op, OpResult, Request, Response, WIRE_VERSION,
};
