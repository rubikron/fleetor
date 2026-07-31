//! Classify each tool call's outcome from a parsed transcript.
//!
//! The Phase 0 question is *tool-call fidelity*: does Flash drive Claude Code's
//! tool surface correctly? The trap (seen in the spike-03 capture) is that a
//! `tool_result` with `is_error:true` is often NOT the model's fault — a failing
//! test run via Bash exits non-zero by design. So we bucket every call into:
//!
//!  - [`Outcome::Ok`]            — result returned, not an error.
//!  - [`Outcome::TaskError`]     — legit non-zero (e.g. a failing test); task
//!                                 surface, not a fidelity problem.
//!  - [`Outcome::Fidelity`]      — the model's fault: malformed input, a bad
//!                                 Edit `old_string`, an invented tool/path.
//!  - [`Outcome::NoResult`]      — tool_use with no matching result (turn cut).
//!
//! Fidelity rate = Fidelity / (total calls) is the headline metric.

use fleetor_cc::{ContentBlock, Event, ToolUse};
use std::collections::HashMap;

/// Claude Code's built-in tool names (2.1.x). A `tool_use` outside this set is
/// a hallucinated tool — a clear fidelity failure.
const KNOWN_TOOLS: &[&str] = &[
    "Read", "Write", "Edit", "MultiEdit", "Bash", "BashOutput", "KillBash",
    "Grep", "Glob", "LS", "TodoWrite", "Task", "WebFetch", "WebSearch",
    "NotebookEdit", "ExitPlanMode",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    TaskError,
    Fidelity(FidelityKind),
    NoResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FidelityKind {
    /// CC rejected the tool input against its schema.
    InputValidation,
    /// Edit/MultiEdit `old_string` didn't match (or wasn't unique).
    EditMismatch,
    /// A tool name Claude Code doesn't have.
    UnknownTool,
    /// Read/Edit/Glob against a path the model invented.
    FileNotFound,
    /// An `is_error` we couldn't attribute to the task surface.
    OtherError,
}

impl FidelityKind {
    pub fn label(self) -> &'static str {
        match self {
            FidelityKind::InputValidation => "input-validation",
            FidelityKind::EditMismatch => "edit-mismatch",
            FidelityKind::UnknownTool => "unknown-tool",
            FidelityKind::FileNotFound => "file-not-found",
            FidelityKind::OtherError => "other-error",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CallRecord {
    pub tool: String,
    pub outcome: Outcome,
    /// First line of the error text, when the outcome carried one (for the report).
    pub error_excerpt: Option<String>,
}

/// Walk the transcript, pair each `tool_use` to its `tool_result` by id, and
/// classify. Order is preserved.
pub fn classify(events: &[Event]) -> Vec<CallRecord> {
    // Collect tool_use blocks in order, and index tool_results by id.
    let mut uses: Vec<&ToolUse> = Vec::new();
    let mut results: HashMap<String, (bool, String)> = HashMap::new();

    for ev in events {
        match ev {
            Event::Assistant(m) => {
                for b in &m.message.content {
                    if let ContentBlock::ToolUse(t) = b {
                        uses.push(t);
                    }
                }
            }
            Event::User(m) => {
                for b in &m.message.content {
                    if let ContentBlock::ToolResult(r) = b {
                        if let Some(id) = &r.tool_use_id {
                            results.insert(id.clone(), (r.is_error, r.text()));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    uses.into_iter()
        .map(|u| {
            let (outcome, excerpt) = match results.get(&u.id) {
                None => (Outcome::NoResult, None),
                Some((false, _)) => (Outcome::Ok, None),
                Some((true, text)) => {
                    let kind = classify_error(&u.name, text);
                    let excerpt = first_line(text);
                    match kind {
                        Some(k) => (Outcome::Fidelity(k), Some(excerpt)),
                        None => (Outcome::TaskError, Some(excerpt)),
                    }
                }
            };
            CallRecord {
                tool: u.name.clone(),
                outcome,
                error_excerpt: excerpt,
            }
        })
        .collect()
}

/// Given an errored tool_result, decide whether it's a fidelity failure (return
/// the kind) or a task-level error (return `None`). Heuristics are keyed off the
/// text Claude Code puts in the result — re-derive from fixtures if CC changes.
fn classify_error(tool: &str, text: &str) -> Option<FidelityKind> {
    let t = text.to_lowercase();

    if !KNOWN_TOOLS.contains(&tool) {
        return Some(FidelityKind::UnknownTool);
    }
    if t.contains("inputvalidationerror") || t.contains("invalid input")
        || t.contains("required property") || t.contains("did not match the required")
    {
        return Some(FidelityKind::InputValidation);
    }
    // Bash: a non-zero exit ("Exit code N") is the task surface, not fidelity.
    if tool == "Bash" {
        if t.contains("exit code") {
            return None; // task-level
        }
        // Bash blocked by permissions / malformed → fidelity-ish.
        if t.contains("permission") || t.contains("not allowed") {
            return Some(FidelityKind::OtherError);
        }
        return None;
    }
    if matches!(tool, "Edit" | "MultiEdit" | "Write") {
        if t.contains("string to replace not found")
            || t.contains("not unique")
            || t.contains("old_string")
            || t.contains("no changes to make")
        {
            return Some(FidelityKind::EditMismatch);
        }
    }
    if matches!(tool, "Read" | "Edit" | "MultiEdit" | "Glob" | "LS") {
        if t.contains("no such file") || t.contains("does not exist")
            || t.contains("enoent") || t.contains("file not found")
        {
            return Some(FidelityKind::FileNotFound);
        }
    }
    // An error we can't attribute to the task → treat as fidelity so it can't
    // hide. Over-counting here is the safe direction for a go/no-go.
    Some(FidelityKind::OtherError)
}

fn first_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    let truncated: String = line.chars().take(160).collect();
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use fleetor_cc::parse_transcript;

    #[test]
    fn spike03_has_no_fidelity_failures() {
        let text =
            include_str!("../../../../tests/fixtures/ndjson/spike-03-tooluse.ndjson");
        let records = classify(&parse_transcript(text));
        // 4 calls: Bash(fail test)=TaskError, Read=Ok, Edit=Ok, Bash(pass)=Ok.
        assert_eq!(records.len(), 4);
        let fidelity = records
            .iter()
            .filter(|r| matches!(r.outcome, Outcome::Fidelity(_)))
            .count();
        assert_eq!(fidelity, 0, "clean run must show zero fidelity failures");
        let task_errors = records
            .iter()
            .filter(|r| r.outcome == Outcome::TaskError)
            .count();
        assert_eq!(task_errors, 1, "the intended failing test is one task error");
    }

    #[test]
    fn edit_mismatch_is_fidelity() {
        assert_eq!(
            classify_error("Edit", "String to replace not found in file."),
            Some(FidelityKind::EditMismatch)
        );
    }

    #[test]
    fn bash_nonzero_is_task_not_fidelity() {
        assert_eq!(classify_error("Bash", "Exit code 1\nFAILED tests/..."), None);
    }

    #[test]
    fn hallucinated_tool_is_fidelity() {
        assert_eq!(
            classify_error("SearchReplace", "whatever"),
            Some(FidelityKind::UnknownTool)
        );
    }
}
