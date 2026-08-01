//! `fleetor quality` — drive one ticket through the Phase 3 quality loop:
//! implement → gate → bounce/fix → peer review → done, with no human input.
//!
//! The fake path (default) is the exit test made runnable from the terminal: a
//! `qa-*` fake worker in a scratch worktree, a real [`ShellGateRunner`] whose
//! sole check is `test -f marker.ok`, and a fake reviewer. It prints the outcome
//! and the full event log so the loop is visible.

use anyhow::{Context, Result};
use fleetor_cc::FakeClaude;
use fleetor_core::gate::{GateCheck, GateSpec};
use fleetor_core::{Store, Ticket};
use fleetor_db::SqliteStore;
use fleetor_server::{run_quality_loop, QualityOptions, Reviewer, ShellGateRunner, SuperviseOptions};
use std::path::PathBuf;

pub struct QualityArgs {
    /// Worker scenario: `qa-bounce` (fails the gate once) or `qa-clean`.
    pub scenario: String,
    /// Reviewer scenario: `review-approve` or `review-count`.
    pub reviewer: String,
    /// Per-turn wall-clock timeout in seconds.
    pub timeout: u64,
    /// Repo root (for locating the fake-claude script); defaults to cwd.
    pub repo_root: PathBuf,
}

fn base_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME not set")?;
    Ok(PathBuf::from(home).join(".fleetor").join("_quality"))
}

fn demo_ticket() -> Ticket {
    Ticket::new(
        "T-quality",
        "make the exit gate pass",
        "Do the work so the exit gate (a marker file) is satisfied. Keep it to \
         your owned files.",
    )
    .with_files(vec!["marker.ok".into()])
}

pub fn run(args: QualityArgs) -> Result<()> {
    let base = base_dir()?;
    // A fresh worktree so a stale marker from a prior run can't pre-pass the gate.
    let cwd = base.join("wt").join(fleetor_core::ids::new_id("run"));
    std::fs::create_dir_all(&cwd)?;
    let store = SqliteStore::open(&base.join("state.db"))?;

    let script = args.repo_root.join("tests/fake-claude/fake-claude.mjs");
    let worker = FakeClaude { script: script.clone(), cwd: cwd.clone(), scenario: args.scenario.clone() };
    let reviewer_agent = FakeClaude { script, cwd: cwd.clone(), scenario: args.reviewer.clone() };

    let gate = ShellGateRunner::new(GateSpec::new(vec![GateCheck::new("build", "test -f marker.ok")]));

    let ticket = demo_ticket();
    let sup = SuperviseOptions::new(1, args.timeout);
    let mut opts = QualityOptions::new(sup, cwd.clone());
    opts.reviewer = Some(Reviewer { slot: 2, raw_log: None });

    eprintln!(
        "quality: worker=fake:{} reviewer=fake:{} in {}",
        args.scenario, args.reviewer, cwd.display()
    );
    let outcome = run_quality_loop(&worker, &reviewer_agent, &ticket, &store, &gate, &opts)?;

    println!("\noutcome: {outcome:?}");
    println!("\nevent log:");
    for (seq, ev) in store.events_since(0)? {
        println!("  [{seq:>3}] {:<14} {}", ev.kind(), serde_json::to_string(&ev)?);
    }
    let _ = std::fs::remove_dir_all(&cwd);
    Ok(())
}
