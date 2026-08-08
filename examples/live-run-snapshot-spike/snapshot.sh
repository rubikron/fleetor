#!/usr/bin/env bash
# Spike: can the evaluator be handed a readable copy of the run that is still
# being written?
#
# WP-15 wakes the evaluator when `orch` calls `fleet handoff` — mid-run. D-058
# rotates the *previous* run at bootstrap, so the run the evaluator wants is the
# live one: `~/.fleetor/_shell/state.db`, in WAL mode, with the app's own writer
# connection holding it open. `runs::write_agent_view` calls
# `fleetor_db::archive::to_json`, which opens SQLITE_OPEN_READ_ONLY.
#
# Three questions, none of which are safe to reason about:
#
#   Q1  Does a read-only connection see rows a live writer has committed but
#       not checkpointed? (If it only sees `state.db`, the answer is an empty
#       or stale run and the failure is silent.)
#   Q2  Does opening it read-only disturb the writer?
#   Q3  Is a plain `cp` of state.db alone enough? (D-058 says no for a *dead*
#       run. Confirm it is also no for a live one, since that is the naive fix.)
#
# Throwaway, per building.md §4 — allowed to be ugly, never imported.
# Findings: docs/notes/live-run-snapshot-notes.md
#
#   ./snapshot.sh

set -uo pipefail
D=$(mktemp -d)
trap 'rm -rf "$D"' EXIT

pass=0; fail=0
ok()   { printf '  \033[32mPASS\033[0m  %s\n' "$1"; pass=$((pass+1)); }
bad()  { printf '  \033[31mFAIL\033[0m  %s\n' "$1"; fail=$((fail+1)); }
check(){ [ "$2" = "$3" ] && ok "$1 ($2)" || bad "$1 — expected $3, got $2"; }

echo "sqlite3: $(sqlite3 --version)"
echo

# --- a live writer: a connection held open, WAL mode, never checkpointed ------
# The fleet app's own shape: one long-lived connection behind a mutex.
mkfifo "$D/ctl"
sqlite3 "$D/state.db" < "$D/ctl" >/dev/null 2>&1 &
writer=$!
exec 3>"$D/ctl"

cat >&3 <<'SQL'
PRAGMA journal_mode=WAL;
CREATE TABLE events (seq INTEGER PRIMARY KEY, ts INTEGER, kind TEXT, payload TEXT);
INSERT INTO events (ts, kind, payload)
  SELECT 0, 'message', '{"type":"message","body":"pre-handoff"}' FROM generate_series(1, 300);
SELECT 'committed';
SQL
sleep 0.5

echo "1. the live run, mid-write"
echo "   state.db  $(stat -f%z "$D/state.db" 2>/dev/null || stat -c%s "$D/state.db") bytes"
echo "   -wal      $(stat -f%z "$D/state.db-wal" 2>/dev/null || stat -c%s "$D/state.db-wal") bytes"
[ -f "$D/state.db-shm" ] && echo "   -shm      present"
echo

echo "2. Q1 — a read-only connection, while the writer still holds it"
got=$(sqlite3 "file:$D/state.db?mode=ro" "SELECT count(*) FROM events" 2>&1)
check "read-only sees every committed row" "$got" "300"

echo
echo "3. Q2 — the writer is undisturbed by that read"
cat >&3 <<'SQL'
INSERT INTO events (ts, kind, payload)
  VALUES (0, 'handoff', '{"type":"handoff","built":"after the read"}');
SELECT 'still writing';
SQL
sleep 0.5
got=$(sqlite3 "file:$D/state.db?mode=ro" "SELECT count(*) FROM events" 2>&1)
check "the writer kept writing and the reader sees the new row" "$got" "301"

echo
echo "4. Q3 — copying state.db alone (the naive snapshot)"
cp "$D/state.db" "$D/naive.db"
got=$(sqlite3 "file:$D/naive.db?mode=ro" "SELECT count(*) FROM events" 2>&1)
case "$got" in
  301) bad "state.db alone carried the whole run — a checkpoint must have happened" ;;
  *)   ok "state.db alone is NOT the run (got: ${got:-<error>}) — the naive copy loses it" ;;
esac

echo
echo "5. the read-only route again, after more traffic, to rule out a fluke"
cat >&3 <<'SQL'
INSERT INTO events (ts, kind, payload)
  SELECT 0, 'message', '{"type":"message"}' FROM generate_series(1, 200);
SELECT 'more';
SQL
sleep 0.5
got=$(sqlite3 "file:$D/state.db?mode=ro" "SELECT count(*) FROM events" 2>&1)
check "read-only still sees the whole log" "$got" "501"

# The reader must also produce rows, not just a count — to_json selects payloads.
got=$(sqlite3 "file:$D/state.db?mode=ro" "SELECT payload FROM events WHERE kind='handoff'" 2>&1)
case "$got" in
  *"after the read"*) ok "payload of the handoff row reads back whole" ;;
  *)                  bad "payload did not read back: $got" ;;
esac

exec 3>&-
wait "$writer" 2>/dev/null

echo
echo "----"
echo "pass $pass   fail $fail"
[ "$fail" -eq 0 ]
