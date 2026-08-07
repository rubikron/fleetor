//! Freezing a run's event log into an archive file, and reading one back
//! (WP-11, D-058).
//!
//! The live log is a WAL-mode database, which means it is **three** files and
//! the interesting one is usually `-wal`: SQLite only folds the write-ahead log
//! into `state.db` at a checkpoint, and the fleet's last connection frequently
//! does not get a clean close to do it in. A force-quit leaves a 4 KB `state.db`
//! that does not even contain the schema, beside a multi-megabyte `-wal` that
//! contains the entire run.
//!
//! So [`freeze`] is not tidying — it is the difference between archiving a run
//! and destroying it. `docs/notes/run-rotation-notes.md` measures all four
//! candidate strategies against a real crashed log; this module implements the
//! one that won.
//!
//! Everything here works on **paths, not [`Store`](fleetor_core::Store)s**. An
//! archived run is a file, not a seam: nothing appends to it, nothing migrates
//! it, and it never becomes the fourth trait `building.md` §3 forbids.

use std::path::Path;

use anyhow::{bail, Context, Result};
use fleetor_core::event::FleetEvent;
use fleetor_core::task::TaskChange;
use rusqlite::{Connection, OpenFlags};

/// How much of a headline a suggested run label gets before it is cut.
const HEADLINE_CHARS: usize = 72;

/// What one archived run holds, **counted from its own log** rather than
/// asserted by whatever wrote it (Tier 1.6).
///
/// `headline` is the one line worth putting in a suggested label: the first task
/// block's outcome if the fleet got as far as decomposing anything, else the
/// first thing anyone said. `None` means the run has neither — a start-then-quit
/// with nothing but boot notices in it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Digest {
    pub events: i64,
    pub messages: i64,
    pub tasks: i64,
    pub first_ts: Option<i64>,
    pub last_ts: Option<i64>,
    pub headline: Option<String>,
}

/// Take a database out of WAL mode: fold the write-ahead log into the main file
/// and delete it, leaving **one self-contained file** that can be moved, copied
/// or opened read-only on its own.
///
/// Verified rather than assumed (`docs/notes/run-rotation-notes.md` strategy D):
/// leaving WAL mode checkpoints and removes `-wal` as a single operation, so
/// there is no window in which a half-archived run exists on disk.
///
/// The returned mode is checked instead of trusted. A live reader elsewhere can
/// make the change silently fail — SQLite reports the mode it *kept* — and a
/// caller that took that for success would go on to move a `state.db` whose
/// contents are still in a `-wal` it is about to leave behind.
pub fn freeze(path: &Path) -> Result<()> {
    let conn = Connection::open(path)
        .with_context(|| format!("opening {} to freeze it", path.display()))?;
    let mode: String = conn
        .query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0))
        .with_context(|| format!("leaving WAL mode on {}", path.display()))?;
    if mode.eq_ignore_ascii_case("wal") {
        bail!("{} is still in WAL mode — something else holds it open", path.display());
    }
    Ok(())
}

/// Summarize an archived run without modifying it.
///
/// Opened **read-only**, and deliberately not through
/// [`SqliteStore::open`](crate::SqliteStore::open): that constructor migrates,
/// and an archive is finished. A run written by a schema this build does not
/// know should fail to summarize with a reason, never be silently upgraded by a
/// migration that has not been tested against it.
pub fn digest(path: &Path) -> Result<Digest> {
    let events = read(path)?;
    let mut digest = Digest { events: events.len() as i64, ..Digest::default() };
    let mut first_message: Option<String> = None;
    let mut first_outcome: Option<String> = None;

    for (_, event) in &events {
        match event {
            FleetEvent::Message { body, .. } => {
                digest.messages += 1;
                if first_message.is_none() && !body.trim().is_empty() {
                    first_message = Some(body.trim().to_string());
                }
            }
            FleetEvent::Task { change, at, .. } => {
                digest.tasks += 1;
                if let TaskChange::Posted { block } = change {
                    if first_outcome.is_none() && !block.outcome.trim().is_empty() {
                        first_outcome = Some(block.outcome.trim().to_string());
                    }
                }
                // A task carries its own timestamp on the payload; the DB `ts`
                // column below covers every other variant.
                digest.first_ts = Some(digest.first_ts.map_or(*at, |t: i64| t.min(*at)));
                digest.last_ts = Some(digest.last_ts.map_or(*at, |t: i64| t.max(*at)));
            }
            _ => {}
        }
    }

    let (db_first, db_last) = timestamps(path)?;
    digest.first_ts = min_opt(digest.first_ts, db_first);
    digest.last_ts = max_opt(digest.last_ts, db_last);
    // A decomposition beats a remark: the outcome is what the run was *for*.
    digest.headline = first_outcome.or(first_message).map(|h| truncate(&h, HEADLINE_CHARS));
    Ok(digest)
}

