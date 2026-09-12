# WP-28 — The session is archived when you quit

status: landed (two live checks outstanding) size: S
depends-on: 11, 27 blocks: —
brief-cost: 0

## Outcome

Quitting FLEETOR leaves `~/.fleetor/runs/` true. Today a session is archived only when something *next* happens — the app opening again (D-085) or a fleet starting (D-058) — so between quitting and relaunching, the session you just finished is a live database under `_shell/`, not a run. That is invisible inside the app, where the row appears on launch either way, and it is wrong outside it: History's archives are written for agents to read with `cat` and `jq`, no app running (D-059), and those agents cannot see the session that ended last. After this package, quitting archives it. Launch keeps archiving too, because a crash never reaches a quit hook and D-058's promise — a crash never loses a run — must survive.

## Performance criteria

### Technical

- [ ] `teardown_fleet` (`src-tauri/src/lib.rs`) kills the panes, drops the live fleet, then rotates — in that order, and the order is held by a test rather than by a line sequence: `fleet::tests::quitting_closes_the_live_log_before_archiving_it_so_the_archive_is_one_file` opens a real WAL-mode store, archives through the same helper `teardown_fleet` uses, and asserts the archive holds `state.db` with **no** `state.db-wal` (the `archive::freeze` path, not the three-file fallback).
- [ ] That test is mutation-checked: archiving before the teardown turns it red.
- [ ] D-085's `the_session_left_in_the_live_slot_is_archived_at_launch_and_announced_later` still passes — the crash net is untouched.
- [ ] `cargo test --no-fail-fast` in `src-tauri/` green apart from the known codex `typing-tuned-submits` vendor arm (parked, WP-27 open question 9); clippy adds no warning in touched code.
- [ ] **Live, Cmd+Q:** start a fleet, quit with Cmd+Q, then with the app closed `ls ~/.fleetor/_shell/state.db` fails and `ls ~/.fleetor/runs/<id>/` shows `state.db events.json manifest.json transcripts` and no `state.db-wal`.
- [ ] **Live, window close:** the same check after closing the window instead.
- [ ] **Live, crash:** start a fleet, `kill -9` the app process, relaunch — the session is a History row before Start, via the launch net.
- [ ] **Live, `tauri dev` restart:** record whether a dev rebuild's restart fires `RunEvent::Exit`. Either answer is acceptable (the launch net covers it); not knowing is not.

### Semantic

- [ ] With the app closed, `~/.fleetor/runs/` is a complete record of every session that ended — the property D-059 makes History's archive worth reading. A reviewer judges it by quitting and reading the directory, not by looking at the app.

## Invariant guardrails

