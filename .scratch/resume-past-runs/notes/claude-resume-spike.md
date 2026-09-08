# Claude Code resume spike — findings

Binary probed: `claude` v2.1.263 (via cmux-cli-shim, which forwards real CLI help).

## Command surface (VERIFIED this session, from `claude --help`)

- `claude --resume <session-id>` accepts a **session id directly** — no interactive picker required. This is the resume-invocation for the Claude Code harness (03).
- `-c, --continue` resumes the most recent conversation (not what we want — we resume a specific archived id).
- `--fork-session` — **"When resuming, create a new session ID"**. Native fork-on-resume. Relevant to Resume (08/09): resuming with `--fork-session` mints a new vendor session id instead of appending to the resumed one.
- Interactive by default; `-p/--print` is the headless mode. View/Resume want the **interactive** TUI (no `-p`), which is exactly the default.

## Token behavior on load

- **Prior-verified (earlier session, subagent), NOT re-run this turn:** `claude --resume <id>` renders from the local `.jsonl` and makes ~0 model/API calls on load — it displays the transcript and waits for input.
- **Not yet re-confirmed with `ANTHROPIC_LOG=debug` / network watch this session.** Needs a live launch to re-verify; blocks 04's "~0 token" acceptance, not 03.

## Resume-summary modal

- Prior finding: sessions >100k tokens or >1h idle can show a "resume full / from summary" modal on load. A fenced read-only View pane (04) must pre-answer/dismiss it so the pane isn't stuck on a dialog it can't drive. Exact trigger + dismissal key sequence still to be characterized on a live large session.

## Config/cwd dependence

- Claude Code finds a transcript under `<CLAUDE_CONFIG_DIR>/projects/<cwd-slug>/<session-id>.jsonl`. Restore (03) must land the `.jsonl` under the ephemeral config's `projects/<slug>/`, and the resumed pane's cwd must produce the same slug. To confirm on a live restore: whether `--resume <id>` matches purely on id or also requires the cwd-slug to line up.

## Open items (live probe, gated to 04 not 03)
- Re-confirm ~0 tokens on load with network watch.
- Characterize + script the resume-summary modal dismissal.
- Confirm id-vs-cwd matching for a restored transcript.