/// Replay an archived run, oldest first — the History view's whole data source.
///
/// Mirrors [`SqliteStore::events_since`](crate::SqliteStore) in the one way that
/// matters: **a row that will not decode is skipped, not fatal.** An archive is
/// read by builds newer than the one that wrote it, so a single payload from a
/// deleted variant must not blank the entire run (L9).
pub fn events(path: &Path, after: i64) -> Result<Vec<(i64, FleetEvent)>> {
    Ok(read(path)?.into_iter().filter(|(seq, _)| *seq > after).collect())
}

// --- internals ----------------------------------------------------------------

fn open_readonly(path: &Path) -> Result<Connection> {
    if !path.is_file() {
        bail!("{} is not there", path.display());
    }
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("opening {} read-only", path.display()))
}

fn read(path: &Path) -> Result<Vec<(i64, FleetEvent)>> {
    let conn = open_readonly(path)?;
    let mut stmt = conn
        .prepare("SELECT seq, payload FROM events ORDER BY seq")
        .with_context(|| format!("{} does not hold an event log", path.display()))?;
    let rows = stmt.query_map([], |r| {
        let seq: i64 = r.get(0)?;
        let payload: String = r.get(1)?;
        Ok((seq, payload))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (seq, payload) = row?;
        match serde_json::from_str::<FleetEvent>(&payload) {
            Ok(event) => out.push((seq, event)),
            Err(e) => eprintln!("archive: skipping event {seq} — it does not decode: {e}"),
        }
    }
    Ok(out)
}

fn timestamps(path: &Path) -> Result<(Option<i64>, Option<i64>)> {
    let conn = open_readonly(path)?;
    let row = conn.query_row("SELECT MIN(ts), MAX(ts) FROM events", [], |r| {
        Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<i64>>(1)?))
    })?;
    Ok(row)
}

fn min_opt(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (x, y) => x.or(y),
    }
}

fn max_opt(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (x, y) => x.or(y),
    }
}

