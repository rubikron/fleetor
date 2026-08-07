//! `fleet handoff` — the orchestrator saying the whole goal is met (WP-13).
//!
//! **A different altitude from `fleet done`, and the two must never blur.** A
//! receipt (`done.rs`, WP-06) is a worker closing *one* block with the output of
//! one check. This is `orch` telling the operator that the mission the vision
//! named is finished: what the fleet built, how anyone could check it, and what
//! it knows is still open.
//!
//! **It appends one event and stops.** Nothing reaches a terminal, nothing is
//! scheduled, nothing is permitted or refused by it — the same shape
//! [`task`](crate::task) has, and for the same reason. `task.rs`'s tripwire list
//! is the standard a sixth [`FleetEvent`] variant had to clear, and this one
//! clears it the same way: no code reads a handoff back to decide anything, and
//! the day something does, the thing that grew back is a fleet whose delivery
//! path knows whether the mission is over.
//!
//! The fields are what a report to the operator has to carry to be worth
//! reading: a claim ([`built`](Handoff::built)), the means to check it
//! ([`evidence`](Handoff::evidence)), and the honesty about what it does not
//! cover ([`open`](Handoff::open)). `evidence` is required for the reason a task
//! block's `--crit-t` is: a claim with nothing checkable behind it cannot be
//! argued with.

use crate::event::FleetEvent;
use crate::pane::PaneId;
use serde::{Deserialize, Serialize};

/// One declaration that the goal is met — a claim `orch` made, never a state
/// anything inferred (Tier 1.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Handoff {
    /// What the fleet built, in the operator's own terms.
    pub built: String,
    /// How anyone could check it — a command, a branch, a file to read. Plural
    /// because a mission worth handing over has more than one way in.
    pub evidence: Vec<String>,
    /// What is unfinished or uncertain. Empty is legal and means exactly that
    /// nothing was named, never that nothing is open.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub open: Vec<String>,
}

impl Handoff {
    /// Build a handoff, or say why it is not one. Validation at the system
    /// boundary and nothing else: this file has no opinion about whether the
    /// goal *is* met, only about whether the report can be read.
    pub fn new(built: &str, evidence: &[String], open: &[String]) -> Result<Self, String> {
        let built = plain(built);
        if built.is_empty() {
            return Err(
                "--built cannot be empty — say what the fleet built, in the operator's terms"
                    .to_string(),
            );
        }
        let evidence = kept(evidence);
        if evidence.is_empty() {
            return Err(
                "--evidence is required at least once — a command someone else could run, a \
                 branch, a file to read. A claim with nothing checkable behind it cannot be \
                 argued with, and this one is the whole report"
                    .to_string(),
            );
        }
        Ok(Self { built, evidence, open: kept(open) })
    }

    /// The log entry. `from` and the row's own timestamp are what make it a
    /// claim somebody made at a time rather than a fact the system asserts.
    pub fn into_event(self, id: impl Into<String>, from: PaneId) -> FleetEvent {
        FleetEvent::Handoff {
            id: id.into(),
            from,
            built: self.built,
            evidence: self.evidence,
            open: self.open,
        }
    }
}

fn kept(raw: &[String]) -> Vec<String> {
    raw.iter().map(|item| plain(item)).filter(|item| !item.is_empty()).collect()
}

/// Drop what a terminal would act on rather than print, keeping newlines and
/// tabs. Its own copy rather than `message::sanitize`, for the reason
/// `task::plain` gives for its: that function is the pty delivery boundary, this
/// package does not touch the delivery path, and coupling the two is how one of
/// them drifts.
fn plain(raw: &str) -> String {
    raw.replace("\r\n", "\n")
        .chars()
        .filter_map(|c| match c {
            '\n' | '\t' => Some(c),
            '\r' => Some('\n'),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// The shape of a report worth reading: the claim, the ways to check it, and
    /// what it does not cover.
    #[test]
    fn a_handoff_carries_a_claim_its_evidence_and_what_is_still_open() {
        let h = Handoff::new(
            "the parser accepts nested groups end to end",
            &list(&["cargo test -p parser", "fleet/integration @ a1b2c3d"]),
            &list(&["the error messages are still the tokenizer's"]),
        )
        .expect("a complete handoff");
        assert_eq!(h.built, "the parser accepts nested groups end to end");
        assert_eq!(h.evidence.len(), 2, "evidence is plural");
        assert_eq!(h.open, vec!["the error messages are still the tokenizer's".to_string()]);
    }

    /// Nothing open is legal — and it is a claim, not an absence the code fills
    /// in. A handoff that had to invent a loose end would teach the fleet to
    /// write one.
    #[test]
    fn a_handoff_with_nothing_open_is_still_a_handoff() {
        let h = Handoff::new("the CLI ships", &list(&["cargo test --workspace"]), &[])
            .expect("a complete handoff");
        assert!(h.open.is_empty());
    }

    /// The two refusals, each naming the flag to fix — the model reads them off
    /// its own stderr and self-corrects.
    #[test]
    fn a_handoff_with_no_claim_or_no_evidence_is_refused_by_name() {
        let why = Handoff::new("  ", &list(&["cargo test"]), &[]).expect_err("must be refused");
        assert!(why.contains("--built"), "{why}");

        let why = Handoff::new("we finished", &[], &[]).expect_err("must be refused");
        assert!(why.contains("--evidence"), "{why}");
        assert!(why.contains("cannot be argued with"), "the reason travels with the rule: {why}");

        // Blank evidence is dropped, not counted: `--evidence ""` is the same as
        // not passing it, and must refuse rather than record one empty line.
        assert!(Handoff::new("we finished", &list(&["", "  "]), &[]).is_err());
    }

    /// The Activity feed prints these into a live view and an archived run is
    /// read with `cat`, so an escape here would repaint a reader's screen rather
    /// than be read. Newlines and tabs survive — a handoff is prose.
    #[test]
    fn control_characters_are_dropped_from_a_handoff() {
        let h = Handoff::new(
            "the parser \x1b[2Jlands",
            &list(&["cargo\ttest"]),
            &list(&["one\r\nloose end"]),
        )
        .expect("a handoff");
        assert_eq!(h.built, "the parser [2Jlands");
        assert!(!h.built.contains('\x1b'));
        assert_eq!(h.evidence[0], "cargo\ttest", "a tab is layout, not an escape");
        assert_eq!(h.open[0], "one\nloose end", "CRLF collapses rather than submitting");
    }

    /// The event is a claim: who said it, and what they said.
    #[test]
    fn a_handoff_becomes_an_attributed_event_of_its_own_kind() {
        let event = Handoff::new("it is done", &list(&["cargo test"]), &[])
            .unwrap()
            .into_event("handoff-1", PaneId::Orch);
        assert_eq!(event.kind(), "handoff");
        let FleetEvent::Handoff { id, from, built, .. } = event else { panic!("expected handoff") };
        assert_eq!(id, "handoff-1");
        assert_eq!(from, PaneId::Orch, "the log records who claimed it");
        assert_eq!(built, "it is done");
    }
}
