//! The exit gate (handoff §4, §8) — the machine-checkable checks that must pass
//! before a report is accepted by the lead. `GateRunner` is the 4th and last
//! sanctioned seam (BUILDING §3): shell commands now, anything later. The trait
//! only promises "run in this worktree, tell me what passed"; *which* commands
//! is config for the shell impl ([`GateSpec`]), not part of the seam — a future
//! non-shell runner keeps the same `run(cwd) -> GateReport` face.
//!
//! Gate *discovery* (reading CLAUDE.md / package.json / Makefile to propose the
//! command set — handoff §3) is a Phase 4 concern; here the spec is given.

use crate::report::GateResults;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// One named check and the shell command that decides pass/fail (exit 0 = pass).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateCheck {
    /// A stable name; the four canonical ones map into [`GateResults`].
    pub name: String,
    /// Shell command, run via `sh -c` in the worktree.
    pub command: String,
}

impl GateCheck {
    pub fn new(name: impl Into<String>, command: impl Into<String>) -> Self {
        Self { name: name.into(), command: command.into() }
    }
}

/// The ordered set of checks a gate runs. An empty spec is a no-op gate that
/// [`GateReport::passed`] treats as vacuously green (Phase 3 test convenience;
/// a misconfigured real gate is a Phase-4 discovery concern).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GateSpec {
    pub checks: Vec<GateCheck>,
}

impl GateSpec {
    pub fn new(checks: Vec<GateCheck>) -> Self {
        Self { checks }
    }
}

/// How much of a failing command's combined output to carry back to the worker
/// in the bounce — enough to act on, bounded so it never floods the turn.
pub const OUTPUT_TAIL_BYTES: usize = 4000;

/// The result of one check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub name: String,
    pub passed: bool,
    /// Tail of combined stdout+stderr — the evidence a bounce hands the worker.
    /// Empty on a pass.
    pub output_tail: String,
}

/// The outcome of a whole gate run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GateReport {
    pub results: Vec<CheckResult>,
}

impl GateReport {
    /// True when every check passed (vacuously true for an empty gate).
    pub fn passed(&self) -> bool {
        self.results.iter().all(|r| r.passed)
    }

    /// The failing checks, in order.
    pub fn failures(&self) -> Vec<&CheckResult> {
        self.results.iter().filter(|r| !r.passed).collect()
    }

    /// Project onto the report schema's fixed gate fields (handoff §4). Unknown
    /// check names simply don't map — the four canonical ones do.
    pub fn to_gate_results(&self) -> GateResults {
        let find = |n: &str| self.results.iter().find(|r| r.name == n).map(|r| r.passed);
        GateResults {
            tests: find("tests"),
            typecheck: find("typecheck"),
            lint: find("lint"),
            build: find("build"),
        }
    }

    /// The auto-bounce feedback (handoff §8, §11): the failing checks and their
    /// output, framed as a fix to make — not a new ticket, and not an override
    /// of the worker's task. Only called when there is at least one failure.
    pub fn bounce_message(&self, ticket: &str) -> String {
        let mut s = format!(
            "The exit gate for {ticket} failed. Your work is not done until it \
             passes. Fix the cause of each failing check below, then end your \
             turn with an updated `fleet-report` block as before. Do not change \
             files outside the ones you own to make a check pass.\n",
        );
        for f in self.failures() {
            s.push_str(&format!("\n── check `{}` FAILED ──\n", f.name));
            if f.output_tail.trim().is_empty() {
                s.push_str("(no output)\n");
            } else {
                s.push_str(f.output_tail.trim_end());
                s.push('\n');
            }
        }
        s
    }
}

/// The seam: run every check in `cwd` (the worker's worktree) and report which
/// passed. One impl now ([`ShellGateRunner`](../../fleetor_server) in the
/// server crate); a fake drives the loop tests.
pub trait GateRunner: Send + Sync {
    fn run(&self, cwd: &Path) -> anyhow::Result<GateReport>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_gate_is_vacuously_green() {
        assert!(GateReport::default().passed());
    }

    #[test]
    fn passed_requires_every_check() {
        let r = GateReport {
            results: vec![
                CheckResult { name: "tests".into(), passed: true, output_tail: String::new() },
                CheckResult { name: "build".into(), passed: false, output_tail: "boom".into() },
            ],
        };
        assert!(!r.passed());
        assert_eq!(r.failures().len(), 1);
        assert_eq!(r.to_gate_results().tests, Some(true));
        assert_eq!(r.to_gate_results().build, Some(false));
        assert_eq!(r.to_gate_results().lint, None);
    }

    #[test]
    fn bounce_message_names_the_failing_check_and_its_output() {
        let r = GateReport {
            results: vec![CheckResult {
                name: "build".into(),
                passed: false,
                output_tail: "error[E0425]: cannot find value".into(),
            }],
        };
        let msg = r.bounce_message("T-9");
        assert!(msg.contains("T-9"));
        assert!(msg.contains("`build` FAILED"));
        assert!(msg.contains("E0425"));
    }
}
