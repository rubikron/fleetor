# WP-32 — opencode as a harness: orch and worker seats

status: in-progress size: M
depends-on: 25, 30 (the seam and its file layout, both landed) blocks: —
brief-cost: 0 — the briefs do not change (D-042). Only the carrier varies (D-145).

**The spec lives in [issue #79](https://github.com/rubikron/fleetor/issues/79).** Decisions D-140–D-145.
Measurements in [`opencode-spike-notes.md`](../notes/opencode-spike-notes.md).

## Outcome

An operator on an opencode plan can seat opencode as orch or as any worker, alone or beside the
other harnesses. Like WP-24, this is one file and one line: `placement/opencode.rs` answering every
checkpoint and its needles, added to `REGISTERED`, covered by conformance and the literals tripwire.
Runs parallel to WP-24 (cursor); both edit `REGISTERED`, a one-line merge.

## The seat, as decided

`opencode --standalone --auto` in the pane's cwd (D-144), on the operator's real data dir (D-143), with:

- `OPENCODE_CONFIG_DIR` = the seat dir: `plugins/fleetor-guardrail.js` (D-142) and `brief.md`;
- `OPENCODE_CONFIG` = the operator's `opencode.json`, read only (D-143);
- `OPENCODE_CONFIG_CONTENT` = `model` when picked (D-141), `default_agent: "fleetor"`,
  `agent.fleetor.prompt: "{file:…/brief.md}"` (D-145), `permission.external_directory: "deny"` (D-142).

No login check (D-140).

## Invariant guardrails

- **Tier 1.7** — auto-approve is narrowed: `--auto` only beside the guardrail plugin and the deny rule.
  If the paid spike shows the hook cannot refuse a call, D-142 reverses before any seat is registered.
- **Tier 1.4** — nothing on the message path; the hub and CLI suites pass unchanged.
- **D-042** — one `orch.md`, one `worker.md`.
- **D-110** — nothing starts an agent at launch. An opencode pane gets no `--prompt`.
- **`placement` reads nothing from the process** — the probe is handed `MachineFacts` (D-127).

## Phases

1. Zero-token spike. **Done** (`opencode-spike-notes.md`).
2. Paid spike, on the operator's word: the open list at the end of the notes.
3. `opencode.rs`, registered; conformance green.
4. One live opencode worker and one opencode orch complete a `fleet send` round trip.

## Session prompt

```
Work in the worktree ../fleetor-opencode on feat/wp-32-opencode. Read
docs/roadmap/32-opencode-tui.md, docs/notes/opencode-spike-notes.md and decisions D-140–D-145,
then placement/harness.rs (the interface) and placement/codex.rs (the closest template).
Continue from the first phase not marked done. Phase 2 spends the operator's Go plan: ask first.
Number new decisions from D-146 (WP-24 owns D-128 onward).
```

## Session exit checklist

- [ ] A decision for every default that moved, cited in the commit subject.
- [ ] Spike notes appended, stamped `opencode v2.2.5` or the build measured.
- [ ] The hub and CLI suites pass unchanged.
- [ ] This doc: status → landed, "How it landed" appended; `00-index.md` updated last.
