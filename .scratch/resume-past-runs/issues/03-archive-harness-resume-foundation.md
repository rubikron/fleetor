# 03: Archive + harness resume foundation

**What to build:** Every registered TUI must declare how its archived transcript is restored, how resume is invoked, and how its resumable session id is captured — enforced so a harness cannot be half-registered. A freshly archived run records the resumable session id per seat. This is the seam View and Resume both stand on; nothing user-visible yet, but conformance and the manifest prove it.

**Blocked by:** 01 (Claude Code resume spike). Codex's checkpoint is filled with the already-known sqlite-restore to satisfy conformance; its real resume behavior is deferred to 06.

**Status:** DONE (262 lib tests green, no warnings)

- [x] The harness seam gains three mandatory checkpoints: restore (the inverse direction of the harvest `Transport`, in `runs::restore_transcript`), resume-invocation (`Harness::resume_args`, required), and session-id capture (`Harness::resumable_session_id`, required).
- [x] Both registered harnesses (Claude Code, Codex) supply all three — enforced by the compiler (required trait methods; the `context_gauge` test macro had to gain them too) and by the conformance test `every_registered_harness_says_how_to_resume_and_carries_the_id_in_its_argv`. Mutation check holds: stub resume to empty argv / restore to a no-op / id to None → the respective test fails.
- [x] `PaneRecord` gains `session_id`; captured in `rotate` after the harvest by `capture_session_ids`, proven end-to-end by `a_rotated_runs_manifest_carries_each_seats_resumable_session_id`.
- [x] Claude Code restore copies the archived `.jsonl` back (the `Transport::Rename` arm of `restore_transcript`); the id is the `.jsonl` filename stem (`claude_code_resumes_with_the_resume_flag_and_reads_its_id_from_the_filename`). Restoring *into* `projects/<slug>/` is View's orchestration (04); the primitive + round-trip prove the mechanic.
- [x] Restore-is-inverse-of-harvest round-trip for `Rename` passes (`a_renamed_transcript_restores_byte_for_byte_and_leaves_the_archive_untouched`) — harvest→restore is byte-lossless and leaves the frozen archive untouched.

**As-built notes:**
- Restore is the reverse direction of `Transport` (both arms a plain copy — the archive is one self-contained file; never a move, the archive is frozen). Non-exhaustive match preserves the "a fourth transport can't be declared without a restore arm" discipline.
- `resume_args` reuses `command_args` for Claude Code (`--resume <id>` + posture). Codex's is the verified command surface only (`resume <id>`); flag-splicing + which artifact it replays are ticket 06.
- `first_transcript_file` (harness.rs, `pub(crate)`) is the shared "find the transcript in an archived seat dir" helper both harnesses' id-capture uses.
