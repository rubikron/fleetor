# WP-09 — Prompt budget + live shakedown

status: budget half landed (overnight, D-053) — shakedown half not-started, pending operator size: S/M + real spend (live half requires the operator)
depends-on: all (02, 03, 05, 06, 07 for the budget; everything for the live run)
brief-cost: negative — this package prunes

## Outcome

Two halves:

1. **Budget (no live spend):** someone finally owns the prompt token budget the autonomy review said nobody owns. Measure both rendered prompts after 02/03/05/06/07 all added text (with the `--system-prompt` switch, the budget is the *whole* prompt). Set a cap as a Tier 2 constant, recorded in `decisions.md`. Prune to fit — the vision's own tenet: what no longer fits gets cut.
2. **Shakedown (operator present — real Opus + DeepSeek spend, named and approved first per `building.md` §9.5):** run the full loop live **once** — vision conversation → task blocks posted → assignment → execution with at least one `fleet cmd` `/compact` (why logged) → receipt → peer review → merge to `fleet/integration` — and write `docs/blackboard-shakedown.md`. This is the manual pass that produces the data to justify (or kill) Shakedown-as-code.

## Performance criteria

### Technical
- [x] Rendered orch + worker prompts measured (token estimate is fine; method stated); cap set; both prompts ≤ cap after pruning; every verb still validated. (`count.py`, DeepSeek Flash tokenizer; D-053; `the_baked_in_templates_validate` and every pinned-literal test in `brief.rs` green.)
- [x] The sum of the roadmap's `brief-cost` lines reconciled against the measurement — the audit catches drift. (D-053: 510 budgeted vs. 2,018 actual across WP-03–07, ~4.0× overall.)
- [ ] The live run's message/task/command log exported (the events DB is the evidence) and cited in the findings doc. (Shakedown half — not this session.)

### Semantic
- Findings are **outcomes, not intentions**: "worker-2 self-compacted once, unprompted; the why-string was junk" — not "self-compaction works."
- Every finding closes one of three ways: fine as-is; a new numbered requirement doc in this directory; or a prompt amendment (with its decisions entry).
- The two pre-measurements the autonomy verdict ordered are checked off here: restart frequency (WP-04's counter, if built) and the hand-run prompt comparison (this package's budget half *is* it).

## Invariant guardrails

- **`building.md` §9.5:** non-trivial live spend is named to the operator and approved before the run — the shakedown half does not start without the operator present.
- **Log outcomes (Tier 1.6)** governs the findings doc's voice.
- Pruning must not cut the pinned clauses: anti-amplification, the delivery contract, the authority bound.

## Current state (when this package starts)

- `brief-cost` ledger: 02 sets the baseline; 03 +~120; 04 +~30; 05 +~150; 06 +~150; 07 +~60 (estimates — measure, don't trust).
- The validation (`validate_orch`/`validate_worker`) is the floor: a pruned prompt that stops teaching a verb is refused at spawn.

## Scope

### In
Measurement + cap + pruning + decisions entry; the one live run; `docs/blackboard-shakedown.md`; follow-up requirement docs as findings demand.

### Out
Shakedown-as-code, Double Entry, any observer automation — those wait for this package's data.

## Session prompt

```
You are working in /Users/bubblyducks/harness/fleetor. Read
docs/futureDesign/requirements/09-brief-budget-shakedown.md in full, then
building.md §1, §9 (especially §9.5). Execute WP-09 in two halves. Budget
half: measure both rendered system prompts, reconcile against the
roadmap's brief-cost ledger, set the Tier 2 cap in decisions.md, prune to
fit without touching the pinned clauses. Shakedown half ONLY with the
operator present and spend approved: run the full Blackboard loop once
(vision → task blocks → execution with a logged /compact → receipt →
review → merge to fleet/integration) and write
docs/blackboard-shakedown.md with findings as outcomes, each closed as
fine / new requirement doc / prompt amendment. Finish with the session
exit checklist.
```

## Session exit checklist

- [x] Cap recorded; prompts measured and within it; validation green. (D-053: orch 2,645 ≤ 2,800, worker 2,099 ≤ 2,200; `cargo test --workspace`, `src-tauri` tests, `tsc --noEmit`, `vite build` all green.)
- [ ] Live-run log exported and cited (shakedown half — not this session; operator presence required per §9.5).
- [x] Findings filed as new NN docs where warranted. (`docs/archive/prompt-budget-menu.md` — the budget half's finding, six costed cut candidates, no code/prompt change applied beyond the conservative prune.)
- [x] `decisions.md` entries appended. (D-053.)
- [x] `00-index.md` statuses updated across the board. (WP-09 row and standing tension 1.)