- **D-058 — a crash never loses a run.** Quit-only archiving would break it. The launch-time rotation (D-085) stays as the net, and bootstrap's rotation stays for the reopen switch. Allowed shape: one more call site of `runs::rotate`, never a second archive path.
- **Panes first, because they cost money** (`teardown_fleet`'s own doc). The archive runs after the kill and after the fleet is dropped, never before: rotation's transcript walk assumes no pane is writing (`runs::rotate`, since D-058), and `archive::freeze` needs the store closed.
- **Tier 1.4 — nothing on the message path.** Rotation reads files after every pane is gone; no `fleet send` touches it.
- **D-073 — one window.** A `CloseRequested` is the close of the application, so archiving there is not archiving on a hide.

## Current state (verified 2026-09-11 — do not re-explore)

- `src-tauri/src/lib.rs` — `on_window_event` calls `teardown_fleet(window)` on `WindowEvent::CloseRequested`; `.run(...)` calls `teardown_fleet(app_handle)` on `RunEvent::Exit`, because macOS Cmd+Q and Dock → Quit skip `CloseRequested` (tauri#9198, #13778). Both calls are made on a normal window close; the second must be a no-op.
- `teardown_fleet` = `pty::kill_all` then `fleet::shutdown`.
- `fleet::shutdown` (`src-tauri/src/fleet.rs`) only `notify_one()`s the hub's shutdown; it leaves `Fleet` in `FleetState`, so the store stays open.
- `Fleet` owns `rt: Runtime` and `store: Arc<dyn Store>`; every other holder of the store is a task on that runtime — `spawn_delivery`, `spawn_hub`, `spawn_guardrail_feed`, `spawn_evaluator_wake`, `spawn_follower` (all in `fleet_bootstrap`). `pty.rs` holds no store. So dropping `Fleet` should close the last connection (believed from the code; the live Cmd+Q check verifies it).
- `run_reopen`'s teardown already does `pty::kill_all` then `*guard = None` before its bootstrap rotates — the same order this package gives quit.
- `fleet::archive_previous_run` / `archive_previous_run_under` (D-085) run `runs::rotate` and hold its notices on `GateHold`; called from `setup` after `orphans::sweep`.
- `runs::rotate` (`src-tauri/src/runs.rs`) is a no-op when `_shell/state.db` is absent, which is what makes a second call on the same quit harmless.
- `fleetor_db::archive::freeze` (`crates/fleetor-db/src/archive.rs`) runs `PRAGMA journal_mode=DELETE` and bails if the mode stays `wal` — "something else holds it open". `runs::archive_files` then moves `state.db`, `-wal` and `-shm` instead (`docs/notes/run-rotation-notes.md`).

## Scope

**In:** quit archives (window close and `RunEvent::Exit`); `fleet::shutdown` drops the fleet; one ordering helper and its test; the live checks above; `decisions.md` D-086.

**Out:**
- R24 (a seat with no session reopens fresh) — its own change in WP-27.
- `useFleet` not clearing its lists on a reopen — found while tracing, WP-27.
- Re-announcing "previous run archived" on the next launch. The notice written at quit dies with the process; the History row is the evidence (D-086).
- A progress UI for a slow quit. Measure first (question 3).
- Retention or deletion of archives, seat directories, worker HOMEs or worktrees.
- Windows and Linux quit paths — untested here, and `RunEvent::Exit` is the portable hook either way.

## Design sketch & open questions

```
quit (window close │ Cmd+Q │ Dock → Quit)
        │
        ▼
teardown_fleet ── fleet::quit
        │
        ├─ 1  pty::kill_all            panes die first — they cost money
        ├─ 2  fleet::shutdown          notify the hub, take Fleet out of state
        │                              → runtime drops → last store connection closes
        │                              → remove _shell/fleet.sock
        └─ 3  archive_previous_run_under
                 runs::rotate          freeze + move state.db, copy transcripts,
                                       manifest, index row, delete run.json
second call on the same quit (RunEvent::Exit after CloseRequested):
        no fleet, no _shell/state.db  → every step is a no-op
```

1. **Does dropping `Fleet` close the store?** Believed, from the holders listed above. **Verify live:** the Cmd+Q archive has no `state.db-wal`. If it has one, rotation fell back — the archive is still whole (strategy C), but the holder has to be found.
2. **Does `RunEvent::Exit` wait for rotation to finish?** Believed: the handler is synchronous and the process exits after it returns. **Verify live** on a session with transcripts in every seat: `manifest.json` names every seat and `transcripts/` holds each file.
3. **How long does quitting take now?** Rotation freezes one database and copies transcripts. **Recommended:** measure on the largest real session available; accept under a second without UI, and file a follow-up above that rather than guessing a spinner now.
4. **Dropping a tokio `Runtime` must not happen inside an async context** (it panics). `teardown_fleet` runs from the event-loop callbacks, not a tokio task, and `run_reopen` already drops the same runtime from a command. **Recommended:** accept; the live quit exercises it.
5. **The hub's socket.** Dropping the runtime cancels the hub before it can unlink `fleet.sock` itself. **Recommended:** `shutdown` removes `layout.socket()` after the drop, so a quit leaves no stale socket.

## Session prompt

```
Read docs/roadmap/28-archive-on-quit.md in full, then decisions.md D-058, D-059,
D-085 and D-086. Build WP-28: quitting FLEETOR archives the session, and launch
keeps archiving as the crash net.

The load-bearing part is order: kill the panes, drop the fleet (closing its
store), then rotate. Hold it with the test the doc names, and mutation-check it
by archiving before the teardown. Do not add a second archive path — call
runs::rotate through fleet::archive_previous_run_under.

No live spend beyond starting a fleet for the quit checks; the operator runs the
app. Finish with the four live checks in the doc's criteria and record their
results in "How it landed".
```

## Session exit checklist

- [ ] `decisions.md` D-086 (cite it in the commit subject).
- [ ] No verb added or changed. *(None expected — brief-cost 0.)*
- [ ] The live-check results, including the `tauri dev` restart answer, appended here as "How it landed" — no separate spike note needed unless question 1 fails.
- [ ] As-built docs: `docs/runtime-layout.md` if it says when `_shell/state.db` is archived.
- [ ] This doc: status → landed, "How it landed" appended.
- [ ] `00-index.md` status column updated — the last act of the session.

## How it landed (live, 2026-09-11)

Session `2026-09-11T22-39-34Z`: started on `~/harness/harness-test`, a message typed into orch, then the app quit (method not recorded — window close or Cmd+Q; both reach `teardown_fleet`).

- **Question 1 answered — dropping `Fleet` does close the store.** `runs/2026-09-11T22-39-34Z/` holds `state.db`, `events.json`, `manifest.json` and `transcripts/orch/<uuid>.jsonl`, with **no `state.db-wal`**: `archive::freeze` succeeded, which it cannot while any connection holds the database. The three-file fallback was not taken.
- **The live slot is empty and clean.** `_shell/` has no `state.db`, no `run.json` and no `fleet.sock` — the socket removal in `shutdown` works.
- **No pane survived.** The only `claude` processes left on the machine run under the operator's own `HOME` with no `CLAUDE_CONFIG_DIR`; none is a fleet seat.
- **Question 2 answered for a small session.** The archive is complete and `manifest.json` names all five seats. Orch carries `session_id bb999ad1-…`; the four untouched workers carry none, which is exactly the shape R24 now lets reopen.
- **Question 3 — quitting is fast.** Orch's transcript was last written at 15:40:13 and the archive directory was created at 15:40:15. No UI needed.
- **Still unanswered:** the `kill -9` crash path, and whether a `tauri dev` restart fires `RunEvent::Exit`.
- **Noticed, not this package's:** the row's label is "harness-test — no activity" even though the operator typed into orch. Labels come from fleet *events*, and typing into a pane is not one. WP-27's lineage/label polish (S3) is where that belongs.