/// Cut at a word boundary where there is one nearby, so a suggested label reads
/// as a phrase rather than a severed token.
fn truncate(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let head: String = flat.chars().take(max).collect();
    let cut = head.rfind(' ').filter(|i| *i > max / 2).unwrap_or(head.len());
    format!("{}…", head[..cut].trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SqliteStore;
    use fleetor_core::event::NoticeLevel;
    use fleetor_core::pane::PaneId;
    use fleetor_core::task::TaskBlock;
    use fleetor_core::Store;

    fn message(body: &str) -> FleetEvent {
        FleetEvent::Message {
            id: fleetor_core::ids::new_id("msg"),
            from: PaneId::Orch,
            to: PaneId::Worker(2),
            body: body.into(),
            group: None,
            accepted: true,
            detail: None,
        }
    }

    fn posted(outcome: &str, at: i64) -> FleetEvent {
        FleetEvent::Task {
            task: "task-1-0".into(),
            from: PaneId::Orch,
            at,
            change: TaskChange::Posted {
                block: TaskBlock {
                    outcome: outcome.into(),
                    technical: vec!["cargo test".into()],
                    semantic: vec!["the parser is the vision".into()],
                    worker: PaneId::Worker(2),
                    instructions: None,
                    parent: None,
                    converges_on: None,
                },
            },
        }
    }

    /// A store written and dropped, so the file on disk is whatever SQLite left.
    fn write_log(path: &Path, events: &[FleetEvent]) {
        let store = SqliteStore::open(path).unwrap();
        for event in events {
            store.append_event(event).unwrap();
        }
    }

    #[test]
    fn freezing_leaves_one_self_contained_file() {
        let dir = tempdir();
        let db = dir.join("state.db");
        write_log(&db, &[message("take the parser"), message("on it")]);

        freeze(&db).unwrap();

        assert!(!dir.join("state.db-wal").exists(), "the -wal must be gone, not merely stale");
        // The whole point: state.db alone, moved somewhere else, still reads.
        let moved = dir.join("moved.db");
        std::fs::rename(&db, &moved).unwrap();
        assert_eq!(digest(&moved).unwrap().messages, 2);
    }

    #[test]
    fn a_digest_counts_the_log_and_prefers_an_outcome_for_its_headline() {
        let dir = tempdir();
        let db = dir.join("state.db");
        write_log(
            &db,
            &[
                FleetEvent::Notice { level: NoticeLevel::Info, text: "target: logstat".into() },
                message("what is the --version flag for?"),
                posted("logstat can report its own version", 1_800_000_000_000),
                message("on it"),
            ],
        );
        freeze(&db).unwrap();

        let d = digest(&db).unwrap();
        assert_eq!(d.events, 4);
        assert_eq!(d.messages, 2);
        assert_eq!(d.tasks, 1);
        assert_eq!(d.headline.as_deref(), Some("logstat can report its own version"));
        assert!(d.first_ts.is_some() && d.last_ts.is_some());
    }

    #[test]
    fn a_run_with_nothing_but_notices_has_no_headline() {
        let dir = tempdir();
        let db = dir.join("state.db");
        write_log(
            &db,
            &[FleetEvent::Notice { level: NoticeLevel::Warn, text: "no fleet binary".into() }],
        );
        freeze(&db).unwrap();

        let d = digest(&db).unwrap();
        assert_eq!(d.events, 1);
        assert_eq!(d.messages, 0);
        assert_eq!(d.headline, None, "a start-then-quit must be labellable as empty, not guessed at");
    }

    #[test]
    fn replay_is_ordered_and_tailable() {
        let dir = tempdir();
        let db = dir.join("state.db");
        write_log(&db, &[message("one"), message("two"), message("three")]);
        freeze(&db).unwrap();

        let all = events(&db, 0).unwrap();
        assert_eq!(all.len(), 3);
        assert!(all[0].0 < all[1].0 && all[1].0 < all[2].0);
        assert_eq!(events(&db, all[0].0).unwrap().len(), 2);
    }

    #[test]
    fn reading_something_that_is_not_an_event_log_says_so_rather_than_panicking() {
        let dir = tempdir();
        let db = dir.join("state.db");
        assert!(digest(&db).is_err(), "a missing file is an error, not an empty run");

        std::fs::write(&db, b"not a database at all").unwrap();
        assert!(digest(&db).is_err());
    }

    #[test]
    fn a_headline_is_cut_at_a_word_boundary() {
        assert_eq!(truncate("short enough", 72), "short enough");
        assert_eq!(truncate("a\n  b   c", 72), "a b c", "newlines would break a one-line label");
        let long = "the parser accepts deeply nested groups and reports the position of the first unbalanced one";
        let cut = truncate(long, 40);
        assert!(cut.ends_with('…') && cut.chars().count() <= 41, "{cut}");
        assert!(!cut.contains("  "));
    }

    fn tempdir() -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!(
            "fleetor-archive-{}-{}",
            std::process::id(),
            fleetor_core::ids::new_id("t")
        ));
        std::fs::create_dir_all(&base).unwrap();
        base
    }
}
