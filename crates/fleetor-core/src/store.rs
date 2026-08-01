//! The `Store` seam (BUILDING §3). SQLite lives behind it (`fleetor-db`); the
//! supervisor depends only on this trait so it can be driven against an
//! in-memory fake in unit tests. One of the four sanctioned seams — no more.

use crate::envelope::{Envelope, Party};
use crate::event::{FleetEvent, TicketState};
use crate::ownership::{BacklogItem, LeaseGrant, Owner};
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

    /// The highest event sequence so far (0 when the log is empty). A cheap
    /// cursor for "watch for events appended after this point" — the supervisor
    /// captures it at assign to detect the hub's report-over-MCP `ReportFiled`
    /// as its primary done-signal (D-018/4b) without re-reading the whole log.
    fn latest_seq(&self) -> Result<i64>;

    /// Persist a mail envelope as undelivered (Phase 2). The `mail` table is the
    /// source of truth so mail survives a crash (handoff §11).
    fn save_mail(&self, env: &Envelope) -> Result<()>;

    /// Return, and mark delivered, all undelivered mail addressed to `to`,
    /// oldest first — the drain path (Stop hook / turn boundary).
    fn take_mail(&self, to: &Party) -> Result<Vec<Envelope>>;

    /// Slots currently holding `path` (Phase 3 `whos_working_on`). Empty if free.
    fn who_owns(&self, path: &str) -> Result<Vec<Owner>>;

    /// Request a lease on `path` for (`slot`, `ticket`). Granted if free or
    /// already this slot's; denied if a *different* slot holds it. Idempotent for
    /// the same holder (Phase 3 `claim_file`).
    fn claim_lease(&self, path: &str, slot: u8, ticket: &str) -> Result<LeaseGrant>;

    /// Persist a parked out-of-scope discovery (Phase 3 `backlog_add`).
    fn add_backlog(&self, item: &BacklogItem) -> Result<()>;

    /// All backlog items, oldest first — the board's backlog column.
    fn list_backlog(&self) -> Result<Vec<BacklogItem>>;
}
