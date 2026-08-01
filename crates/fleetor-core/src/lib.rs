//! `fleetor-core` — the frozen contracts every other crate speaks (BUILDING §4).
//!
//! Pure domain types, no I/O beyond serde. Defined and versioned *before* any
//! supervision logic so the wire surface (report schema, message envelope, event
//! stream, DB shape) can't drift as internals churn. Phase 1 exercises the
//! ticket / report / event / store types; the envelope and gate fields are
//! defined now but not driven until Phases 2–3.

pub mod envelope;
pub mod event;
pub mod ids;
pub mod report;
pub mod store;
pub mod ticket;
pub mod time;

pub use envelope::{Envelope, MessageKind, Party, Ref, ENVELOPE_VERSION};
pub use event::{FleetEvent, GateOutcome, TicketState, WorkerState};
pub use report::{GateResults, Report, ReportStatus};
pub use store::Store;
pub use ticket::{Budget, Ticket};
