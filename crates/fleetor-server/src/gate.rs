//! `ShellGateRunner` — the concrete [`GateRunner`] (BUILDING §3, the 4th seam):
//! each check is a shell command run via `sh -c` in the worker's worktree, and
//! its exit status decides pass/fail. Combined stdout+stderr is captured and
//! tailed so a failing check hands the worker enough to fix it (handoff §8).
//!
//! [`GateRunner`]: fleetor_core::GateRunner

use anyhow::{Context, Result};
use fleetor_core::gate::{CheckResult, GateReport, GateRunner, GateSpec, OUTPUT_TAIL_BYTES};
use std::path::Path;
use std::process::Command;

/// Runs a fixed [`GateSpec`] of shell checks. In Phase 4 the spec is discovered
/// from the repo (CLAUDE.md / package.json / Makefile); here it is given.
pub struct ShellGateRunner {
    pub spec: GateSpec,
}

impl ShellGateRunner {
    pub fn new(spec: GateSpec) -> Self {
        Self { spec }
    }
}

impl GateRunner for ShellGateRunner {
    fn run(&self, cwd: &Path) -> Result<GateReport> {
        let mut results = Vec::with_capacity(self.spec.checks.len());
        for check in &self.spec.checks {
            let out = Command::new("sh")
                .arg("-c")
                .arg(&check.command)
                .current_dir(cwd)
                .output()
                .with_context(|| format!("running gate check `{}`", check.name))?;

            let passed = out.status.success();
            let output_tail = if passed {
                String::new()
            } else {
                let mut combined = String::from_utf8_lossy(&out.stdout).into_owned();
                combined.push_str(&String::from_utf8_lossy(&out.stderr));
                tail(&combined, OUTPUT_TAIL_BYTES)
            };
            results.push(CheckResult { name: check.name.clone(), passed, output_tail });
        }
        Ok(GateReport { results })
    }
}

/// Keep the last `max_bytes` of `s` on a char boundary (front-truncated so the
/// most recent, usually most relevant, output survives).
fn tail(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut start = s.len() - max_bytes;
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    format!("…{}", &s[start..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use fleetor_core::gate::GateCheck;

    #[test]
    fn passing_and_failing_checks_are_reported() {
        let spec = GateSpec::new(vec![
            GateCheck::new("tests", "exit 0"),
            GateCheck::new("build", "echo 'boom: E0425' 1>&2; exit 1"),
        ]);
        let report = ShellGateRunner::new(spec).run(std::env::temp_dir().as_path()).unwrap();
        assert!(!report.passed());
        let f = report.failures();
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].name, "build");
        assert!(f[0].output_tail.contains("E0425"));
        // The passing check carries no output.
        assert!(report.results.iter().find(|r| r.name == "tests").unwrap().output_tail.is_empty());
    }

    #[test]
    fn a_check_runs_in_the_given_cwd() {
        let dir = std::env::temp_dir().join(format!("fleetor-gate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("marker.ok"), b"x").unwrap();
        let spec = GateSpec::new(vec![GateCheck::new("build", "test -f marker.ok")]);
        let report = ShellGateRunner::new(spec).run(&dir).unwrap();
        assert!(report.passed(), "the marker file in cwd should make the check pass");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
