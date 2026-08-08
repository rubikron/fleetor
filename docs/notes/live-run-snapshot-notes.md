# Reading a run that is still being written

Measured 2026-08-07 · sqlite3 3.43.2 (macOS system) · reproduce with
`examples/live-run-snapshot-spike/snapshot.sh`

WP-15 wakes the evaluator on `fleet handoff` — **mid-run**. D-058 archives the
*previous* run at bootstrap, so the run the evaluator has to read is the live
one: `~/.fleetor/_shell/state.db`, WAL mode, with the app's own writer
connection still holding it open. Nothing in WP-11 reads a run in that state,
and the naive implementations of "give the evaluator the run" are both wrong in
the silent direction, so this was measured before anything was built on it.

## The question

`runs::write_agent_view` produces the agent-facing `events.json` by calling
`fleetor_db::archive::to_json`, which opens the file **read-only**
(`archive.rs::open_readonly`, `OpenFlags::SQLITE_OPEN_READ_ONLY`). Every
existing caller passes a *frozen* archive — a database that has been through
`archive::freeze` and is out of WAL mode. Pointing it at a live one asks three
things nobody had checked.

## What was measured

A writer connection is held open through a fifo, WAL mode, never checkpointed —
the same shape as the fleet's one-connection-behind-a-mutex store. Reads are
issued from a second, separate connection while that writer is still alive.

| | Result |
|---|---|
| **Q1 — does a read-only connection see committed-but-uncheckpointed rows?** | **Yes.** 300 inserted, 300 read back, with `state.db` at 4 KB and the whole run in a 37 KB `-wal`. |
| **Q2 — does the read disturb the writer?** | **No.** The writer inserted again after the read and the next read saw 301. |
| **Q3 — is copying `state.db` alone enough?** | **No, and it fails loudly here rather than quietly.** The copied 4 KB file does not contain the schema: `Error: in prepare, unable to open database file (14)`. |
| Re-check after further traffic | 501 rows read back; the `handoff` row's payload came back whole, not just its count. |

Q3 is the same finding D-058 recorded for a *crashed* run
(`run-rotation-notes.md`), confirmed to hold for a *live* one. It is worth
restating because "copy the db and read the copy" is the obvious first
implementation, and here it produces an error rather than a wrong answer only
because the run was young — a run whose `state.db` had been checkpointed once
would copy as a **stale but readable** prefix, which is the failure that looks
like success.

## What follows

**The live run is read in place, read-only, through the function that already
exists.** `runs::snapshot_live_run` calls `archive::to_json` on
`_shell/state.db` directly. No copy of the database, no checkpoint, no
`VACUUM INTO`, no new `Store` method and no second serializer — so the
`events.json` an evaluator reads mid-run and the one it reads out of `runs/`
afterwards are produced by the same code and cannot drift into two formats
(D-059's requirement).

**Transcripts are copied, not moved.** `runs::harvest_transcripts` *moves* them
(D-059), which is correct at rotation — no pane exists then. At handoff every
pane is alive and Claude Code is still appending to those files; moving one out
from under a running session is a data-loss bug in the fleet the evaluator is
about to judge. The snapshot copies, and accepts that the run's transcripts then
exist twice on disk until the next rotation clears `_shell/retro/`.

## The limit, stated

The snapshot is a **point in time**: what the log and the transcripts held when
the handoff was appended. Anything a pane does after the handoff is not in it.
That is the intended reading — the retro is about the run up to the moment
`orch` said it was finished — but it means an evaluator that wants the last
word of a still-running pane will not find it, and the manifest says so in
`reading_this` rather than leaving it to be discovered.

There is one race this does not close and does not try to: a pane committing an
event between `to_json`'s read and the transcript copy will appear in
`events.json` and not in `transcripts/`. It costs one line of context in a
critique and is not worth a lock on the message path to prevent (Tier 1.4).
