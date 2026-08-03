//! The `Store` seam (BUILDING §3). SQLite lives behind it (`fleetor-db`); the
//! hub depends only on this trait so it can be driven against an in-memory fake
//! in unit tests. One of the four sanctioned seams — no more.
//!
//! **Three methods.** Phase 5 removed the other nine: tickets, reports, mail,
//! leases, backlog. An append-only log of what the fleet said is the entire
//! persistence story of a TUI fleet — panes hold their own state in their own
//! terminals, which is exactly what made the mail queue and the turn boundary
//! unnecessary.

use crate::event::FleetEvent;
use anyhow::Result;

pub trait Store: Send + Sync {
    /// Append one event to the log; returns its monotonic sequence number.
    fn append_event(&self, event: &FleetEvent) -> Result<i64>;

    /// Events with sequence greater than `after` (0 = from the start), oldest
    /// first — the UI's replay and tail.
    fn events_since(&self, after: i64) -> Result<Vec<(i64, FleetEvent)>>;

    /// The highest event sequence so far (0 when the log is empty).
    fn latest_seq(&self) -> Result<i64>;
}
