//! The event stream the fleet server emits to the CLI and (Phase 4) the UI
//! (BUILDING §4.2). Append-only: every state change is one `FleetEvent`
//! persisted to the `events` log. Phase 1 emits worker-state, ticket-state,
//! tool-activity, report, and notice events; mail and gate events are defined
//! now but not emitted until Phases 2–3.

use serde::{Deserialize, Serialize};

/// A worker slot's lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkerState {
    /// Spawned, awaiting the `init` event.
    Booting,
    /// Alive, no ticket in flight.
    Idle,
    /// A turn is running.
    Working,
    /// Parked on an `ask_lead` (Phase 2).
    Blocked,
    /// Process gone.
    Dead,
}

/// A ticket's position on the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TicketState {
    #[default]
    Backlog,
    Assigned,
    InProgress,
    InReview,
    Done,
    Blocked,
    Failed,
}

/// Result of an exit-gate run (Phase 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GateOutcome {
    Pass,
    Fail,
}

/// One entry in the append-only event log. `#[serde(tag = "type")]` gives each
/// variant a stable discriminator that also becomes the `kind` column in the DB.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum FleetEvent {
    WorkerState { slot: u8, from: WorkerState, to: WorkerState },
    TicketState { ticket: String, from: TicketState, to: TicketState },
    /// A tool call observed in a worker's stream — drives the dashboard
    /// "activity" line (handoff §10).
    ToolActivity { slot: u8, ticket: String, tool: String },
    /// A structured report was ingested.
    ReportFiled { ticket: String, slot: u8, status: super::report::ReportStatus },
    /// Mail routed between parties (Phase 2).
    Mail { id: String, from: String, to: String, kind: String },
    /// Exit-gate outcome (Phase 3).
    GateResult { ticket: String, slot: u8, outcome: GateOutcome },
    /// Free-form operational note (timeouts, crashes, reprompts).
    Notice { level: NoticeLevel, text: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NoticeLevel {
    Info,
    Warn,
    Error,
}

impl FleetEvent {
    /// The stable discriminator (the `type` tag), for the DB `kind` column and
    /// log lines. Derived from the serialized form so it can never drift.
    pub fn kind(&self) -> &'static str {
        match self {
            FleetEvent::WorkerState { .. } => "worker-state",
            FleetEvent::TicketState { .. } => "ticket-state",
            FleetEvent::ToolActivity { .. } => "tool-activity",
            FleetEvent::ReportFiled { .. } => "report-filed",
            FleetEvent::Mail { .. } => "mail",
            FleetEvent::GateResult { .. } => "gate-result",
            FleetEvent::Notice { .. } => "notice",
        }
    }
}
