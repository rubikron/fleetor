# 05: View — five panes, coexists with live

**What to build:** A viewed past run shows all five panes (orch + four workers) in their original layout, each resumed read-only, and can be opened while a fleet is live without disturbing it.

**Blocked by:** 04.

**Status:** backend + UI DONE + green (271 lib + 9 run-view-render tests); live five-pane render is the remaining shakedown

- [x] All five seats resume read-only in the original layout; each renders its own restored native transcript. `RunViewGrid` mirrors the live `TerminalGrid` (orch left, worker tabstrip right), every pane a `PaneId::View(cell)` spawned through `viewOpen(runId, seat)`.
- [x] View opens while a fleet is live: the live run's ptys and `state.db` keep running, hidden; exit-to-Live returns to the running fleet intact. Each viewer holds no `FLEET_SOCKET` and touches no live record (C6); `exitToLive` kills all five `RUN_VIEW_PANES`.
- [x] Each seat is restored into its own ephemeral config/home. `view::reset(fleetor, cell)` clears only `view/<cell>/`, so five concurrent opens do not collide. Tests: `view::resetting_one_cell_does_not_disturb_another`, `placement::a_worker_seat_resumes_into_its_own_cell_not_the_orchestrators`.
- [x] `Rename` harvest→restore round-trip proven across worker seats (the Claude-Code `.jsonl` restore into a worker cell).
- [x] Render test asserts the whole-grid takeover shows five panes. `the_run_view_grid_takeover_shows_all_five_seats` renders the real `RunViewGrid` and counts five terminal hosts + five labelled heads; fencing is read out of `RunViewGrid.tsx` (`readOnly` on every pane) and `TerminalPane.tsx` (the `onData` guard).
- [~] The **live** five-pane render — five real vendor sessions resuming at once under their per-cell homes (mixed Codex/Claude) — is the shakedown (`tauri dev` + open a full run from History).

Decision recorded as WP-26 C8 in `decisions.md`.

Note: a run with fewer than five recorded seats shows the backend's error on the missing cells; the degraded placeholder is ticket 10. Codex worker seats resume via ticket 06 (landed).
