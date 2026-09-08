# 06: Codex resume support

**What to build:** Codex seats resume correctly in View (and later Resume): the archive captures whatever `codex resume` actually replays, and the Codex resume-invocation is the real command rather than the conformance placeholder.

**Blocked by:** 02 (Codex resume spike), 05. **Pulled forward** ahead of 05 because the operator's orchestrators are Codex, so ticket 04's Claude-Code-only View could not open their runs.

**Status:** backend DONE + green (269 lib + full suite); live `codex resume` render is the remaining shakedown

- [x] Codex resumable id read from the **thread store specifically** (`thread_history_<n>.sqlite`), not any `.sqlite` — the live-shakedown bug (`resumable_session_id` grabbed `goals_1.sqlite`, reported a real session absent). Test: `the_resumable_id_is_read_from_the_thread_store_not_a_decoy_sqlite`.
- [x] `resume_args` is the verified `codex resume <uuid>`; the viewer passes an empty seat so no flags are spliced (their acceptance stays a 02 detail).
- [x] A Codex seat's **whole store** is restored (all `.sqlite`, not one) into the ephemeral `CODEX_HOME`, **seed-then-restore** so the run's threads win over the operator's snapshot; the operator's `auth.json` is copied in via `.for_the_operator()` (Codex auth lives in `CODEX_HOME`, no keychain). Test: `a_codex_viewer_restores_the_whole_store_and_resumes_by_thread_id`.
- [x] A Codex seat renders its genuine native session read-only. Live shakedown (codex 0.153.4) found `codex resume` opens on an interactive "Choose working directory" picker that a read-only pane cannot answer; fixed with `-c tui.resume_cwd=current` in `resume_args` (WP-26 C9). This also answered 02's artifact question: the restored `.sqlite` store **alone** drove resume to the picker (session found and loaded), so the legacy `rollout-*.jsonl` is not needed on this version.
- [x] `SqliteBackup` harvest→restore round-trip proven (whole-store restore + copy-not-move).

**Note:** the earlier session-id-recovery fix (`runs::seat_session_id`) also lets pre-ticket-03 runs view; a viewer's brief is a fixed read-only note (Codex's seeder refuses an empty brief).
