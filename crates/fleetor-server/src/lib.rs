//! `fleetor-server` — supervision (BUILDING §3). Phase 1 delivers the ticket
//! lifecycle loop: spawn → assign → turn-end detection → report ingestion,
//! persisted through the [`Store`] seam and observable via the event log.
//!
//! Phase 2 adds the [`hub`] — the routing core behind the unix socket: mail
//! delivery, the `ask_lead`/`reply` blocking round-trip, and the lead's
//! `await_events` long-poll. The gate runner (Phase 3) joins later.
//!
//! [`Store`]: fleetor_core::Store

pub mod gate;
pub mod hub;
pub mod quality;
pub mod supervisor;

pub use gate::ShellGateRunner;
pub use hub::{Hub, HubConfig};
pub use quality::{run_quality_loop, QualityOptions, QualityOutcome, Reviewer, DEFAULT_RETRY_CAP};
pub use supervisor::{run_ticket, Outcome, SuperviseOptions};
