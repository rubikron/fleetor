//! The append-only event log's payloads (BUILDING §4).
//!
//! Three variants. Phase 5 deleted the other ten along with the headless fleet
//! that emitted them — worker states, ticket moves, tool activity, reports, gate
//! results, review verdicts, mail routing. None of them describe a fleet of live
//! terminals, and keeping them "in case" would have left the log able to
//! describe a supervisor that no longer exists, which is how a ticket system
//! grows back.
//!
//! What is left is what the TUI fleet actually does: it messages, it moves panes
//! through a lifecycle, and it tells the operator when something is wrong.

use crate::pane::{PaneId, PaneState};
use serde::{Deserialize, Serialize};

/// One entry in the append-only event log. `#[serde(tag = "type")]` gives each
/// variant a stable discriminator that also becomes the `kind` column in the DB.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum FleetEvent {
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
    /// Free-form operational note. The honest-failure channel: a target that
    /// could not be read, a `fleet` binary that is not there, a socket that would
    /// not bind. Everything that would otherwise leave the shell looking fine
    /// while something the operator cares about is broken.
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
            FleetEvent::Message { .. } => "message",
            FleetEvent::PaneState { .. } => "pane-state",
            FleetEvent::Notice { .. } => "notice",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `kind()` is the DB's `kind` column, so it must equal the serialized `type`
    /// tag for every variant.
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
            FleetEvent::Notice { level: NoticeLevel::Warn, text: "no fleet binary".into() },
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
