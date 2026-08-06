# Prompt budget — the operator's morning read

> ## ⚠ ARCHIVED — overtaken by events (2026-08-06)
>
> Every headline number here is stale: the budget moved to orch 3,366 / cap 3,500 (D-056) and the
> window to 500,000 tokens (D-054). Cut candidate #1 was not declined but **inverted** — D-056
> removed the distillation and put the operator's full tenets essay into the orch brief. The
> measurement method (`examples/system-prompt-spike/count.py`, write-measure-revert) is still the
> way to cost a prompt change. Kept as the menu the operator was handed. Do not edit below this
> banner.

Written by WP-09 (budget half), overnight. This is the decision the ledger in
`decisions.md` D-053 kept deferring: **is 2,645 / 2,099 a problem, and if so,
what gets cut?** Every number below is a real measurement from
`examples/system-prompt-spike/count.py` (DeepSeek Flash tokenizer) — each
candidate was actually written into `prompts/*.md`, measured, and reverted,
not estimated. None of it is applied. This file is the menu; the cutting is
the operator's.

## Where things stand

| | orch | worker |
|---|---|---|
| Current, post-WP-09-prune | **2,645** tokens | **2,099** tokens |
| Share of `WORKER_WINDOW_TOKENS` (128,000, D-046) | 2.07% | 1.64% |
| vs. the 1,485-token CC guidance block `--system-prompt` stopped sending (D-043) | +1,160 (1.78×) | +614 (1.41×) |
| Tier 2 cap, this package (D-053) | 2,800 | 2,200 |

**Is it a problem in measurable terms?** No behavioral cost has been observed
from either number — no worker has been seen mishandling, truncating, or
ignoring brief content because of size. What *is* measurable: the whole
system prompt, verbs and protocol included, costs under 2.1% of a worker's
context window before a single message arrives. The "past CC parity"
comparison is a reference point (what a generic coding agent's guidance used
to cost), not a demonstrated regression — this fleet's brief teaches a verb
list, a task board and a review protocol a generic brief never had to. The
live shakedown (this package's other half, operator-gated) is the first place
a real cost — a Flash worker mishandling a rule, or visibly running short of
room — could show up. Until then this is a number to watch, not a fire.

## The cut candidates

Each was written into the actual template, measured with `count.py`, then
reverted — so the token figure is exact, not a guess. Ranked by savings.

### 1. Distill the vision tenets to their bolded phrases only
**Saves ~225 orch tokens** (worker untouched — `vision_tenets` is orch-only).
Cuts every one-sentence elaboration under each of the six tenets
(`prompts/vision-tenets.md`), leaving only `**Write the vision down.**`,
`**A vision is as much what it is not.**`, etc. **What degrades:** the
orchestrator keeps the six operative phrases (still validated —
`the_orch_brief_distills_the_tenets_rather_than_quoting_them` only checks the
bolded lines) but loses the one sentence of *why* under each — the exact
thing D-048 named as "the rule without the reason is what the next rewrite
deletes first." A weak model given only slogans is more likely to treat them
as decoration than as filters that actually change a decision. **Owner:**
WP-02 / WP-04 (D-044). **Largest single lever in this menu** — over 8% of the
whole orch prompt in one cut.

### 2. Drop the security/refusal paragraph from `scaffolding.md`
**Saves ~58 tokens per brief (~116 combined)** — shared fragment, so it costs
the same in both. Cuts "Assist with authorized security testing... refuse
destructive techniques, denial of service, mass targeting, supply-chain
compromise, and detection evasion," keeping only the they/them pronoun
default. **What degrades:** the fleet's own written record of its security
posture disappears from the pane's system prompt. The underlying model's own
training almost certainly refuses the same requests regardless of what the
prompt says — but D-043 flagged this fragment as one of five *load-bearing*
restatements of what CC's own prompt used to supply, specifically because
`--system-prompt` **replaces** rather than appends, and this project doesn't
have independent evidence the base-model default is enough on its own once
CC's own guidance is gone. **Owner:** WP-02 (D-043, `scaffolding.md`). **The
riskiest candidate here** — the only one touching anything security-adjacent,
flagged accordingly rather than recommended.

### 3. Cut the worked `fleet cmd` example from `orch.md`
**Saves ~39 orch tokens.** Drops the concrete `fleet cmd 2 "/compact keep the
parser design..." --why "worker-2 finished the parser..."` example, leaving
the rule and the syntax pattern (`fleet cmd <pane|self> "<slash command>"
--why "<reason>"`) intact — `both_briefs_teach_the_command_verb_with_its_mandatory_why`
still passes, it never checks for the worked example. **What degrades:** Q-2's
own finding was that a weak model uses a verb correctly only when it's shown
*what a good invocation looks like*, not just the shape of one. Losing the
example is losing the thing that taught DeepSeek Flash what a real
`--why` string reads like, versus a restated command. **Owner:** WP-03
(D-045).

### 4. Trim the review-and-receipt rationale clauses in `worker.md`
**Saves ~46 worker tokens** (measured; Q-3's own guess at WP-06 time was
"~120," another data point for how unreliable pre-prose estimates are).
Drops five small non-pinned reason-clauses from the "Finishing a block" and
"Reviewing a peer" sections: "and your reviewer reads that commit,"; "so a
peer's branch is readable from here with no fetching"; "as though they had
deleted your work"; "and never edit their files"; "and answer plainly — met,
or specifically what is not." Every pinned rule survives untouched (three-dot
diff, `cd` boundary, criteria-not-taste). **What degrades:** each clause was
the *why* attached to a rule that would otherwise read as an arbitrary
restriction — exactly D-048's warning again. A worker told "never `cd` into a
peer's worktree" with no reason is one rewrite away from a worker that no
longer sees why not to. **Owner:** WP-06 (D-050).

