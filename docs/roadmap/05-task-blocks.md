# WP-05 — Task blocks: the blackboard record + Tasks view

status: **landed** (D-047) size: L
depends-on: 01, 02 (soft — the vision language the criteria reference) blocks: 06
brief-cost: budgeted +~150 tokens across both prompt files; **spent +318 orch / +248 worker** (measured, D-047)

**As landed.** `fleet task post|update|list` — one verb, one `Op::Task`, one new
`FleetEvent::Task`, no migration. `Hub::task` is the only arm that is not `async`
and never touches the app: the board is a fold over the event log
(`fleetor_core::task::board`), with no `tasks` table and no cached state. The
tripwire list held — nothing on it was built. The delivery-independence pin is
`crates/fleetor-server/tests/task_board.rs::a_send_is_byte_identical_whether_the_board_is_empty_or_full`,
and the message path's `git diff` is literally empty (`message.rs`, `command.rs`,
`deliver.rs`, `pty.rs`, `pane_messaging.rs`, `panes.rs` — zero changes).

## Outcome

The orchestrator can record the agreed decomposition — the big pizza cut into slices — as **task blocks**, and everyone (fleet and operator) sees the same board. Each block carries the vision's exact shape: **outcome** (what this execution enables), **performance criteria** (technical: checkable, command-shaped where possible; semantic: which part of the vision this serves), **assigned worker**, optional **detailed instructions**, and **tree links** (parent / converges-on) so streams of work can branch and converge. The board is a **passive record the orchestrator maintains** — a diary, not a dispatcher. Assignment still travels as an ordinary `fleet send`; nothing anywhere reads task state to permit, order, or refuse anything.

## Performance criteria

### Technical
- [ ] Exactly one new verb family: `fleet task post|update|list` (spends the verb budget once).
- [ ] New `FleetEvent::Task` appended through the normal single-writer store. Every task event is a **claim an agent made**, timestamped and attributed — outcomes, not intentions (Tier 1.6).
- [ ] Status is a descriptive enum (`planned|claimed|done|dropped`). `done` is an *unverified claim* — verification is WP-06's receipts and review. No code enforces transitions.
- [ ] Tree links are ids; cycles are tolerated (render flat with a warning) — never validated into a workflow engine.
- [ ] **The delivery path diff is empty.** Pinned test: a `fleet send` to a worker with zero tasks behaves byte-identically to one with ten — no code path consults task state before a delivery.
- [ ] Tasks view added as the fifth view (`Sidebar.tsx` currently: `fleet | messages | activity | settings`) without touching terminal mounting rules (`building.md` §7.5 — every terminal stays mounted; hidden = `.is-hidden`).
- [ ] `ui/src/fleet/types.ts` + feeds extended — the frontend renders unknown event kinds as nothing, so the TS mirror moves in the same session.
- [ ] Board state is replayed from events (same as messages) — no second table, no second source of truth.

### Semantic
- The task-block schema is the vision's shape **verbatim**: outcome / performance criteria (technical + semantic vision-link) / worker / instructions / tree position.
- Orch prompt teaches: decompose **only after** the vision is confirmed (WP-02's clause); post the blocks; then `fleet send` each worker its block with the detailed instructions — the send is the assignment, the board is the record.
- Worker prompt teaches: your task's criteria are the definition of done; check the situation fits them before claiming `done` (the claim gets verified by peers in WP-06).

## Invariant guardrails

**This package is the one most likely to regrow what D-030 deleted.** The old system's `Assign` op returned Ack while nothing ever ran; `event.rs:4-8` and `wire.rs:16` carry literal warnings that kept-just-in-case structures "are the seed of the ticket system growing back."

The board is a diary, not a dispatcher. Tripwires — wanting **any** of these means stop and file a Tier 1 question, not work around:

- No gates. No enforced state-machine transitions.
- No supervisor or poller acting on the board.
- No auto-routing or load-balancing by task state.
- No blocking "claim" semantics.
- No per-task worktrees.
- No `Assign`-like op that makes anything run.

## Current state (verified 2026-08-06 — do not re-explore)

- Events: `crates/fleetor-core/src/event.rs:20` — three variants, module doc `:4-8` explains the freeze. DB: one `events` table behind `Store` (a vestigial `budget` column in `migrations.rs` is from the *deleted* ticket system — a cautionary fossil, not a foundation).
- Wire: `crates/fleetor-core/src/wire.rs:54` `Op` (4 ops), `:78` `OpResult`; test pins wire tag == CLI verb. Hub dispatch: `hub.rs:136`.
- UI patterns to copy: `MessageFeed.tsx` (unbounded record) and `EventFeed.tsx` (bounded 300) — the Tasks view is a third sibling; view registry in `Sidebar.tsx` (`type View`), all views stay mounted in `App.tsx`.
- Verb/prompt coupling (post-WP-01): new verb ⇒ both prompt files + `VERBS` + clap + pinned tests move in one commit.
- Rejected alternative, recorded so it stays rejected: a markdown blackboard file maintained by orch — zero code, but workers' auto-approve is worktree-scoped so they couldn't write it, and it loses attribution and replay.

## Scope

### In
Event variant + DB kind + TS type + Tasks view + `fleet task` verb + hub arm + prompt text + tests (including the delivery-independence pin).

### Out
Everything in the tripwire list; task-driven notifications; editing enforcement (anyone may append an update event — the log shows who; ownership is social, not coded); receipts and review (WP-06).

## Design sketch & open questions

1. **CLI arg shape.** Recommended: flat flags — `fleet task post --outcome "…" --crit-t "cargo test -p x" --crit-s "serves <vision part>" --to 2 --parent t-3 [--instructions "…"]` — weak models emit flat flags far more reliably than JSON or heredocs.
2. **Task ids.** Recommended: reuse the `ids.rs` pattern (`msg-<epoch_ms>-<seq>` → `task-…`).
3. **`update` semantics.** Recommended: append-only updates (status, notes); anyone may post one; the event's `from` field is the accountability.
4. **`list` output.** Recommended: compact tree, one task per line, criteria on demand (`fleet task list --full`) — the reading model is a pane with finite context.

## Session prompt

```
You are working in /Users/bubblyducks/harness/fleetor. Read
docs/roadmap/05-task-blocks.md in full, then building.md
§1 and §9, and the module docs at crates/fleetor-core/src/event.rs and
wire.rs (the ticket-system-regrowth warnings). Execute WP-05: the fleet
task post|update|list verb, a FleetEvent::Task appended through the normal
store, and a read-only Tasks view as the fifth UI view — a diary, not a
dispatcher. The task-block schema is: outcome, performance criteria
(technical + semantic vision-link), assigned worker, optional
instructions, parent/converges-on links. Pin the delivery-independence
test (a send consults no task state). If your design wants a gate,
supervisor, auto-routing, blocking claims, or enforced transitions: stop
and ask — do not build it. Prompts/VERBS/clap/tests move in one commit.
Finish with the session exit checklist.
```

## Session exit checklist

- [ ] Full test matrix green; delivery-independence test pinned; message-path diff empty.
- [ ] `decisions.md` entry (the verb, the schema, events-only storage).
- [ ] Both prompt files + `VERBS` + clap + pinned tests in one commit.
- [ ] Tasks view mounted per §7.5 rules (no terminal unmounts).
- [ ] `00-index.md` status updated.
