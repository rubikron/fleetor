//! Schema migrations, applied from day one even for table 1 (BUILDING §3).
//!
//! Versioning uses SQLite's `user_version` pragma: each entry in [`MIGRATIONS`]
//! whose index ≥ the stored version is applied in order inside one transaction,
//! then the version is bumped. To evolve the schema, append a new SQL string —
//! never edit an existing one.
//!
//! The 0001 sketch carries all five tables from handoff §4 (`tickets`, `leases`,
//! `mail`, `events`, `knowledge_proposals`). Phase 1 writes `tickets` and
//! `events`; the rest are created now so their phases (2, 5) add rows, not
//! tables.

use anyhow::{Context, Result};
use rusqlite::Connection;

pub const MIGRATIONS: &[&str] = &[
    // 0001 — initial sketch.
    r#"
    CREATE TABLE tickets (
        id          TEXT PRIMARY KEY,
        title       TEXT NOT NULL,
        body        TEXT NOT NULL,
        files_owned TEXT NOT NULL,          -- JSON array of paths
        slot        INTEGER,
        state       TEXT NOT NULL,
        budget      TEXT NOT NULL,          -- JSON Budget
        report      TEXT,                   -- JSON Report, NULL until filed
        updated_at  INTEGER NOT NULL
    );

    CREATE TABLE events (
        seq     INTEGER PRIMARY KEY AUTOINCREMENT,
        ts      INTEGER NOT NULL,
        kind    TEXT NOT NULL,
        payload TEXT NOT NULL               -- full JSON FleetEvent
    );
    CREATE INDEX idx_events_kind ON events(kind);

    CREATE TABLE leases (
        path        TEXT NOT NULL,
        slot        INTEGER NOT NULL,
        ticket      TEXT NOT NULL,
        acquired_at INTEGER NOT NULL,
        expires_at  INTEGER,
        PRIMARY KEY (path, ticket)
    );

    CREATE TABLE mail (
        id        TEXT PRIMARY KEY,
        from_party TEXT NOT NULL,
        to_party  TEXT NOT NULL,
        kind      TEXT NOT NULL,
        body      TEXT NOT NULL,
        ref_kind  TEXT,
        ref_val   TEXT,
        ts        INTEGER NOT NULL,
        v         INTEGER NOT NULL,
        delivered INTEGER NOT NULL DEFAULT 0
    );

    CREATE TABLE knowledge_proposals (
        id          TEXT PRIMARY KEY,
        path        TEXT NOT NULL,
        body        TEXT NOT NULL,
        proposed_by TEXT NOT NULL,
        ticket      TEXT,
        state       TEXT NOT NULL DEFAULT 'pending',
        ts          INTEGER NOT NULL
    );
    "#,
];

/// Bring `conn` up to the latest schema version. Idempotent.
pub fn migrate(conn: &Connection) -> Result<()> {
    let current: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .context("reading user_version")?;
    let current = current.max(0) as usize;

    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
        conn.execute_batch(&format!("BEGIN;\n{sql}\nCOMMIT;"))
            .with_context(|| format!("applying migration {:04}", i + 1))?;
        // user_version can't be parameterized; the value is a trusted index.
        conn.execute_batch(&format!("PRAGMA user_version = {}", i + 1))
            .context("bumping user_version")?;
    }
    Ok(())
}
