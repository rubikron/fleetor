//! Phase 1 exit test: the full ticket lifecycle against fake-claude, every
//! scenario (BUILDING §5, §6). Fast, free, deterministic — no tokens spent.

use fleetor_cc::FakeClaude;
use fleetor_core::event::{FleetEvent, TicketState};
use fleetor_core::report::ReportStatus;
use fleetor_core::{Store, Ticket};
use fleetor_db::SqliteStore;
use fleetor_server::{run_ticket, Outcome, SuperviseOptions};
use std::path::PathBuf;
use std::time::Duration;

fn fake_script() -> PathBuf {
    // crates/fleetor-server → repo root → tests/fake-claude/fake-claude.mjs
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fake-claude/fake-claude.mjs")
        .canonicalize()
        .expect("fake-claude.mjs must exist")
}

fn agent(scenario: &str) -> FakeClaude {
    FakeClaude {
        script: fake_script(),
        cwd: std::env::temp_dir(),
        scenario: scenario.to_string(),
    }
}

fn opts(secs: u64) -> SuperviseOptions {
    SuperviseOptions::new(1, secs)
}

fn ticket() -> Ticket {
    Ticket::new("T-101", "add a helper", "Add a helper fn and make the build pass.")
        .with_files(vec!["src/lib.rs".into()])
}

/// Count events of a given kind in the log.
fn count_kind(store: &SqliteStore, kind: &str) -> usize {
    store
        .events_since(0)
        .unwrap()
        .into_iter()
        .filter(|(_, e)| e.kind() == kind)
        .count()
}

#[test]
fn happy_path_reports_done_and_logs_the_lifecycle() {
    let store = SqliteStore::open_in_memory().unwrap();
    let t = ticket();
    let outcome = run_ticket(&agent("happy"), &t, &store, &opts(30)).unwrap();

    assert_eq!(outcome, Outcome::Reported { status: ReportStatus::Done });

    // Ticket landed on Done, report persisted.
    let tk = &store.tickets().unwrap()[0];
    assert_eq!(tk.state, TicketState::Done);

    // Tool activity was surfaced (Bash + Edit), and a report was filed.
    assert!(count_kind(&store, "tool-activity") >= 2, "expected tool activity events");
    assert_eq!(count_kind(&store, "report-filed"), 1);

    // The final ReportFiled event carries the done status.
    let filed = store
        .events_since(0)
        .unwrap()
        .into_iter()
        .find_map(|(_, e)| match e {
            FleetEvent::ReportFiled { status, .. } => Some(status),
            _ => None,
        })
        .unwrap();
    assert_eq!(filed, ReportStatus::Done);
}

#[test]
fn no_report_turn_triggers_a_reprompt_then_succeeds() {
    let store = SqliteStore::open_in_memory().unwrap();
    let t = ticket();
    let outcome = run_ticket(&agent("no-report"), &t, &store, &opts(30)).unwrap();

    // The reprompt recovers a proper report on the second turn.
    assert_eq!(outcome, Outcome::Reported { status: ReportStatus::Done });
    assert_eq!(store.tickets().unwrap()[0].state, TicketState::Done);
    // Exactly one report ends up filed.
    assert_eq!(count_kind(&store, "report-filed"), 1);
}

#[test]
fn malformed_report_block_fails_without_looping() {
    let store = SqliteStore::open_in_memory().unwrap();
    let t = ticket();
    let outcome = run_ticket(&agent("bad-report"), &t, &store, &opts(30)).unwrap();

    assert!(matches!(outcome, Outcome::BadReport { .. }));
    assert_eq!(store.tickets().unwrap()[0].state, TicketState::Failed);
    assert_eq!(count_kind(&store, "report-filed"), 0);
}

#[test]
fn wedged_worker_is_killed_by_the_watchdog() {
    let store = SqliteStore::open_in_memory().unwrap();
    let t = ticket();
    // Short budget so the test is quick; the fake hangs forever.
    let mut o = opts(0);
    o.idle_timeout = Duration::from_secs(2);
    let outcome = run_ticket(&agent("hang"), &t, &store, &o).unwrap();

    assert_eq!(outcome, Outcome::TimedOut);
    assert_eq!(store.tickets().unwrap()[0].state, TicketState::Failed);
}
