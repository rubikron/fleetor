//! The event stream the fleet server emits to the CLI and (Phase 4) the UI
//! (BUILDING §4.2). Append-only: every state change is one `FleetEvent`
//! persisted to the `events` log. Phase 1 emits worker-state, ticket-state,
//! tool-activity, report, and notice events; mail and gate events are defined
//! now but not emitted until Phases 2–3.

use crate::pane::{PaneId, PaneState};
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

/// Result of a peer review (Phase 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewOutcome {
    Approved,
    ChangesRequested,
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
    /// Peer-review verdict (Phase 3). `reviewer_slot` is the fresh agent that
    /// reviewed, distinct from the slot that did the work.
    ReviewResult { ticket: String, reviewer_slot: u8, outcome: ReviewOutcome },
    /// Free-form operational note (timeouts, crashes, reprompts).
    Notice { level: NoticeLevel, text: String },
    /// A worker's assistant text, for the transcript view (observability only).
    WorkerSaid { slot: u8, ticket: String, text: String },
    /// A worker process ended. `ok` is false on crash/timeout/spawn-failure, with
    /// `detail` carrying the captured stderr tail — the reason a headless worker
    /// died, which was invisible before (stderr used to be dropped).
    WorkerExited { slot: u8, ticket: String, ok: bool, detail: String },

    // ---- the TUI fleet (D-030) ----
    /// One pane→pane message, **body included** — the append-only message log the
    /// feed replays, and the reason there is no mail queue any more.
    ///
    /// `accepted` means the target pane was live and the bytes were queued to its
    /// pty. It is deliberately not called `delivered`: nothing here knows whether
    /// the model at the other end read them, and a UI that claims otherwise is the
    /// worst failure mode this product has (L3). `group` is set on every leg of a
    /// broadcast fan-out so the feed can collapse them back into one row.
    Message {
        id: String,
        from: PaneId,
        to: PaneId,
        body: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        group: Option<String>,
        accepted: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    /// A pane moved through its lifecycle (spawning → live → dead).
    PaneState { pane: PaneId, from: PaneState, to: PaneState },
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
            FleetEvent::ReviewResult { .. } => "review-result",
            FleetEvent::Notice { .. } => "notice",
            FleetEvent::WorkerSaid { .. } => "worker-said",
            FleetEvent::WorkerExited { .. } => "worker-exited",
            FleetEvent::Message { .. } => "message",
            FleetEvent::PaneState { .. } => "pane-state",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `kind()` is the DB's `kind` column, so it must equal the serialized `type`
    /// tag for every variant — including the two new ones.
    #[test]
    fn kind_matches_the_serialized_type_tag() {
        let events = [
            FleetEvent::Message {
                id: "msg-1".into(),
                from: PaneId::Orch,
                to: PaneId::Worker(2),
                body: "take the parser".into(),
                group: None,
                accepted: true,
                detail: None,
            },
            FleetEvent::PaneState {
                pane: PaneId::Worker(2),
                from: PaneState::Spawning,
                to: PaneState::Live,
            },
        ];
        for event in events {
            let json: serde_json::Value = serde_json::to_value(&event).unwrap();
            assert_eq!(json["type"].as_str().unwrap(), event.kind());
        }
    }

    /// Panes cross the wire as bare strings, and the body survives the round-trip.
    #[test]
    fn a_message_event_round_trips_with_bare_pane_strings() {
        let event = FleetEvent::Message {
            id: "msg-1".into(),
            from: PaneId::Worker(1),
            to: PaneId::Worker(3),
            body: "I own src/api".into(),
            group: Some("grp-7".into()),
            accepted: false,
            detail: Some("pane is not live".into()),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""from":"worker-1""#), "{json}");
        assert!(json.contains(r#""to":"worker-3""#), "{json}");
        assert_eq!(serde_json::from_str::<FleetEvent>(&json).unwrap(), event);
    }
}
