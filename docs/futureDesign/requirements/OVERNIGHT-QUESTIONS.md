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
