# Blackboard requirements — index

The Blackboard vision, split into nine session-sized work packages. The vision in one line: FLEETOR stops being only a messaging spine and becomes a collaboration system that fosters communication, teamwork, and novel ideas — vision-first problem understanding, task blocks with real performance criteria, context-aware execution, and peer review.

**How to use this directory.** Pick the lowest-numbered `not-started` package whose `depends-on` are all `landed`. Open a fresh Claude Code session against this repo. Paste the package's **Session prompt** section verbatim. Each doc is self-contained: the implementing session needs `building.md` plus the one doc — not the 103 KB `decisions.md`.

## Operator decisions on record (2026-08-06)

1. **Panes own the entire system prompt.** Switch `--append-system-prompt` → `--system-prompt` so the prompt files *replace* Claude Code's default system prompt rather than appending to it. Owned by WP-02, spike-first (interactive-mode support must be measured, and the replacement must carry whatever CC-default behavior the panes still need). Sanctioned by Tier 1.2 — `building.md:16` places "system prompt" in harness territory.
2. **The Fence is in scope** (WP-08) even though it wasn't in the original dump — the command channel, receipts, and merge flow raise the stakes.
3. **Worktrees stay.** Peer review happens from each worker's own worktree via the shared git object DB; orch merges reviewed branches to `fleet/integration`, never trunk (WP-06).
4. **Task blocks ship as the full package** — verb + event + Tasks UI view in one session (WP-05).

## The packages

| WP | Doc | Name | Depends on | Size | Status |
|----|-----|------|-----------|------|--------|
| 01 | `01-land-context-management.md` | Land `feat/context-management` (prompts-as-files) | — | M | **landed** |
| 02 | `02-vision-culture-briefs.md` | Own the system prompt: vision & culture | 01 | L | **landed** |
| 03 | `03-command-channel.md` | `fleet cmd` — `/clear` + `/compact` with `--why` | 01 | L | **landed** |
| 04 | `04-context-visibility.md` | Context visibility (the Loadout counter + gauge) | 01 | M | **landed** |
| 05 | `05-task-blocks.md` | Task blocks — the blackboard record + Tasks view | 01, 02 (soft) | L | not-started |
| 06 | `06-receipts-review-merge.md` | Receipts, peer review, and the merge step | 01, 05 | L | not-started |
| 07 | `07-operator-participant.md` | Operator as participant | 01 | M | not-started |
| 08 | `08-the-fence.md` | The Fence — private worker HOME | 01 | S/M | not-started |
| 09 | `09-brief-budget-shakedown.md` | Prompt budget + live shakedown | all | S/M + live spend | not-started |

## Dependency graph

```
WP-01 ─┬─→ WP-02 ─→ WP-05 ─→ WP-06 ─┐
       ├─→ WP-03 ───────────────────┤
       ├─→ WP-04 ───────────────────┼─→ WP-09
       ├─→ WP-07 ───────────────────┤
       └─→ WP-08 ───────────────────┘
```

- **Serial spine** (mirrors the session-cycle phases in the vision): 01 → 02 → 05 → 06 → 09.
- **Parallel lanes** after 01: 03, 04, 07, 08 are mutually independent.
- **Recommended solo order:** 01, 02, 03, 04, 05, 06, 07, 08, 09.

## Contention warning — read before running packages in parallel

- WP-03, WP-05, WP-06, WP-07 all edit `prompts/orch.md` + `prompts/worker.md`, `VERBS`, and the clap enum. The validation WP-01 lands **refuses** a prompt that fails to teach every verb, so a verb and its prompt text must move in one commit. Run **at most one verb-adding package (03, 05, 06) at a time**.
- Whichever of WP-06 / WP-08 lands second must re-validate worker commits: a private HOME drops the global `user.name`/`user.email` (WP-08 seeds a gitconfig for exactly this).

## Reconciliation with the autonomy-designs build order

The "FLEETOR — Autonomy Designs" doc (landed by WP-01, at [`../plans/`](../plans/)) ordered: Fence → Receipts → Shakedown. No conflict: **Fence** is WP-08 intact; **Receipts** is folded into WP-06 (receipts are the evidence the review phase examines — they were never separable); **Shakedown-as-code** moves *after* WP-09, whose manual live pass produces the data that justifies or kills building it.

Deferred, with reasons — do not resurrect without new evidence:

- **Shakedown-as-code, Double Entry** — observer reports; wait for WP-09's manual findings.
- **Callsign's analysis graph** — fails its own weak-model threshold; only the auto reply-tag survives, as a WP-06 stretch item.
- **Carryover** (handover notes) — its own verdict was "measure restart frequency first"; WP-04 adds the one-line restart counter if trivial, and WP-03's after-`/clear` re-brief rule covers the near-term need.
- **Bake-Off, Cheap Twin, Hearsay, Posture Ladder** — rejected upstream in the adversarial review; reasons recorded in the designs doc.

## Standing tensions (every session should know these)

1. **The system prompt is a shared budget, and it is already over.** Four packages append prompt text; each doc carries a `brief-cost` line and WP-09 audits the sum. With the `--system-prompt` switch landed (WP-02, D-043), the budget is the *whole* prompt, baseline included. **After WP-03 it is orch 1,885 tokens / worker 1,343** (DeepSeek Flash tokenizer, `examples/system-prompt-spike/count.py`), up from WP-02's 1,637 / 1,115. WP-03 budgeted ~120 tokens across both files and spent **+248 / +228** — about four times its estimate, recorded in D-045 rather than absorbed quietly. Two lessons for 05, 06 and 07, which have not written their prose yet: a `brief-cost` line written before the prose is a guess, and a verb costs what its *rules* cost, not what its syntax costs. Against the 1,485 tokens of CC guidance that stopped being sent, orch is now roughly at parity and a worker still starts smaller than before WP-02 — but the headroom that made the overrun affordable is spent.
2. **"Better than human work" vs DeepSeek-Flash workers.** The worker model is a Tier 2 default. If the shakedown (WP-09) shows Flash can't carry peer review or task-criteria judgment, swap the model with a three-line `decisions.md` entry — bring the evidence, not the intuition.

## Conventions (all packages)

Feature branches only, never trunk (Tier 1.1). Lowercase prose-y commit messages in repo style, decisions cited by ID. Tier 2 changes get a three-line `decisions.md` entry. Update this index's Status column as the last act of every session.
