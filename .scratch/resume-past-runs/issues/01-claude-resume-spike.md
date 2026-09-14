# 01: Claude Code resume spike

**What to build:** A verified, written-down answer to how `claude --resume` behaves, so the harness resume-invocation checkpoint (03) and the read-only View pane (04) rest on facts, not guesses. Deliverable is a findings note under the feature folder, not shipped code.

**Blocked by:** None (can start immediately).

**Status:** command surface VERIFIED; runtime probes open (gated to ticket 04, not 03)

- [x] `claude --resume <session-id>` accepts an id directly (verified from CLI help, v2.1.263) — no picker. Also found: `--fork-session` (native fork-on-resume).
- [ ] Zero model/API calls on load — prior-verified by an earlier subagent, NOT re-run with a network watch this session. Needs a live launch; gates 04's "~0 token" acceptance.
- [ ] Characterize + script the >100k-token / >1h-idle resume-summary modal dismissal for a fenced View pane. Open.
- [x] cwd/config dependence noted: transcript at `<CLAUDE_CONFIG_DIR>/projects/<cwd-slug>/<id>.jsonl`; whether `--resume` matches on id alone or also needs the cwd-slug is the one live-probe item feeding 04.
- [x] Findings written to `.scratch/resume-past-runs/notes/claude-resume-spike.md`.
