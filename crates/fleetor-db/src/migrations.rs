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
    // 0002 — Phase 3: parked out-of-scope discoveries (`backlog_add`). The
    // `leases` table (from 0001) already carries file ownership for
    // `claim_file`/`whos_working_on`; only the backlog needs a new table.
    r#"
    CREATE TABLE backlog (
        id       TEXT PRIMARY KEY,
        text     TEXT NOT NULL,
        added_by TEXT NOT NULL,            -- JSON Party
        ticket   TEXT,                     -- source ticket, if any
        ts       INTEGER NOT NULL
    );
    "#,
    // 0003 — the TUI pivot (D-030). Every table the headless supervisor owned
    // goes; `events` is all that is left, because a fleet of live terminals has
    // no state to persist beyond what it said. Appended rather than edited, per
    // this file's own rule, so an existing database migrates rather than being
    // rebuilt from a schema it never had.
    //
    // **`DELETE FROM events` is deliberate, and it is the destructive part.**
    // Every row already in the log is a variant `FleetEvent` no longer has —
    // worker states, ticket moves, tool activity. `events_since` would meet the
    // first of them on replay and, before the skip-and-warn below, would have
    // failed the whole read: a shell that shows nothing because of what happened
    // three phases ago. The log is an observability record of a system that has
    // been replaced, not durable user data.
    r#"
    DROP TABLE IF EXISTS tickets;
    DROP TABLE IF EXISTS leases;
    DROP TABLE IF EXISTS mail;
    DROP TABLE IF EXISTS knowledge_proposals;
    DROP TABLE IF EXISTS backlog;
    DELETE FROM events;
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
