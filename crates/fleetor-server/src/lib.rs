//! `fleetor-server` — supervision (BUILDING §3). Phase 1 delivers the ticket
//! lifecycle loop: spawn → assign → turn-end detection → report ingestion,
//! persisted through the [`Store`] seam and observable via the event log.
//!
//! Routing, the unix socket, and the gate runner (Phases 2–3) will join this
//! crate; they are deliberately absent now (YAGNI — BUILDING §8 over-abstraction
//! row).
//!
//! [`Store`]: fleetor_core::Store

pub mod supervisor;

pub use supervisor::{run_ticket, Outcome, SuperviseOptions};
