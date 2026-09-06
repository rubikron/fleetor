# WP-24 — Multi-TUI: harness and model per pane (Claude Code + Cursor)

status: not-started size: L
depends-on: 20 (placement, landed) blocks: —
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

## Current state (verified 2026-09-05 — do not re-explore)

Where the fourteen checkpoints are today. 61 of the ~79 Claude-Code-specific references
are in one file, which is why the seam belongs in `placement` (M23).

- `src-tauri/src/placement/spawn.rs:116` — `orch_command_with`; `:286` `worker_command_with`;
  `:367` `base_command_with` (the literal `claude`); `:543` `seed_config_dir`.
- `src-tauri/src/placement/mod.rs:285` — `Host`, where harness discovery belongs (login,
  plan, model list) so `place` keeps its "nothing reads the process" rule.
- `src-tauri/src/guardrail.rs` — the `PreToolUse` half, 18 references.
- `src-tauri/src/orphans.rs:51` — `sweep_at_named(path, "claude")`. **A live bug the moment
  a cursor pane exists**: no cursor process matches, so every crashed cursor pane leaks.
  Its replacement must catch a Node tree without ever matching a bare interpreter.
- `src-tauri/src/runs.rs:226` — `harvest_transcripts`, Claude-Code-only. A cursor pane
  contributes **zero** transcripts and `runs.rs:64` documents `0` as ordinary.
- `src-tauri/src/context_gauge.rs:50` — `WORKER_WINDOW_TOKENS`, the constant exported to
  the harness so display and bookkeeping cannot disagree (D-054).
- `spawn::project_key` — Claude Code's **own** canonicalization, shared by the trust flag
  (`deliver.rs`) and the gauge. Cursor keys projects differently; it can no longer be one
  function.
- `ui/src/components/StartGate.tsx` — the spend gate the pickers join (M15).

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

1. The `Harness` trait + `HarnessSpec`, inside `placement`, with the conformance suite over
   the fourteen checkpoints — Claude Code first, green before Cursor exists.
2. Per-harness configuration seeding: the `.mdc` brief, the layered `~/.cursor` snapshot,
   the two sandbox keys, per-worktree git exclusion.
3. The gate: login/plan probe on `Host`, the pickers, the per-harness cost line.
4. The integration tail: gauge reader, transcript harvest, orphan sweep names, project-key
   split.

## Session prompt

```
Read docs/roadmap/24-multi-tui.md in full, then the spec at
https://github.com/rubikron/fleetor/issues/12, then decisions.md M1–M28 (search "WP-22 —
multi-TUI"; the M-series is the interview trail). Then building.md §1 and §9.

Implement phase 1 only: the Harness trait and HarnessSpec inside placement, with the
conformance suite over the fourteen checkpoints, Claude Code as the sole registered
harness. No cursor code in this phase — the suite must be green with one harness before a
second exists, or it is not a suite, it is a description of cursor.

Spike-first for anything the spec marks unverified (building.md §4). Two things it marks
so: whether an alwaysApply .mdc survives /clear, and cursor's transcript format. Neither
is phase 1's problem; do not start them here.

Exit: the checklist at the bottom of the roadmap doc.
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
