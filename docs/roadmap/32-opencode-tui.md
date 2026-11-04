# WP-32 — opencode as a harness: orch and worker seats

status: in-progress size: M
depends-on: 25, 30 (the seam and its file layout, both landed) blocks: —
brief-cost: 0 — the briefs do not change (D-042). Only the carrier varies (D-145).

**The spec lives in [issue #79](https://github.com/rubikron/fleetor/issues/79).** Decisions D-140–D-150.
Measurements in [`opencode-spike-notes.md`](../notes/opencode-spike-notes.md).

## Outcome

An operator on an opencode plan can seat opencode as orch or as any worker, alone or beside the
other harnesses. Like WP-24, this is one file and one line: `placement/opencode.rs` answering every
checkpoint and its needles, added to `REGISTERED`, covered by conformance and the literals tripwire.
Runs parallel to WP-24 (cursor); both edit `REGISTERED`, a one-line merge.

## The seat, as decided

`opencode --standalone` in the pane's cwd (D-144, D-149), on the operator's real data dir (D-143), with:

- `OPENCODE_CONFIG_DIR` = the seat dir: a snapshot of the operator's `opencode.json` with the seat's keys over it (D-146), `brief.md`, and `plugins/fleetor-guardrail.js` (D-142);
- `OPENCODE_CONFIG_CONTENT` (D-147) = `model` when picked (D-141), `default_agent: "fleetor"`, `agent.fleetor.prompt` = the brief (D-145), `permission: {"*": "allow", "external_directory": "deny"}` (D-149).

No login check (D-140), no trust record (D-150). Slice 1 has no resume, gauge or archived transcript (D-148).

## Invariant guardrails

- **Tier 1.7** — auto-approve is narrowed: `permission.*: allow` only beside the guardrail plugin and the deny rule.
- **Tier 1.4** — nothing on the message path; the hub and CLI suites pass unchanged.
- **D-042** — one `orch.md`, one `worker.md`.
- **D-110** — nothing starts an agent at launch. An opencode pane gets no `--prompt`.
- **`placement` reads nothing from the process** — the probe is handed `MachineFacts` (D-127).

## Phases

1. Zero-token spike. **Done** (`opencode-spike-notes.md`).
2. Paid spike. **Done** — every arm passed except `external_directory` against `shell`, which D-142 now records.
3. `opencode.rs`, registered; conformance green. **Done** (slice 1, D-148).
4. One live opencode worker and one opencode orch complete a `fleet send` round trip.

## Session prompt

```
Work in the worktree ../fleetor-opencode on feat/wp-32-opencode. Read
docs/roadmap/32-opencode-tui.md, docs/notes/opencode-spike-notes.md and decisions D-140–D-145,
then placement/harness.rs (the interface) and placement/codex.rs (the closest template).
Continue from the first phase not marked done. Phase 2 spends the operator's Go plan: ask first.
Number new decisions from D-151 (WP-24 owns D-128 onward).
```

## Session exit checklist

- [ ] A decision for every default that moved, cited in the commit subject.
- [ ] Spike notes appended, stamped `opencode v2.2.5` or the build measured.
- [ ] The hub and CLI suites pass unchanged.
- [ ] This doc: status → landed, "How it landed" appended; `00-index.md` updated last.
