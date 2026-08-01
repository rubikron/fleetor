//! `fleetor-db` — SQLite behind the [`Store`] seam (BUILDING §3).
//!
//! WAL mode, a single connection guarded by a `Mutex` so there is exactly one
//! writer (BUILDING §8 "SQLite write contention"). Everything the supervisor
//! persists — tickets, the append-only event log, reports — goes through here.

mod migrations;

use anyhow::{Context, Result};
use fleetor_core::envelope::{Envelope, Party, Ref};
use fleetor_core::event::{FleetEvent, TicketState};
use fleetor_core::ownership::{BacklogItem, LeaseGrant, Owner};
use fleetor_core::report::Report;
use fleetor_core::ticket::Ticket;
use fleetor_core::{time, Store};
use rusqlite::{Connection, OptionalExtension};
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
    fn upsert_ticket(&self, ticket: &Ticket) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO tickets (id, title, body, files_owned, slot, state, budget, report, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8)
             ON CONFLICT(id) DO UPDATE SET
                title=excluded.title, body=excluded.body, files_owned=excluded.files_owned,
                slot=excluded.slot, state=excluded.state, budget=excluded.budget,
                updated_at=excluded.updated_at",
            rusqlite::params![
                ticket.id,
                ticket.title,
                ticket.body,
                serde_json::to_string(&ticket.files_owned)?,
                ticket.slot,
                serde_json::to_string(&ticket.state)?,
                serde_json::to_string(&ticket.budget)?,
                time::now_ms(),
            ],
        )
        .context("upsert ticket")?;
        Ok(())
    }

    fn set_ticket_state(&self, id: &str, state: TicketState) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE tickets SET state=?2, updated_at=?3 WHERE id=?1",
            rusqlite::params![id, serde_json::to_string(&state)?, time::now_ms()],
        )
        .context("set ticket state")?;
        Ok(())
    }

    fn save_report(&self, ticket: &str, _slot: u8, report: &Report) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE tickets SET report=?2, updated_at=?3 WHERE id=?1",
            rusqlite::params![ticket, serde_json::to_string(report)?, time::now_ms()],
        )
        .context("save report")?;
        Ok(())
    }

    fn append_event(&self, event: &FleetEvent) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO events (ts, kind, payload) VALUES (?1, ?2, ?3)",
            rusqlite::params![time::now_ms(), event.kind(), serde_json::to_string(event)?],
        )
        .context("append event")?;
        Ok(conn.last_insert_rowid())
    }

    fn tickets(&self) -> Result<Vec<Ticket>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, body, files_owned, slot, state, budget FROM tickets ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            let files: String = r.get(3)?;
            let state: String = r.get(5)?;
            let budget: String = r.get(6)?;
            Ok(Ticket {
                id: r.get(0)?,
                title: r.get(1)?,
                body: r.get(2)?,
                files_owned: serde_json::from_str(&files).unwrap_or_default(),
                slot: r.get(4)?,
                state: serde_json::from_str(&state).unwrap_or_default(),
                budget: serde_json::from_str(&budget).unwrap_or_default(),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn events_since(&self, after: i64) -> Result<Vec<(i64, FleetEvent)>> {
        let conn = self.conn.lock().unwrap();
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
            let ev: FleetEvent = serde_json::from_str(&payload)
                .with_context(|| format!("decoding event {seq}"))?;
            out.push((seq, ev));
        }
        Ok(out)
    }

    fn save_mail(&self, env: &Envelope) -> Result<()> {
        let (ref_kind, ref_val) = match &env.r#ref {
            Some(Ref::Ticket(t)) => (Some("ticket"), Some(t.clone())),
            Some(Ref::Branch(b)) => (Some("branch"), Some(b.clone())),
            None => (None, None),
        };
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO mail (id, from_party, to_party, kind, body, ref_kind, ref_val, ts, v, delivered)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0)",
            rusqlite::params![
                env.id,
                serde_json::to_string(&env.from)?,
                serde_json::to_string(&env.to)?,
                serde_json::to_string(&env.kind)?,
                env.body,
                ref_kind,
                ref_val,
                env.ts,
                env.v,
            ],
        )
        .context("save mail")?;
        Ok(())
    }

    fn take_mail(&self, to: &Party) -> Result<Vec<Envelope>> {
        let to_json = serde_json::to_string(to)?;
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, from_party, kind, body, ref_kind, ref_val, ts, v
             FROM mail WHERE to_party=?1 AND delivered=0 ORDER BY rowid",
        )?;
        let rows = stmt.query_map([&to_json], |r| {
            Ok((
                r.get::<_, String>(0)?, // id
                r.get::<_, String>(1)?, // from_party (json)
                r.get::<_, String>(2)?, // kind (json)
                r.get::<_, String>(3)?, // body
                r.get::<_, Option<String>>(4)?, // ref_kind
                r.get::<_, Option<String>>(5)?, // ref_val
                r.get::<_, i64>(6)?,    // ts
                r.get::<_, u32>(7)?,    // v
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, from, kind, body, ref_kind, ref_val, ts, v) = row?;
            let r#ref = match (ref_kind.as_deref(), ref_val) {
                (Some("ticket"), Some(val)) => Some(Ref::Ticket(val)),
                (Some("branch"), Some(val)) => Some(Ref::Branch(val)),
                _ => None,
            };
            out.push(Envelope {
                id,
                from: serde_json::from_str(&from).context("decode mail from_party")?,
                to: to.clone(),
                kind: serde_json::from_str(&kind).context("decode mail kind")?,
                body,
                r#ref,
                ts,
                v,
            });
        }
        drop(stmt);
        conn.execute(
            "UPDATE mail SET delivered=1 WHERE to_party=?1 AND delivered=0",
            [&to_json],
        )
        .context("mark mail delivered")?;
        Ok(out)
    }

    fn who_owns(&self, path: &str) -> Result<Vec<Owner>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT slot, ticket FROM leases WHERE path=?1 ORDER BY acquired_at")?;
        let rows = stmt.query_map([path], |r| {
            Ok(Owner { slot: r.get::<_, i64>(0)? as u8, ticket: r.get(1)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn claim_lease(&self, path: &str, slot: u8, ticket: &str) -> Result<LeaseGrant> {
        let conn = self.conn.lock().unwrap();
        // A lease held by a *different* slot blocks the claim (handoff §4).
        let held: Option<Owner> = conn
            .query_row(
                "SELECT slot, ticket FROM leases WHERE path=?1 AND slot<>?2 LIMIT 1",
                rusqlite::params![path, slot as i64],
                |r| Ok(Owner { slot: r.get::<_, i64>(0)? as u8, ticket: r.get(1)? }),
            )
            .optional()
            .context("checking existing lease")?;
        if let Some(held_by) = held {
            return Ok(LeaseGrant::Denied { held_by });
        }
        // Free, or already this slot's — grant it (idempotent for the same
        // (path, ticket)).
        conn.execute(
            "INSERT INTO leases (path, slot, ticket, acquired_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, NULL)
             ON CONFLICT(path, ticket) DO UPDATE SET slot=excluded.slot",
            rusqlite::params![path, slot as i64, ticket, time::now_ms()],
        )
        .context("granting lease")?;
        Ok(LeaseGrant::Granted)
    }

    fn add_backlog(&self, item: &BacklogItem) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO backlog (id, text, added_by, ticket, ts) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                item.id,
                item.text,
                serde_json::to_string(&item.added_by)?,
                item.ticket,
                item.ts,
            ],
        )
        .context("add backlog item")?;
        Ok(())
    }

    fn list_backlog(&self) -> Result<Vec<BacklogItem>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT id, text, added_by, ticket, ts FROM backlog ORDER BY ts, id")?;
        let rows = stmt.query_map([], |r| {
            let added_by: String = r.get(2)?;
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, added_by, r.get::<_, Option<String>>(3)?, r.get::<_, i64>(4)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, text, added_by, ticket, ts) = row?;
            out.push(BacklogItem {
                id,
                text,
                added_by: serde_json::from_str(&added_by).context("decode backlog added_by")?,
                ticket,
                ts,
            });
        }
        Ok(out)
    }
}

