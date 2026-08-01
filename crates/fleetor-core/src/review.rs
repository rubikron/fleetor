//! The peer-review verdict (handoff §8) — a different agent, fresh session, given
//! the diff + AC, says approve or request-changes. "Fresh eyes catch what
//! self-review structurally cannot," and at Flash prices it is nearly free.
//!
//! Like the report (D-008), the verdict rides in the reviewer's final message as
//! a single fenced `fleet-review` JSON block; the supervisor scrapes it. When
//! Phase 4 unifies the runner with the hub, this can move onto an MCP tool the
//! same way `report()` does — the schema is the stable part.

use serde::{Deserialize, Serialize};

/// The info string on the fenced block carrying a review verdict.
pub const REVIEW_FENCE: &str = "fleet-review";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewDecision {
    /// Ship it — the change meets the acceptance criteria.
    Approve,
    /// Bounce it — the listed `blocking` items must be fixed first.
    RequestChanges,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewVerdict {
    pub decision: ReviewDecision,
    pub summary: String,
    /// The required changes when `decision` is `request-changes`; empty on
    /// approve. Carried into the bounce so the worker knows exactly what to fix.
    #[serde(default)]
    pub blocking: Vec<String>,
}

impl ReviewVerdict {
    /// True when the reviewer approved.
    pub fn approved(&self) -> bool {
        self.decision == ReviewDecision::Approve
    }

    /// Extract and parse the last `fleet-review` block from reviewer text.
    /// `None` when no such block exists (→ treat like a missing verdict).
    pub fn from_transcript_text(text: &str) -> Option<anyhow::Result<ReviewVerdict>> {
        let json = crate::fenced::extract_last_fenced(text, REVIEW_FENCE)?;
        Some(serde_json::from_str(&json).map_err(Into::into))
    }

    /// The bounce handed to the original worker when the reviewer requests
    /// changes — framed as required fixes, not a new directive.
    pub fn bounce_message(&self, ticket: &str) -> String {
        let mut s = format!(
            "Peer review of {ticket} requested changes before this can merge. \
             Address each item below, then end your turn with an updated \
             `fleet-report` block.\n\nReviewer summary: {}\n",
            self.summary,
        );
        for (i, item) in self.blocking.iter().enumerate() {
            s.push_str(&format!("{}. {item}\n", i + 1));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_an_approval() {
        let text = "Looks good.\n\n```fleet-review\n\
                    {\"decision\":\"approve\",\"summary\":\"meets the AC\"}\n```";
        let v = ReviewVerdict::from_transcript_text(text).unwrap().unwrap();
        assert!(v.approved());
        assert!(v.blocking.is_empty());
    }

    #[test]
    fn parses_request_changes_with_blocking_items() {
        let text = "```fleet-review\n\
            {\"decision\":\"request-changes\",\"summary\":\"missing a test\",\
             \"blocking\":[\"add a test for the empty case\"]}\n```";
        let v = ReviewVerdict::from_transcript_text(text).unwrap().unwrap();
        assert!(!v.approved());
        assert_eq!(v.blocking.len(), 1);
        assert!(v.bounce_message("T-3").contains("empty case"));
    }

    #[test]
    fn missing_block_is_none() {
        assert!(ReviewVerdict::from_transcript_text("no verdict").is_none());
    }
}