### 5. Drop the optional task-linking flags from the `fleet task post` example
**Saves ~24 orch tokens.** Trims `[--instructions "…"] [--parent <task-id>]
[--converges-on <task-id>]` from the shown syntax, leaving the four required
flags. **What degrades:** a real capability loss, not just prose — the
orchestrator loses the *syntax* for attaching free-text instructions or
building a task tree (parent/converges-on links). Since nothing else in the
brief teaches those flag names, an orchestrator that needs them would have to
guess or discover them via `--help`. **Owner:** WP-05 (D-047). **Smallest
saving here for real capability lost** — probably the last one to take.

### 6. Drop "Say back what you are changing because of it" from the operator-authority clause
**Saves ~10 worker tokens.** The one sentence in `worker.md`'s operator
clause beyond what `the_worker_brief_gives_the_operators_word_final_authority`
pins. **What degrades:** the instruction to *acknowledge* a course-change the
operator ordered — without it, the authority rule ("their word is final —
outranks `orch`, your block's criteria...") still stands, but nothing tells
the worker to say back what changed, so a silent compliance is possible and
unremarked in the log. **Owner:** WP-07 (D-051). Cheapest and lowest-value
cut in the menu — ~0.5% of the worker prompt for a real (if small) loss of
transparency.

## Totals if taken together

Candidates are independent (none overlaps another's text) and additive.
Per-candidate orch/worker savings: #1 = 225/0, #2 = 58/58, #3 = 39/0,
#4 = 0/46, #5 = 24/0, #6 = 0/10.

| Combination | orch saved | worker saved | orch final | worker final |
|---|---|---|---|---|
| All six | 346 | 114 | 2,299 | 1,985 |
| Everything except #2 (the security paragraph — the one flagged risky) | 288 | 56 | 2,357 | 2,043 |
| Recommended, below | 63 | 0 | 2,582 | 2,099 |

Taking everything drops orch to **2,299** (13.1% below today's 2,645, 17.9%
below the Tier 2 cap of 2,800) and worker to **1,985** (5.4% below today's
2,099, 9.8% below the cap of 2,200). That is real headroom for a future
addition without another overrun — at the cost of every "why" this menu
lists.

## A recommendation, labeled as one

If forced to pick: **take #3 and #5, leave the rest.** Both are small,
concrete, syntax-level cuts with no rationale attached to lose — #3 trades a
worked example for 39 orch tokens (a real teaching loss, but the smallest of
the "loses a why" candidates), and #5 trades an uncommonly-used flag pair for
24 orch tokens (a real capability loss, but one nothing in the roadmap
currently exercises — no landed package posts a `--parent` or
`--converges-on` block). Combined this is a modest 63-token orch cut (worker
untouched) — small on purpose. Skip #1, #2, #4 and #6: they all cut a stated
*reason*, and D-048 already named what that costs on the next rewrite. The
ledger's own finding — every overrun came from a verb's *rules*, not its
syntax — argues for leaving rules and their reasons alone and cutting syntax
sugar first. This is one read of the trade-offs, not the only defensible
one; the numbers above are what each choice actually costs either way.
