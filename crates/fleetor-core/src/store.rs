//! The `Store` seam (BUILDING §3). SQLite lives behind it (`fleetor-db`); the
//! supervisor depends only on this trait so it can be driven against an
//! in-memory fake in unit tests. One of the four sanctioned seams — no more.

use crate::envelope::{Envelope, Party};
use crate::event::{FleetEvent, TicketState};
use crate::report::Report;
use crate::ticket::Ticket;
use anyhow::Result;

pub trait Store: Send + Sync {
    /// Insert or replace a ticket row.
    fn upsert_ticket(&self, ticket: &Ticket) -> Result<()>;

    /// Move a ticket to a new board state.
    fn set_ticket_state(&self, id: &str, state: TicketState) -> Result<()>;

    /// Persist an ingested report against its ticket.
    fn save_report(&self, ticket: &str, slot: u8, report: &Report) -> Result<()>;

    /// Append one event to the log; returns its monotonic sequence number.
    fn append_event(&self, event: &FleetEvent) -> Result<i64>;

    /// All tickets, for board reconstruction.
    fn tickets(&self) -> Result<Vec<Ticket>>;

    /// Events with sequence greater than `after` (0 = from the start), oldest
    /// first — the CLI/UI tail.
    fn events_since(&self, after: i64) -> Result<Vec<(i64, FleetEvent)>>;

    /// Persist a mail envelope as undelivered (Phase 2). The `mail` table is the
    /// source of truth so mail survives a crash (handoff §11).
    fn save_mail(&self, env: &Envelope) -> Result<()>;

    /// Return, and mark delivered, all undelivered mail addressed to `to`,
    /// oldest first — the drain path (Stop hook / turn boundary).
    fn take_mail(&self, to: &Party) -> Result<Vec<Envelope>>;
}
