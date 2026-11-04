# WP-24 — Multi-TUI: harness and model per pane (Claude Code + Cursor)

status: in-progress size: M
depends-on: 25, 30 (the seam and its file layout, both landed) blocks: —
brief-cost: 0 — the briefs do not change. One `orch.md`, one `worker.md`, both harnesses (M4).

**The spec lives in [issue #12](https://github.com/rubikron/fleetor/issues/12), not here.** This doc
is the roadmap's pointer to it: what it is, what was decided, what was measured, and the
anchors an implementing session needs. The issue carries the problem statement, 45 user
stories, the implementation and testing decisions, and the out-of-scope list. The decision
trail — every option considered, every rejected alternative, and what would reverse each
choice — is `decisions.md` **M1–M28**.

## Outcome

The harness stops being an assumption and becomes a choice the operator makes at the start
gate, beside the target repository. A fleet can be all Claude Code, all Cursor, or mixed,
and the panes collaborate through the same nine verbs regardless. Underneath, the thing
that made this hard becomes the thing that makes the next harness easy: a `Harness` seam
with fourteen named checkpoints and a conformance suite, so adding a third TUI is a
checklist rather than an archaeology project.

## What was measured, not assumed

Design ran spike-first (`building.md` §4) against the real `cursor-agent`
(`2026.09.02-c22c1a3`). Findings that shaped the spec, each reproduced in the issue:

- **A rules file is a working system-prompt substitute** — an `alwaysApply` `.mdc` in the
  pane's own cwd reached the model with nothing on screen. The same rule on an
  `--add-dir` root did **not** reach it, so the brief must live in the pane's worktree.
- **`--force` disables the sandbox.** It was recommended and withdrawn inside one session:
  the identical probe wrote outside the workspace with it and was refused without it.
- **The sandbox covers the agent's own file-writing tool**, not only shell — so cursor's
  missing pre-edit hook event costs nothing, and a cursor worker is genuinely fenced.
- **Delivery is unchanged.** A bracketed paste + CR into a real `cursor-agent` pty, with
  this app's own env, submitted and produced a reply carrying a token that appeared
  nowhere in the pasted bytes.
- **Auth follows `HOME`**, so the Fence's private home logs a cursor pane out unless the
  credential is seeded (M8).
- **The socket refusal was policy, not architecture.** A sandboxed pane cannot dial a unix
  socket under the default policy — measured outside *and inside* the worktree — but the
  sandbox's network-access key selects between `(allow network-outbound (remote ip
  "localhost:*"))` and unrestricted `(allow network-outbound)`, and the unrestricted form
  lifts it. **This deleted a whole phase** (M28): no bridge, no second transport, no split
  of the receipt verb.

## Current state (2026-11-02)

**Re-scoped by C19; the September map below this line is gone.** WP-25 built the `Harness`
seam and registered codex through it; WP-30 gave each harness its own file. So this package
is now: write `placement/cursor.rs` answering all fifteen checkpoints and its needles, add
it to `REGISTERED` in `harness.rs`, and let the conformance suite and the literals tripwire
cover it. `claude_code.rs` and `codex.rs` are the templates.

The M-series was measured on `2026.09.02-c22c1a3`. Everything is being re-measured on
`2026.10.22-27dcf5f` in [`cursor-spike-notes.md`](../notes/cursor-spike-notes.md). Already
changed: M8 does not hold (the login is keychain-only), so D-128 runs a cursor worker on
the operator's real `HOME`, with `CURSOR_CONFIG_DIR` keeping its configuration separate.
D-129 contains it with the write guardrail as a Claude Code hook in the worktree; D-130
carries its brief as a worktree rule and keeps cursor off the orchestrator seat.

## Scope

**In:** harness + model per seat including orch (M1, M2); per-harness brief carrier (M5);
cursor auth on the operator's subscription (M8); layered preference snapshot (M9, M10);
the two sandbox keys FLEETOR owns (M18, M28); the `Harness` seam and conformance suite
(M23); gate login/plan gating (M17); gauge, archive, orphan sweep, project-key split
(M22, M24).

**Out:** mid-run harness switching; any cursor API-key path; harness choice for the
evaluator and the Critic; per-pane budgets; quota-exhaustion detection; transcript
normalization; a third harness; a frontend test runner. **Also out, deliberately:** the
CLI's identity and authorization model — pane identity is client-asserted and the one
authorization rule is client-side. Recorded in M27 as the operator's call, unconditional
now that no bridge is being built.

## Invariant guardrails

- **Tier 1.4 — nothing on the message path.** The hook bridge would have sat beside it and
  is not built (M28). The existing hub and CLI suites passing **unchanged** is the
  evidence, the way `write_guardrail.rs` asserts the same property for the guardrail.
- **Tier 1.7 — auto-approve is narrowed, not widened.** A cursor worker's sandbox refuses a
  superset of what the write guardrail refuses (M11, M12).
- **D-030/D-052 are amended, not ignored.** "The orchestrator is the operator's own
  `claude`" stops being an invariant once that seat can be another vendor; a
  `default (your login)` sentinel keeps today's behaviour reachable (M2).
- **D-042 holds.** One `orch.md`, one `worker.md`, both harnesses. Only the carrier varies.
- **M25 — the roster stays harness-free.** Workers see each other as equals; the operator's
  rail carries the fact instead.

## Phases

1. Zero-token spike: login, config relocation, trust, process tree. **Done** (D-128).
2. Paid spike, on the operator's word: brief, sandbox, socket, delivery, `/clear` and
   `/compact`, transcript, resume. **Done** (D-129, D-130).
3. `cursor.rs` with every checkpoint answered from phases 1–2, registered; conformance
   green over three harnesses. **Done**, worker seats only (D-130); the gauge reads
   unavailable until the SQLite chat store has a reader.
4. One live cursor worker completes a `fleet send` round trip. **Done** (live run, 2026-11-03,
   after the `.mdc` gained its `alwaysApply` header).
5. **TODO: a live baseline every new harness must pass**, written to
   `docs/notes/live-baseline.md` (**written**; Cursor partly run). One live worker, by hand:
   - It receives a message and answers with `fleet reply`.
   - It edits a file, runs the tests and commits on its own branch, with no prompts.
   - The guardrail refuses writes and deletes outside its worktree, both through the
     harness's own tools and through a shell redirect, and the refusals show in Activity.
   - The operator's `git status` stays clean.
   - The pane carries no vendor credentials in `env`.
   - It uses the other `fleet` verbs a worker has, and the orchestrator sees them.
   - It survives `/clear`, `/compact` and a restart that resumes the same session.
   - A message sent before the composer paints still arrives.
   - The worker's card shows a context reading, not `ctx unavailable`. Cursor fails this
     until its SQLite chat store has a reader.
   Later, consider turning it into an opt-in test that spends paid turns.

## Session prompt

```
Read docs/roadmap/24-multi-tui.md, docs/notes/cursor-spike-notes.md and decisions D-128,
then placement/harness.rs (the interface) and placement/codex.rs (the closest template).
Continue from the first phase not marked done. Phase 2 spends the operator's Cursor plan:
ask before running it. Never pass --force or --yolo (M11).
```

## Session exit checklist

- [ ] `decisions.md` entry for every Tier 2 default that moved (cite M-number and new
      D-number in the commit subject).
- [ ] No verb was added or changed — if that stops being true, both prompt files + `VERBS`
      + the clap enum + pinned tests move in one commit.
- [ ] Spike notes committed to `docs/notes/`, version-stamped (the vendor's build id, not
      just a date — every measurement here is against `2026.09.02-c22c1a3`), with a
      `docs/README.md` index row.
- [ ] The hub and CLI suites pass **unchanged** — that is the Tier 1.4 evidence, not a
      formality.
- [ ] This doc: status → landed, "How it landed" appended.
- [ ] `00-index.md` status column updated — the last act of the session.