/// A `Budget` needs a `Default` for row decode; re-exported for convenience.
pub use fleetor_core::ticket::Budget;

#[cfg(test)]
mod tests {
    use super::*;
    use fleetor_core::event::{NoticeLevel, WorkerState};

    #[test]
    fn migrates_and_roundtrips_a_ticket() {
        let store = SqliteStore::open_in_memory().unwrap();
        let t = Ticket::new("T-1", "add thing", "do the thing");
        store.upsert_ticket(&t).unwrap();
        store.set_ticket_state("T-1", TicketState::InProgress).unwrap();

        let got = store.tickets().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, "T-1");
        assert_eq!(got[0].state, TicketState::InProgress);
    }

    #[test]
    fn event_log_is_ordered_and_tailable() {
        let store = SqliteStore::open_in_memory().unwrap();
        let a = store
            .append_event(&FleetEvent::WorkerState {
                slot: 1,
                from: WorkerState::Booting,
                to: WorkerState::Idle,
            })
            .unwrap();
        let b = store
            .append_event(&FleetEvent::Notice {
                level: NoticeLevel::Info,
                text: "hi".into(),
            })
            .unwrap();
        assert!(b > a);
        let since_a = store.events_since(a).unwrap();
        assert_eq!(since_a.len(), 1);
        assert_eq!(since_a[0].0, b);
    }

    #[test]
    fn migrate_is_idempotent_across_reopen() {
        // Two opens of the same in-memory-like flow must not re-run migrations.
        let store = SqliteStore::open_in_memory().unwrap();
        // A second migrate call is a no-op (version already at max).
        let conn = store.conn.lock().unwrap();
        migrations::migrate(&conn).unwrap();
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v as usize, migrations::MIGRATIONS.len());
    }
}
