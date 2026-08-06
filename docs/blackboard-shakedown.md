# Blackboard shakedown — live findings

The WP-09 live half: the full loop run against the seeded testbed, findings
recorded as outcomes, each closed one of three ways — **fine** / **new
requirement doc** / **prompt amendment**. Run started 2026-08-06, CC 2.1.223,
orch on the operator's own model, workers on DeepSeek Flash with a 500k
window (D-054).

## Finding 1 — the vision-partner clause lost to a small concrete task

**Observed.** Operator pasted the probe `Add a --version flag to the CLI.`
Orch did not ask what the flag was *for* — it asked how to implement it.
Verified against the live process: the correct build was running and the
rendered `--system-prompt` contained both vision clauses ("bigger picture",
"smaller than it could be"), so this was the prompt losing, not the prompt
missing.

**Read.** Two causes, one fixable here. (a) The clause gated on "before you
decompose anything into work" — a one-line task didn't read as decomposition,
so orch fell through to its clarify-the-task instinct, which is question-
shaped but at implementation altitude instead of purpose altitude. (b) A
standing confounder: orch is the operator's own `claude`, so it also carries
the operator's global rule set (plan-first, requirements-capture), which
pushes exactly that posture. (b) is the operator's own context and stays.

**Closed: prompt amendment.** `prompts/orch.md` clause 1 now names the trap:
*"A small, concrete request is the trigger for this question, not an
exemption from it — your first question is about purpose, never
implementation."* Cost +46 tokens (orch 2,645 → 2,691, cap 2,800 — fits).
Pinned-literal tests and validation green. D-055.

**Re-test.** Restart the fleet (the prompt is compiled in via `include_str!`,
so the dev rebuild must run), paste the same probe, expect a purpose
question.
