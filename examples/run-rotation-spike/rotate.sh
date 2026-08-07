#!/usr/bin/env bash
# Spike: how do you move a WAL-mode SQLite database that a crashed process left
# behind, without losing what was in it?
#
# WP-11 rotates ~/.fleetor/_shell/state.db into ~/.fleetor/runs/<id>/ at every
# fleet start. The obvious implementation is `fs::rename` on state.db. This
# spike exists because that implementation silently destroys the run it is
# supposed to be preserving.
#
# Throwaway, per building.md §4 — allowed to be ugly, never imported.
# Findings: docs/notes/run-rotation-notes.md
#
#   ./rotate.sh

set -uo pipefail
D=$(mktemp -d)
trap 'rm -rf "$D"' EXIT

# --- build a specimen: a WAL-mode db whose writer died without checkpointing.
# sqlite3 checkpoints on a clean exit, so the connection is held open through a
# fifo and then SIGKILLed — which is what a force-quit or a panic looks like.
mkfifo "$D/ctl"
sqlite3 "$D/state.db" < "$D/ctl" >/dev/null 2>&1 &
pid=$!
exec 3>"$D/ctl"
cat >&3 <<'SQL'
PRAGMA journal_mode=WAL;
CREATE TABLE events (seq INTEGER PRIMARY KEY, ts INTEGER, kind TEXT, payload TEXT);
INSERT INTO events (ts, kind, payload)
  SELECT 0, 'message', 'body' FROM generate_series(1, 500);
SQL
sleep 0.5
kill -9 $pid 2>/dev/null
exec 3>&-
wait $pid 2>/dev/null

echo "=== specimen: what a killed writer leaves behind ==="
ls -l "$D"/state.db* | awk '{print $5, $9}'

# --- A: the obvious implementation. Move state.db, leave -wal/-shm behind.
mkdir -p "$D/a" && cp "$D"/state.db* "$D/a/"
rm -f "$D/a/state.db-wal" "$D/a/state.db-shm"
echo
echo "=== A — move state.db alone ==="
sqlite3 "$D/a/state.db" "SELECT COUNT(*) FROM events;" 2>&1

# --- B: checkpoint the WAL into the db first, then move state.db alone.
mkdir -p "$D/b" && cp "$D"/state.db* "$D/b/"
sqlite3 "$D/b/state.db" "PRAGMA wal_checkpoint(FULL);" >/dev/null 2>&1
rm -f "$D/b/state.db-wal" "$D/b/state.db-shm"
echo
echo "=== B — checkpoint(FULL), then move state.db alone ==="
sqlite3 "$D/b/state.db" "SELECT COUNT(*) FROM events;" 2>&1

# --- C: move all three files together, no checkpoint.
mkdir -p "$D/c" && cp "$D"/state.db* "$D/c/"
echo
echo "=== C — move state.db + -wal + -shm together ==="
sqlite3 "$D/c/state.db" "SELECT COUNT(*) FROM events;" 2>&1

# --- D: leave WAL mode entirely. Documented to checkpoint and delete the -wal in
# one step; this is the check of that claim, and it is the strategy WP-11 ships.
mkdir -p "$D/d" && cp "$D"/state.db* "$D/d/"
echo
echo "=== D — PRAGMA journal_mode=DELETE, then move state.db alone ==="
echo -n "  pragma returned: "; sqlite3 "$D/d/state.db" "PRAGMA journal_mode=DELETE;"
echo -n "  -wal still present: "; [ -f "$D/d/state.db-wal" ] && echo yes || echo no
mkdir -p "$D/d2" && cp "$D/d/state.db" "$D/d2/"
echo -n "  rows after moving state.db alone: "
sqlite3 "$D/d2/state.db" "SELECT COUNT(*) FROM events;" 2>&1
echo "  read-only open, no -shm present:"
sqlite3 "file:$D/d2/state.db?mode=ro" \
  "SELECT '    ' || kind || ' ' || COUNT(*) FROM events GROUP BY kind;" 2>&1
