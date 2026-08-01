//! Phase 3 EXIT TEST (BUILDING §6): a deliberately buggy ticket bounces, gets
//! fixed, passes review — no human input.
//!
//! Driven against fake-claude with a *real* [`ShellGateRunner`]: the fake worker
//! only writes the gate's marker file on its second turn, so the real gate
//! (`test -f marker.ok`) fails first, the loop bounces the failure back, the
//! worker fixes it, the gate goes green, a fresh reviewer approves, and the
//! ticket lands on Done. No tokens, fully deterministic.

use fleetor_cc::FakeClaude;
use fleetor_core::event::TicketState;
use fleetor_core::gate::{GateCheck, GateSpec};
use fleetor_core::{GateRunner, Store, Ticket};
use fleetor_core::gate::GateReport;
use fleetor_db::SqliteStore;
use fleetor_server::{run_quality_loop, QualityOptions, QualityOutcome, Reviewer, ShellGateRunner};
use fleetor_server::SuperviseOptions;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn fake_script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fake-claude/fake-claude.mjs")
        .canonicalize()
        .expect("fake-claude.mjs must exist")
}

/// A fresh worktree dir for one test (isolated so the marker file can't leak).
fn worktree(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fleetor-qa-{tag}-{}-{}",
        std::process::id(),
        fleetor_core::ids::new_id("w")
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A worker fake in `cwd` running one of the self-contained `qa-*` scenarios.
fn worker(cwd: &Path, scenario: &str) -> FakeClaude {
    FakeClaude { script: fake_script(), cwd: cwd.to_path_buf(), scenario: scenario.into() }
}

fn reviewer_agent(cwd: &Path, scenario: &str) -> FakeClaude {
    FakeClaude { script: fake_script(), cwd: cwd.to_path_buf(), scenario: scenario.into() }
}

fn ticket() -> Ticket {
    Ticket::new("T-301", "add a helper that builds", "Add the helper and make the exit gate pass.")
        .with_files(vec!["marker.ok".into()])
}

fn gate() -> ShellGateRunner {
    ShellGateRunner::new(GateSpec::new(vec![GateCheck::new("build", "test -f marker.ok")]))
}

fn opts(cwd: PathBuf, reviewer: Option<Reviewer>) -> QualityOptions {
    let mut sup = SuperviseOptions::new(1, 30);
    sup.max_turns = 2;
    let mut o = QualityOptions::new(sup, cwd);
    o.reviewer = reviewer;
    o
}

fn count_kind(store: &SqliteStore, kind: &str) -> usize {
    store.events_since(0).unwrap().into_iter().filter(|(_, e)| e.kind() == kind).count()
}

/// THE exit test: buggy → bounce → fix → gate green → review approved → Done.
#[test]
fn buggy_ticket_bounces_gets_fixed_and_passes_review() {
    let cwd = worktree("exit");
    let store = SqliteStore::open_in_memory().unwrap();
    let t = ticket();

    let w = worker(&cwd, "qa-bounce");
    let r = reviewer_agent(&cwd, "review-approve");
    let g = gate();
    let o = opts(cwd.clone(), Some(Reviewer { slot: 2, raw_log: None }));

    let outcome = run_quality_loop(&w, &r, &t, &store, &g, &o).unwrap();

    assert_eq!(outcome, QualityOutcome::Passed { gate_bounces: 1, review_bounces: 0 });
    assert_eq!(store.tickets().unwrap()[0].state, TicketState::Done);
    // The gate ran twice (fail then pass); review ran once (approve).
    assert_eq!(count_kind(&store, "gate-result"), 2);
    assert_eq!(count_kind(&store, "review-result"), 1);
    let _ = std::fs::remove_dir_all(&cwd);
}

/// Review requests changes once, the worker fixes, the second review approves.
#[test]
fn review_requests_changes_then_approves_after_a_fix() {
    let cwd = worktree("review");
    let store = SqliteStore::open_in_memory().unwrap();
    let t = ticket();

    // Worker writes the marker on turn 1, so the gate is green from the start;
    // the only bounce comes from review. The reviewer keeps its own counter file
    // in cwd (approves on the 2nd round).
    let w = worker(&cwd, "qa-clean");
    let r = reviewer_agent(&cwd, "review-count");
    let g = gate();
    let o = opts(cwd.clone(), Some(Reviewer { slot: 2, raw_log: None }));

    let outcome = run_quality_loop(&w, &r, &t, &store, &g, &o).unwrap();

    assert_eq!(outcome, QualityOutcome::Passed { gate_bounces: 0, review_bounces: 1 });
    assert_eq!(store.tickets().unwrap()[0].state, TicketState::Done);
    assert_eq!(count_kind(&store, "review-result"), 2, "one changes-requested + one approved");
    let _ = std::fs::remove_dir_all(&cwd);
}

/// A gate that never goes green escalates to the lead after the retry cap
/// (handoff §8) rather than grinding forever.
#[test]
fn gate_that_never_passes_escalates_after_the_cap() {
    let cwd = worktree("escalate");
    let store = SqliteStore::open_in_memory().unwrap();
    let t = ticket();

    // A gate whose check always fails, regardless of what the worker does.
    struct AlwaysFail {
        runs: Arc<AtomicUsize>,
    }
    impl GateRunner for AlwaysFail {
        fn run(&self, _cwd: &Path) -> anyhow::Result<GateReport> {
            self.runs.fetch_add(1, Ordering::SeqCst);
            Ok(GateReport {
                results: vec![fleetor_core::gate::CheckResult {
                    name: "build".into(),
                    passed: false,
                    output_tail: "still broken".into(),
                }],
            })
        }
    }
    let runs = Arc::new(AtomicUsize::new(0));
    let g = AlwaysFail { runs: runs.clone() };

    // Worker keeps reporting done, but the gate is impossible to satisfy.
    let w = worker(&cwd, "qa-bounce");
    let r = reviewer_agent(&cwd, "review-approve");
    let mut o = opts(cwd.clone(), Some(Reviewer { slot: 2, raw_log: None }));
    o.gate_retry_cap = 2;

    let outcome = run_quality_loop(&w, &r, &t, &store, &g, &o).unwrap();

    assert_eq!(outcome, QualityOutcome::GateEscalated);
    assert_eq!(store.tickets().unwrap()[0].state, TicketState::Blocked);
    // Initial gate + 2 bounces = 3 gate runs (cap = 2 bounces).
    assert_eq!(runs.load(Ordering::SeqCst), 3);
    let _ = std::fs::remove_dir_all(&cwd);
}
