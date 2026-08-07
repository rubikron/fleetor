//! `fleetor-db` — SQLite behind the [`Store`] seam (BUILDING §3).
//!
//! WAL mode, a single connection guarded by a `Mutex` so there is exactly one
//! writer (BUILDING §8 "SQLite write contention").
//!
//! **One table after Phase 5.** `events` is the whole persistence story of a TUI
//! fleet: the panes hold their own state in their own terminals, so there is
//! nothing else worth surviving a restart. Migration 0003 drops the five tables
//! the headless supervisor owned.

pub mod archive;
mod migrations;

use anyhow::{Context, Result};
use fleetor_core::event::FleetEvent;
use fleetor_core::{time, Store};
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

pub struct SqliteStore {
    conn: Mutex<Connection>,
}

impl SqliteStore {
    /// Open (creating if needed) at `path`, enable WAL, and migrate.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path).with_context(|| format!("opening db {path:?}"))?;
        Self::init(conn)
    }

    /// In-memory store for tests.
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL").ok();
        conn.pragma_update(None, "foreign_keys", "ON").ok();
        migrations::migrate(&conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }
}

impl Store for SqliteStore {
    fn append_event(&self, event: &FleetEvent) -> Result<i64> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT INTO events (ts, kind, payload) VALUES (?1, ?2, ?3)",
            rusqlite::params![time::now_ms(), event.kind(), serde_json::to_string(event)?],
        )
        .context("append event")?;
        Ok(conn.last_insert_rowid())
    }

    /// Replay the log from `after`.
    ///
    /// **A row that will not decode is skipped, not fatal.** This read is the
    /// UI's entire history, and it runs on every boot: one undecodable payload —
    /// a variant deleted in a later phase, a row written by a build that has
    /// since changed — would otherwise take down the whole replay and leave the
    /// shell blank with no explanation. The row is named on stderr and the rest
    /// of the log still arrives (L9).
    fn events_since(&self, after: i64) -> Result<Vec<(i64, FleetEvent)>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt =
            conn.prepare("SELECT seq, payload FROM events WHERE seq > ?1 ORDER BY seq")?;
        let rows = stmt.query_map([after], |r| {
            let seq: i64 = r.get(0)?;
            let payload: String = r.get(1)?;
            Ok((seq, payload))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (seq, payload) = row?;
            match serde_json::from_str::<FleetEvent>(&payload) {
                Ok(event) => out.push((seq, event)),
                Err(e) => eprintln!("db: skipping event {seq} — it does not decode: {e}"),
            }
        }
        Ok(out)
    }

    fn latest_seq(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let seq: i64 = conn.query_row("SELECT COALESCE(MAX(seq), 0) FROM events", [], |r| r.get(0))?;
        Ok(seq)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fleetor_core::event::NoticeLevel;
    use fleetor_core::pane::PaneId;

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

    #[test]
    fn the_event_log_is_ordered_and_tailable() {
        let store = SqliteStore::open_in_memory().unwrap();
        let a = store.append_event(&message("take the parser")).unwrap();
        let b = store
            .append_event(&FleetEvent::Notice { level: NoticeLevel::Info, text: "hi".into() })
            .unwrap();
        assert!(b > a);

        let since_a = store.events_since(a).unwrap();
        assert_eq!(since_a.len(), 1);
        assert_eq!(since_a[0].0, b);

        let all = store.events_since(0).unwrap();
        assert_eq!(all.len(), 2, "replay from zero returns the whole log");
    }

    /// L9, as a test: one row that will not decode must cost that row and nothing
    /// else. This read is the UI's entire history on every boot, so failing it
    /// outright leaves the shell blank with no explanation.
    #[test]
    fn an_undecodable_row_is_skipped_rather_than_taking_the_replay_down() {
        let store = SqliteStore::open_in_memory().unwrap();
        store.append_event(&message("first")).unwrap();
        {
            let conn = store.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO events (ts, kind, payload) VALUES (1, 'worker-state', ?1)",
                rusqlite::params![r#"{"type":"worker-state","slot":1,"from":"idle","to":"working"}"#],
            )
            .unwrap();
        }
        store.append_event(&message("third")).unwrap();

        let events = store.events_since(0).unwrap();
        assert_eq!(events.len(), 2, "the two readable events survive the one that does not");
        let bodies: Vec<&str> = events
            .iter()
            .filter_map(|(_, e)| match e {
                FleetEvent::Message { body, .. } => Some(body.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(bodies, vec!["first", "third"]);
    }

    /// Migration 0003 removed the supervisor's tables. A database that still had
    /// them would let a stray `INSERT INTO tickets` compile and run somewhere.
    #[test]
    fn only_the_event_table_survives_the_pivot() {
        let store = SqliteStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")
            .unwrap();
        let tables: Vec<String> =
            stmt.query_map([], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
        assert_eq!(tables, vec!["events".to_string()], "left over: {tables:?}");
    }

    #[test]
    fn migrate_is_idempotent_across_reopen() {
        let store = SqliteStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        migrations::migrate(&conn).unwrap();
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v as usize, migrations::MIGRATIONS.len());
    }
}
