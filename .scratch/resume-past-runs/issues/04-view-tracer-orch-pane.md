# 04: View — orch pane resumed, read-only (tracer bullet)

**What to build:** Opening a past run from History takes over the grid with the orchestrator pane resumed: the real `claude --resume` rendering that run's native transcript, read-only. The thinnest complete View — one seat, one harness, end to end.

**Blocked by:** 03.

**Status:** DONE + green (full `cargo test` suite passing, `tsc --noEmit` clean, `views.rs` 7/7, `run_view_renders` 7/7)

- [x] Selecting a past run's **View** action switches the app into the `run-view` stage-view; backend `view_open` command spawns the resumed pane.
- [x] The orch transcript is restored into an **ephemeral, fleet-owned config dir** under `~/.fleetor/view/`, cleared each open, a sibling of `_shell` (rotation never scoops it). **Design change from the PRD:** the viewer inherits the operator's **real HOME** (not a throwaway one) so `claude` can authenticate via the keychain (C73); isolation is the config dir + cwd. Proven by `place_view` test.
- [x] The pane spawns the actual vendor binary in resume mode (`spawn::view_command_with` → `claude --resume <id>`); zero tokens by construction (renders local jsonl). The **vendor project-slug** for the restore location is best-effort, flagged for the live `claude --resume` confirmation.
- [x] Input fenced (`readOnly` gates `term.onData` before `writePane`) + banner naming the run + Exit-to-Live (`killPane(view)` → History). Backend: the View pane holds no `FLEET_SOCKET` and never goes through `record_pane`/the gate/`PaneState`.
- [x] Tests: `a_viewer_restores_the_archived_transcript_and_builds_the_resume_command` (placement seam, whole read path, no pty), `a_viewer_resumes_read_only_…` (argv/env), `view::` slug + restore_dest, `PaneId::View` round-trip, and the C63-style `run_view_renders.rs` (real render probe + source-read checks, mutation-verified).

**Open (live shakedown, consistent with fleetor culture — no test can prove these):**
- The real `claude --resume <id>` render: vendor project-slug match, auth under the ephemeral config, and ~0 tokens on load. Needs a click in the running app.
- Keystroke-fence *behavior* is pinned by source-read, not a driven DOM event (repo carries no jsdom, per C24). 
- Tauri arg casing (`runId`) is pinned as a literal in the render test but only a live click exercises it.

**Landed backend chain:** `PaneId::View` → `runs::{restore_transcript,pane_records,archived_transcript}` → `view` module → `spawn::view_command_with` → `placement::place_view` + `PaneSpec::View` → `fleet::{spawn_view_pane,view_open}` + lib.rs registration. See decisions.md WP-26 C6.
