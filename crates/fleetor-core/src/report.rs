//! The report schema (handoff §4) — the structured completion that keeps the
//! orchestrator's context clean. Raw transcripts stay on disk; only this
//! reaches the lead.
//!
//! In Phase 1 the worker emits the report as a single fenced `fleet-report`
//! JSON block in its final message, and the supervisor ingests it from the
//! transcript (DECISIONS D-008). When Phase 2 lands the MCP shim, `report()`
//! becomes the primary channel and this parser stays as the "no report filed"
//! backstop.

use serde::{Deserialize, Serialize};

/// The info string on the fenced code block carrying a report.
pub const REPORT_FENCE: &str = "fleet-report";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReportStatus {
    Done,
    Blocked,
    NeedsDecision,
    Failed,
}

/// Machine-checkable exit-gate results (handoff §4). Filled by the gate runner
/// in Phase 3; optional here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateResults {
    #[serde(default)]
    pub tests: Option<bool>,
    #[serde(default)]
    pub typecheck: Option<bool>,
    #[serde(default)]
    pub lint: Option<bool>,
    #[serde(default)]
    pub build: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub ticket: String,
    pub status: ReportStatus,
    pub summary: String,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub diffstat: Option<String>,
    #[serde(default)]
    pub gate: Option<GateResults>,
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub questions: Vec<String>,
    #[serde(default)]
    pub risks: Vec<String>,
    #[serde(default)]
    pub followups: Vec<String>,
}

impl Report {
    /// Extract and parse the last `fleet-report` fenced block from assistant
    /// text. Returns `None` if no such block exists (→ the no-report reprompt).
    /// Scans for the *last* block so a worker that shows a draft then a final
    /// report yields the final one.
    pub fn from_transcript_text(text: &str) -> Option<anyhow::Result<Report>> {
        let json = crate::fenced::extract_last_fenced(text, REPORT_FENCE)?;
        Some(serde_json::from_str(&json).map_err(Into::into))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_happy_report_from_final_message() {
        let text = "Done. Here is my report:\n\n```fleet-report\n\
                    {\"ticket\":\"T-1\",\"status\":\"done\",\"summary\":\"added it\"}\n```\n";
        let r = Report::from_transcript_text(text).unwrap().unwrap();
        assert_eq!(r.ticket, "T-1");
        assert_eq!(r.status, ReportStatus::Done);
        assert!(r.questions.is_empty());
    }

    #[test]
    fn missing_block_returns_none() {
        assert!(Report::from_transcript_text("no report here").is_none());
    }

    #[test]
    fn takes_the_last_block_when_several() {
        let text = "```fleet-report\n{\"ticket\":\"T-1\",\"status\":\"blocked\",\"summary\":\"draft\"}\n```\n\
                    later...\n\
                    ```fleet-report\n{\"ticket\":\"T-1\",\"status\":\"done\",\"summary\":\"final\"}\n```";
        let r = Report::from_transcript_text(text).unwrap().unwrap();
        assert_eq!(r.status, ReportStatus::Done);
        assert_eq!(r.summary, "final");
    }

    #[test]
    fn malformed_json_is_an_error_not_a_none() {
        let text = "```fleet-report\n{not json}\n```";
        assert!(Report::from_transcript_text(text).unwrap().is_err());
    }
}
