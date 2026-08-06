# Overnight questions — decisions taken unattended that a human may want to overrule

Written by the overnight sessions. Nothing here blocked a package: each item was
decided, recorded in `decisions.md`, and shipped. They are collected because each
is either the closest a package came to a tripwire, or a number that moved
further than its doc predicted — the two kinds of thing a morning reviewer would
want surfaced rather than buried in a D-entry.

Each item states what would reverse it, so overruling costs one small change
rather than a re-litigation.

---

## Q-1 (WP-05, D-047) — `fleet task update` refuses an id that is not on the board. Is that a gate?

**What was built.** `Hub::task`'s `Update` arm replays the board and refuses when
the named block is not on it: `no task "task-typo" is on the board — 'fleet task
list' shows the ids`. Nothing is written.

**Why it was judged not a tripwire.** WP-05's tripwire list bans gates, enforced
state-machine transitions, supervisors acting on the board, routing by task
state, blocking claims, and `Assign`-like ops. This is none of those: it
constrains nothing about *which* status may follow which (any may follow any,
tested backwards), it permits and orders nothing, and it never touches a
delivery. It is referential validation at a system boundary — the same class as
the hub's existing `no pane worker-9 is running`. The reason it is worth having:
a model that typos an id would otherwise get a success, and its claim would sit
in the log attached to nothing, unread by the board and by WP-06's review.

**Why it is flagged anyway.** It is the only place in the package where anything
reads task state before deciding to do something. That sentence is exactly the
shape of the thing the package exists to avoid, even though what it decides is
only "is this a claim about anything at all."

**What reverses it.** Delete the existence check in the `Update` arm and let the
event be appended regardless; `task::board` already skips a claim with no post
(tested — it is the L9 "skip the row, keep the replay" posture), so the board
stays correct either way. The cost of reversing is that a typo'd id becomes
silent. One `if` block, one test
(`an_update_to_a_block_that_is_not_there_is_refused_and_writes_nothing`).

---

## Q-2 (WP-05, D-047) — the prompt budget overran ~4× for the second package running

**What happened.** WP-05 budgeted ~150 tokens across both prompt files and spent
**+318 orch / +248 worker**. WP-03 budgeted ~120 and spent +248 / +228. Same
multiple, same reason: a verb costs what its *rules* cost, not what its syntax
costs. Rendered totals are now **orch 2,228 / worker 1,591**, against the 1,485
tokens of Claude Code guidance that `--system-prompt` stopped sending (D-043).

**Why it was spent rather than trimmed further.** A tightening pass ran first
(−6 orch, −27 worker). What is left is: seven flag names, without which the
schema is unreachable; four status words, without which an invented
`in-progress` is a refusal; "write criteria that could fail" with its
counter-example; and the diary-not-dispatcher clause with its consequence. Every
one of those is pinned by a test, because shortening it turns it into advice. The
honest options were spend it or ship a verb nobody uses correctly.

**What a reviewer may want to do.** WP-09 was scoped as an audit of the sum. Two
overruns in a row make it a **prune** — and the decision of *what* to cut across
four packages' prose is an operator's call about what the fleet is for, not a
builder's. WP-06 and WP-07 have not written their prose yet and should be told
the headroom is already gone.

---

## Q-3 (WP-06, D-050) — the prompt budget overran a third time, after Q-2 said the headroom was gone

**What happened.** WP-06 budgeted ~150 tokens across both files and spent
**+314 orch / +376 worker**. Rendered totals are now **orch 2,542 / worker
1,967**. Q-2 was written specifically to warn 06 and 07 that there was no
headroom left; 06 read it, tightened first, and spent the tokens anyway.

**Why it was spent.** The tightening pass ran *before* the number was recorded
and recovered **21 orch / 25 worker** — about 6% of the addition. That is the
finding: there is no fat to cut. Unlike a verb with a syntax, `fleet done` has a
*protocol* — commit, run the check, read the receipt, ask the named reviewer,
review from your own worktree with the right diff form, merge to integration and
never trunk. Each step is a separate failure if unstated, and several are pinned
as literals because a shortened version becomes advice: the three-dot diff
(two-dot renders a peer's work as your own deletion — reproduced live), "never
in the exit code" (or a failing check reads as a failed delivery and the worker
resends), "**never into trunk**" (Tier 1.1), and "evidence, not a verdict".

**What a reviewer may want to do.** This is the third consecutive overrun and the
first one that was *pre-warned*, which makes it a data point about the estimates
rather than about the packages: a `brief-cost` line written before the prose has
now been wrong by 3–5× four times running. Two calls are the operator's, not a
builder's:

1. **Is 2,542 / 1,967 actually a problem?** Nobody has measured a cost or a
   behaviour change from it — only a number going up. Against the 1,485 tokens of
   Claude Code guidance `--system-prompt` stopped sending (D-043), orch is now
   ~1,050 past parity. WP-09's live shakedown is the first evidence either way,
   and it may well be that the answer is "fine, delete the budget."
2. **If it is a problem, what gets cut is a question about what the fleet is
   for.** Every candidate is one package's core protocol. Cutting review prose
   buys ~150 worker tokens and gives up peer review; cutting the task-block flag
   names makes the board unreachable. A builder choosing between those is
   choosing product scope.

**What reverses it.** Nothing needs reversing to ship — this is a number, not a
change. If the operator wants WP-06 smaller, the cheapest ~120 worker tokens are
the *reasons* attached to the review rules (keep the commands, drop the
explanations), at the known cost that D-048 names: the rule without the reason is
what the next rewrite deletes first.
