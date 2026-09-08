# 02: Codex resume spike

**What to build:** A verified answer to how `codex resume` works and which on-disk artifact it actually replays, so the Codex harness fill (06) captures the right files. Deliverable is a findings note, not shipped code.

**Blocked by:** None (can start immediately).

**Status:** command surface VERIFIED; artifact question has strong evidence but needs a runtime confirm (gates ticket 06, not 03)

- [x] `codex resume [SESSION_ID]` and `codex fork [SESSION_ID]` take a UUID directly (verified from CLI help, v0.153.4) — picker only when omitted.
- [~] Artifact evidence gathered: help frames `rollout-*.jsonl` as *legacy* (`codex migrate-rollouts`) and the sqlite thread-history as current — so today's `SqliteBackup` archive likely already suffices. Runtime confirm (restore sqlite-only, `codex resume`, observe) still owed; gates 06.
- [ ] Session-id source (where fleetor reads the resumable UUID) and load-time token behavior — open, live probe.
- [~] Tentative conclusion: capture both is likely unnecessary on this version; 06 keeps "both" until the runtime test proves sqlite-only.
- [x] Findings written to `.scratch/resume-past-runs/notes/codex-resume-spike.md`.
