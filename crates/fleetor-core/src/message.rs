//! What one pane says to another, and how it looks when it lands (D-030).
//!
//! Two jobs, deliberately in one small file because they must not drift:
//!
//!  1. [`Message`] — the record. It carries the **body**, unlike the Phase-2
//!     `FleetEvent::Mail` it replaces, because a product about watching messages
//!     cannot keep the messages out of its own log.
//!  2. [`frame_for_pane`] / [`frame_broadcast_for_pane`] — the exact bytes typed
//!     into the receiving TUI. Single-sourced here, the way [`crate::mail`]
//!     single-sources injection framing today, so the one delivery path in
//!     `src-tauri::deliver` can never invent a second spelling.
//!
//! The direct and broadcast framings differ on purpose: L5 (broadcast
//! amplification) is mitigated partly by a brief clause telling workers never to
//! answer a broadcast unless it names them, and a worker can only obey that if it
//! can *see* which kind arrived.

use crate::event::FleetEvent;
use crate::pane::PaneId;
use serde::{Deserialize, Serialize};

/// One pane→pane message. `group` is set on every leg of a broadcast fan-out so
/// the feed can collapse N rows back into the one gesture that produced them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub from: PaneId,
    pub to: PaneId,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    pub ts: i64,
}

impl Message {
    /// A one-to-one message (`fleet send`, `fleet reply`).
    pub fn direct(from: PaneId, to: PaneId, body: impl Into<String>) -> Self {
        Self::build(from, to, body, None)
    }

    /// One leg of a fan-out (`fleet broadcast`); every leg shares `group`.
    pub fn in_group(from: PaneId, to: PaneId, body: impl Into<String>, group: impl Into<String>) -> Self {
        Self::build(from, to, body, Some(group.into()))
    }

    fn build(from: PaneId, to: PaneId, body: impl Into<String>, group: Option<String>) -> Self {
        Self {
            id: crate::ids::new_id("msg"),
            from,
            to,
            body: body.into(),
            group,
            ts: crate::time::now_ms(),
        }
    }

    pub fn is_broadcast(&self) -> bool {
        self.group.is_some()
    }

    /// The bytes to type into the receiving pane.
    pub fn framed(&self) -> String {
        if self.is_broadcast() {
            frame_broadcast_for_pane(self.from, &self.body)
        } else {
            frame_for_pane(self.from, &self.body)
        }
    }

    /// Turn the record into its log entry. `accepted` means *the target pane was
    /// live and the bytes were queued to its pty* — never that the model read
    /// them. Nothing downstream may render it as "delivered" (L3).
    pub fn into_event(self, accepted: bool, detail: Option<String>) -> FleetEvent {
        FleetEvent::Message {
            id: self.id,
            from: self.from,
            to: self.to,
            body: self.body,
            group: self.group,
            accepted,
            detail,
        }
    }
}

/// Frame a direct message for injection into a live pane: `[fleet · worker-1] …`.
pub fn frame_for_pane(from: PaneId, body: &str) -> String {
    frame(&from.to_string(), body)
}

/// Frame one leg of a broadcast: `[fleet · worker-1 → all] …`. Visibly distinct
/// from a direct message so the receiver can apply the do-not-answer-a-broadcast
/// rule from its brief (L5).
pub fn frame_broadcast_for_pane(from: PaneId, body: &str) -> String {
    frame(&format!("{from} → all"), body)
}

fn frame(label: &str, body: &str) -> String {
    format!("[fleet · {label}] {body}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_framing_names_the_sender() {
        assert_eq!(frame_for_pane(PaneId::Worker(1), "rebasing now"), "[fleet · worker-1] rebasing now");
        assert_eq!(frame_for_pane(PaneId::Orch, "take T-4"), "[fleet · orch] take T-4");
    }

    #[test]
    fn broadcast_framing_is_visibly_different_from_a_direct_message() {
        let direct = frame_for_pane(PaneId::Worker(1), "status?");
        let fanned = frame_broadcast_for_pane(PaneId::Worker(1), "status?");
        assert_ne!(direct, fanned, "a worker must be able to tell a broadcast apart (L5)");
        assert_eq!(fanned, "[fleet · worker-1 → all] status?");
    }

    #[test]
    fn a_message_frames_itself_according_to_its_group() {
        let direct = Message::direct(PaneId::Orch, PaneId::Worker(2), "hi");
        assert_eq!(direct.framed(), "[fleet · orch] hi");
        let fanned = Message::in_group(PaneId::Orch, PaneId::Worker(2), "hi", "grp-1");
        assert_eq!(fanned.framed(), "[fleet · orch → all] hi");
    }

    #[test]
    fn multi_line_bodies_are_framed_intact() {
        // Phase 0 proved a multi-line bracketed paste arrives as one message, so
        // framing must not flatten or truncate one.
        let framed = frame_for_pane(PaneId::Worker(3), "three things:\nfirst\nsecond");
        assert!(framed.starts_with("[fleet · worker-3] three things:"));
        assert!(framed.ends_with("\nfirst\nsecond"));
    }

    #[test]
    fn the_event_carries_the_body_and_the_honest_outcome() {
        let msg = Message::direct(PaneId::Worker(1), PaneId::Worker(2), "own src/api");
        let id = msg.id.clone();
        let event = msg.into_event(false, Some("pane is dead".into()));
        assert_eq!(event.kind(), "message");
        let FleetEvent::Message { id: got, body, accepted, detail, group, .. } = event else {
            panic!("expected a message event");
        };
        assert_eq!(got, id);
        assert_eq!(body, "own src/api", "the body lives in the event, not just the pty");
        assert!(!accepted);
        assert_eq!(detail.as_deref(), Some("pane is dead"));
        assert_eq!(group, None);
    }

    #[test]
    fn ids_are_unique_per_message() {
        let a = Message::direct(PaneId::Orch, PaneId::Worker(1), "x");
        let b = Message::direct(PaneId::Orch, PaneId::Worker(1), "x");
        assert_ne!(a.id, b.id);
    }
}
