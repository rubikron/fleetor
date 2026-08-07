# WP-13 — The done verb (`fleet handoff`)

status: landed size: M
depends-on: — blocks: 15
brief-cost: **+302 orch / +40 worker, measured** (a pre-prose guess of ~80 would have been 4.3× low — exactly D-053's ledger shape for a verb-adding package)

**The verb is spelled `handoff`, not `done`.** `fleet done <task-id> "<check>"` was
taken in WP-06 by a *worker* closing one block with a receipt, and this is the
orchestrator saying the whole mission is finished — a different act at a different
altitude. Two verbs whose names were near-synonyms would be the one drift this repo's
`accepted`/`recorded`/`delivered` discipline exists to prevent. The package keeps the
name the arc doc gave it ("the done verb"); the *word a pane types* is `handoff`.

---

## Outcome

`orch` gains a way to say the thing nothing in the product could previously mean: **I
believe the goal the operator confirmed is met, here is what the fleet built, and here
is how anyone could check it.** The operator reads it on the Activity feed as it
happens, and the archived run keeps the moment the orchestrator believed it was
finished — with its evidence and its named loose ends beside it. Both of those are
worth having on their own, before anything reacts to the verb.

## Performance criteria

### Technical

- [x] `fleet handoff --built "…" --evidence "…" [--evidence …] [--open …]` exists; `--built` and at least one `--evidence` are `required` in clap; `--open` is optional and repeatable.
- [x] It answers **`recorded`** and never `accepted` — no pty is written to.
- [x] `crates/fleetor-core/src/brief.rs`'s `VERBS`, the clap subcommand enum, the wire `Op`, both prompt files and every pinned-literal brief test move **in one commit**; `cargo test --workspace` is green at that commit and at no point between.
- [x] `FleetEvent::Handoff` and `ui/src/fleet/types.ts` move in the same commit, so the event renders rather than being silently invisible (building.md §4 point 3).
- [x] A handoff sends the app **zero** `AppCommand`s, and a `fleet send` is byte-identical before and after one (`crates/fleetor-server/tests/handoff.rs`).
- [x] `cargo test --workspace`, `cargo test --manifest-path src-tauri/Cargo.toml`, `npx tsc --noEmit`, `npx vite build` all green.
- [x] `python3 examples/system-prompt-spike/count.py` re-run; both numbers stated, and the orch cap restated in `decisions.md` rather than silently edited (D-053's reversal clause).

### Semantic

- [x] **The veil holds.** Nothing in any word a pane is told names an evaluator, an assessment, a retro or a rubric. The prose frames the verb as a report to the operator, which is the whole truth as far as the fleet is concerned. Pinned by `no_pane_is_briefed_about_the_evaluator`.
- [x] The two altitudes are separated *in both briefs*: `orch` is told this is not `fleet done`, a worker is told `handoff` is not theirs. Neither pane spends a turn discovering it reached for the wrong verb.
- [x] A handoff cannot be written without evidence, for the reason a task block cannot be posted without a criterion: a claim with nothing checkable behind it cannot be argued with.
- [x] Nothing anywhere reads a handoff back. The fleet behaves identically before and after one.

## Invariant guardrails

**Tier 1.4 — nothing in the message path.** This verb is not on the path between `fleet
send` and a pty, and the design is what keeps it off rather than a promise to stay off.
`Hub::handoff` is **not `async` and never touches `self.app`** — the same signature-level
tell `Hub::task` (D-047) and `Hub::record` (D-051) carry. It cannot delay, refuse,
reorder, drop or alter anything, because it never learns that anything is being
delivered. `crates/fleetor-server/tests/handoff.rs` counts that rather than asserting it.

**Tier 1.5 — the outcome word is `recorded`, chosen deliberately.** `accepted` means
bytes reached a live pty; a handoff has no pty to reach, and D-051 records that
stretching `accepted` to cover a record-only op is exactly the dilution the third word
exists to prevent. `delivered` is never rendered by anything. So `recorded` — and it
reuses `OpResult::Recorded { record_id }` rather than minting a fourth variant, because
D-051's argument is that a second spelling of "entered the log, no pty exists" is drift,
not precision. Three callers now share the one word: a task claim, a message to
`operator`, and this.

**Tier 1.6 — the log records outcomes, not intentions.** The event is appended after
`Handoff::new` has accepted the content, and it is attributed (`from`) — a claim
somebody made, never a state the system inferred. A failed append **fails the op**, the
same asymmetry a task post and a message to the operator carry: here the log *is* the
deliverable, so answering `recorded` when nothing was written would be L3's lie.

**`task.rs`'s tripwire list, applied to a sixth `FleetEvent` variant.** The bar is that
nothing reads the event back to permit, order or refuse anything. It is cleared the same
way `Task` clears it — and the day something branches on "has the goal been declared
met", what has grown back is a delivery path that knows whether the mission is over.
That is why the independence test is in the package and not left for later.

**The veil (WP-12).** `orch` must not learn that anything reacts to this verb. That is a
Tier 1-class constraint for the arc: an orchestrator told its handoff will be assessed
writes the handoff for the assessment, and the run stops being evidence of how the fleet
actually works. Handled as a property of *which files contain which words*, extending
the mechanism WP-16 already built.

## Current state (verified 2026-08-07 — do not re-explore)

Anchors verified in the tree this package lands. The first block is what the package
touched; the second is the machinery it fitted into and did not change.

**Added or changed by WP-13:**

| Anchor | What is there |
|---|---|
| `crates/fleetor-core/src/handoff.rs:1` | The contract: `Handoff::new` validates, `into_event` logs. No I/O, no opinion about whether the goal *is* met |
| `crates/fleetor-core/src/event.rs:111` | `FleetEvent::Handoff { id, from, built, evidence, open }` — no `accepted` field, because no pty was written to |
| `crates/fleetor-core/src/wire.rs:102` | `Op::Handoff { built, evidence, open }` — the second op that reaches the store and never the app |
| `crates/fleetor-core/src/brief.rs:42` | `VERBS`, now nine, with `handoff` between `done` and `roster` — the clap enum's order |
| `crates/fleetor-core/src/brief.rs:776` | `the_orch_brief_hands_the_work_back_only_against_the_confirmed_vision` — the pinned literals |
| `crates/fleetor-core/src/brief.rs:798` | `both_briefs_separate_closing_a_block_from_handing_the_work_back` — the two altitudes |
| `crates/fleetor-cli/src/main.rs:139` | The clap subcommand; `--built` and `--evidence` required, `--open` repeatable and optional |
| `crates/fleetor-cli/src/main.rs:313` | `handoff_op` — the one local check: a worker is refused and told `fleet done` instead |
| `crates/fleetor-server/src/hub.rs:190` | `Hub::handoff` — not `async`, never touches `self.app` |
| `crates/fleetor-server/tests/handoff.rs:116` | The independence pin: zero asks, and a byte-identical send either side of a handoff |
| `ui/src/fleet/types.ts:149` | The TS mirror — moved in the same commit or the event renders as nothing |
| `ui/src/components/EventFeed.tsx:84` | The Activity row: coral, railed, evidence printed in full |
| `prompts/orch.md:96` | "When the goal is met" — the section that teaches the verb |
| `prompts/worker.md:50` | The one worker line: this verb is `orch`'s, yours is `fleet done` |
| `src-tauri/tests/dev_mode.rs:115` | `no_pane_is_briefed_about_the_evaluator` — the veil, extended from the mode to the thing behind it |

**Unchanged machinery this fitted into:**

| Anchor | Why it mattered |
|---|---|
| `crates/fleetor-core/src/brief.rs:176` | `require_verbs` — a rendered brief that omits any verb is **refused**. This is the contention rule's whole mechanism |
| `crates/fleetor-cli/src/main.rs:561` | `the_subcommands_are_exactly_the_verbs_the_briefs_teach` — clap pinned to `VERBS`, order included |
| `crates/fleetor-core/src/task.rs:1` | The tripwire list a new `FleetEvent` variant is measured against |
| `crates/fleetor-core/src/wire.rs:186` | `OpResult::Recorded` — one variant, one word, now three callers |
| `crates/fleetor-db/src/migrations.rs:1` | No `CHECK` on `events.kind`, so a new kind needs no migration |
| `src-tauri/src/runs.rs:386` | `record_for` counts messages and tasks by kind; a handoff raises `events` and needs no new column |

## Scope

**In:** one verb, one op, one event variant, its TS mirror and Activity row, the prose in
both briefs, and the tests that pin all of it.

**Out, deliberately:**

- **Anything that reacts.** No evaluator, no window, no trigger, no state change. WP-15 owns that, and this verb is useful before it exists.
- **A `handoffs` column on `RunRecord`.** The event is in `events.json` and the History row's `events` count already moves; a second derived number would be UI work for a fact the archive already carries.
- **The operator's inbox.** A handoff is not a message and must not be rendered as one — `orch` said nothing to anybody. It is a declaration on the record, and the Activity feed is where the record is read.
- **A "mission" object.** There is no start-of-mission marker, no id to correlate handoffs with, no notion of an open or closed mission anywhere. A handoff is a claim in a log, and the moment it becomes a state something reads is the moment `task.rs`'s tripwire trips.
- **Any refusal of a *second* handoff.** `orch` finding more work after handing back and handing back again is a real sequence; a system that argued would be asserting it knows better, exactly as the task board refuses to.

## Design sketch & open questions

**Why the worker brief was touched at all.** `brief::require_verbs` validates the
rendered **worker** brief against the same `VERBS` list as the orchestrator's, so a verb
in `VERBS` that `worker.md` never mentions makes the shipped template fail its own
validation. The options were (1) one line in `worker.md`, (2) splitting `VERBS` into
per-pane lists, or (3) leaving `handoff` out of `VERBS` and breaking the clap pin. Option
1, taken: it is one sentence, it is *useful* prose rather than a formality — `done` and
`handoff` are the two verbs a model could most plausibly confuse — and options 2 and 3
both trade a single stable CLI contract for a shape where a verb can end up pinned by
nothing. Logged in `decisions.md` as the Tier 2 call it is.

**Why a new event variant rather than a `Message` to `operator`.** A message is something
a pane *said* to an addressee; rendering a mission declaration in the message record
would put words in `orch`'s mouth. The structured fields are the other half of it — a
handoff flattened into prose would lose the property that its evidence is a list anyone
can walk, which is most of what makes it worth archiving.

**Why `orch`-only, and why refused in the CLI.** The refusal is a contract on the sender,
checked before the socket, exactly where `done.rs` refuses `orch` and for the same
reason: the sender reads its own stderr, and the message can name the verb it actually
wanted. The hub does not re-check — it records `from` as given, and the log is the
accountability.

**Open, and left open on purpose:** whether a handoff should carry a pointer to the
confirmed-vision file `orch` wrote. It would make the archive self-contained, and it
would also be a path that may not exist, in a field nothing validates. Wait for a real
run to say whether the evidence lines already cover it.

## Session prompt

```
Read docs/roadmap/13-done-verb.md in full, then building.md §1 and §9. This package is
landed — read it as the record of what `fleet handoff` is and why, before changing any
part of it. If you are adding something that reacts to a handoff, you are in WP-15, and
the veil (WP-12) is the constraint that matters: `orch` learns nothing about it.
```

## Session exit checklist

- [x] `decisions.md` entry (D-064), cited in the commit subject.
- [x] Verb added: both prompt files + `VERBS` + the clap enum + pinned tests in one commit.
- [x] No spike — nothing here touched an unknown; the seams were all read rather than measured.
- [x] As-built docs updated in the same PR: `docs/fleet-comms-map.md` §2, §3e, §7; root `README.md`'s verb list; `building.md` §4's op count.
- [x] This doc: status → landed, "How it landed" appended.
- [x] `00-index.md` status column updated.

## How it landed

One commit, as the contention rule requires. The verb is `handoff`; it answers
`recorded`; it appends `FleetEvent::Handoff` and does nothing else.

**Measured cost (`count.py`, DeepSeek Flash tokenizer): orch 3,366 → 3,668 (+302),
worker 2,099 → 2,139 (+40).** The orch figure is **over the 3,500 cap D-056 set**, and
the cap is restated to **3,800** in D-064 with its reasoning, per D-053's reversal clause
— re-measured and stated, never silently edited. The ledger's finding held again: a verb
costs what its *rules* cost. The syntax is one line; the rules are the altitude split,
the evidence requirement, the check-before-you-claim instruction and the consequence
attached to each.

**What was not needed.** No migration (`events.kind` has no constraint), no `RunRecord`
change, no `deliver.rs`/`pty.rs`/`message.rs` diff at all — the message path's diff for
this package is empty by construction. `src-tauri/src/prompts.rs`'s override-fixture
template needed `fleet handoff` added, which is the contention rule catching a ninth
place the verb list is written down, exactly as designed.

**Tests:** 210 workspace (49 CLI, 108 core, 26 pane-messaging, 7 task-board, **3
handoff**, 4 event-bus, 12 db, 1 ipc), 121 shell (107 lib, 3 dev-mode, 11 real-pty),
`tsc --noEmit` and `vite build` clean.
