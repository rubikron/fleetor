# WP-21 — the Critic interviews, on the operator's word

status: not-started size: L
depends-on: WP-20 (the Critic exists) · a `fleet reply` fix, in flight — see Blocking defect
blocks: — brief-cost: **0**, by design — see Invariant guardrails §4

## Outcome

The retro is the best thing WP-12 designed and the only pane that can run one is hidden
behind a build feature. `12-self-improving-loop.md:41` defines it: *"the conversation after
`orch` declares done: evaluator questions, orch answers, proposals come out."* That is a
product capability wearing an instrument's clothes — reading a run and then **asking the
people who did it what they were thinking** is how you find a lapse of judgement, and
lapses of judgement are invisible in an archive that only records what was said.

This package gives the retro to the Critic, for ordinary runs, on the operator's word. The
Critic already reads. It gains the ability to ask, when the operator opens the interview
and not before, and a second output section for what asking produced.

**The operator's switch is the design, not a convenience.** An interview costs the fleet
turns and perturbs a run in progress. That trade belongs to the person paying for it, and
handing it to them resolves — rather than argues about — the contamination question that
would otherwise block this package.

## What the interview is for, and the line it must not cross

The current brief forbids judging the work: *"you never say whether the work was good,
whether the tests were sufficient, whether the design was right."* Asking about **mistakes
and lapses of judgement** sounds like it reverses that. It does not, and the distinction
is the most important sentence in this document:

| In remit | Out of remit |
|---|---|
| A decision about **the run** — routing, decomposition, verification, what a pane concluded and on what basis | A judgement about **the code** — whether it works, whether the design is right, whether the tests suffice |
| "orch reported the run verified by receipts; no receipt existed" | "the retry logic should have been extracted" |
| "worker-3 reported consensus 4.6s after writing that it was waiting for confirmation" | "worker-3's approach was inelegant" |

Both examples in the left column are real findings from the Critic's first live run, and
both are exactly the kind of thing an interview sharpens: the archive shows *what* the pane
did, and only the pane can say what it believed at the time. The remit does not widen. What
widens is the evidence available inside it.

## The three stages

| | Adds | The risk it carries |
|---|---|---|
| **A — the interview** | a socket, and an operator control that opens it | contamination; confabulation |
| **B — the dynamics** | more than one archive in scope | a law announced from three runs |
| **C — the proposals** | the right to prescribe, into WP-12's ledger | Goodhart; a remit reversal |

A is the only stage that changes the process model. B widens what is on disk. C changes
the brief's central prohibition and should not be attempted until A has produced evidence
that asking is worth the turns.

## Blocking defect — `fleet reply`, in flight

`crates/fleetor-cli/src/main.rs:72-76` declares `Reply { text: Vec<String> }` with
`trailing_var_arg = true`, and `Op::Reply { text }` has no recipient field. So
`fleet reply worker-3 "…"` parses `worker-3` as the first word of the body and delivers to
whoever spoke last. The Critic's first live run measured **9 misdeliveries in 5m 40s**,
with two panes spending turns diagnosing "the cross-replies got tangled".

An interview is a reply-heavy exchange. Run one over this verb and the transcript is
unreadable. The fix — refuse at parse when the first argv element resolves to a `PaneId` —
is being built separately and must land first.

## Performance criteria

### Technical

**Stage A**

- [ ] An **Open interview** control in the Critic view. Pressing it makes the Critic an
      address; pressing it again removes it. Disabled when no fleet is running.
- [ ] While closed, `fleet send orch` from inside the Critic fails at **resolution** with
      "no such pane". A test asserts the event log is *unchanged* — not merely that the
      call failed.
- [ ] While open, the same call is `accepted` and reaches the pane. Tested on a real pty
      in `tests/panes.rs`, not by inspecting a `CommandBuilder`.
- [ ] A pane's answer reaches the Critic and appears in the log with `to: critic`.
- [ ] Opening and closing the interview each write a `notice` to the run's event log. The
      log records outcomes, and this one changes what is possible.
- [ ] `PaneId::Critic.is_fleet_member()` is still `false`, **with a socket in hand** — no
      roster row, not a broadcast leg, in no rendered brief's peer list. This is the point:
      addressable, never enumerated.
- [ ] `prompts/orch.md`, `prompts/worker.md` and `VERBS` byte-unchanged; `git diff` empty.
- [ ] The four veil tripwires still pass and still match the evaluator only.
- [ ] The Critic's write-guardrail roots are still its own working directory alone. A pane
      that gained a voice must not also gain a pen.

**Stage B**

- [ ] The Critic is placed against a directory of archives; a test asserts the layout.
- [ ] Every `RunSource` variant that exists, works.

**Stage C**

- [ ] A proposal carries WP-12's four ledger fields — surface, evidence, what it would
      change, what would falsify it — and a test pins that the brief demands all four.
- [ ] The guardrail is not relaxed. Proposals land in the Critic's own directory; moving
      one into the ledger is an operator action.

### Semantic

- [ ] An interview answer is never the sole support for a finding. A reader can tell from
      the report which claims rest on the record and which on testimony.
- [ ] The panes do not start writing for the Critic. Neither brief mentions it; a pane
      meets the name when a message arrives.
- [ ] The operator can tell, before pressing the control, that it will spend the fleet's
      turns.

## Invariant guardrails

### 1. Tier 1.4 — the closed interview is addressing, not delivery

The Critic must not be able to interrupt work the operator has not agreed to interrupt,
and the naive shape — accept the send, then drop it — is precisely the banned thing.
`building.md` §9.3 records that this argument has been had and lost twice.

**The allowed shape, with precedent:** while the interview is closed the Critic is not an
*address*. `fleet send orch` from inside it fails at resolution with "no such pane",
identically to `fleet send worker-9` today. Nothing is accepted-then-dropped and the log
does not lie. This is the resolution WP-12 already chose for the cross-fleet cutoff
(`12-self-improving-loop.md`, Tier 1.4 section) — the same problem, so the same answer.

The operator's control moves the Critic between *not an address* and *an address*. It is
never a mute on a live route.

### 2. The veil

Two run-readers now hold sockets and `orch` will receive messages from one of them.

- `prompts/critic.md` still contains no evaluator vocabulary. The tripwire
  `the_critics_brief_is_openly_named_and_names_no_evaluator_vocabulary` covers five of the
  six needles; the sixth, `answer key`, is exempted at line 14 as the Critic describing its
  own limit, pinned by exact phrase. That exemption survives this package unchanged.
- **Known, and stage A sharpens it:** in a dev-mode session the evaluator's first-contact
  message to `orch` is in `events.json`, which the Critic reads (D-076, "Not verified").
  Today it can read that. With a socket it could *ask orch about it*, putting the
  evaluator's existence into a live conversation with a fleet member. See open question 2.

### 3. Auto-approve and the guardrail are untouched

The Critic's write roots stay its own directory alone. Voice and pen are independent
properties and only one moves here.

### 4. The prompt budget pays nothing

orch sits at 3,668 against a 3,800 cap; worker at 2,139 against 2,200 (D-064). There is no
room for a protocol, and D-053's finding is that a verb costs ~4× whatever was estimated,
because it costs what its *rules* cost.

**So no brief teaches the Critic.** A pane meets the name when a message arrives and
answers with `fleet reply`, which it already knows. This is not only budget arithmetic: a
brief that told panes they may be interviewed is a brief that tells them to perform for the
interview — the failure the veil prevents for the evaluator, and which cannot be prevented
that way here, because the Critic is openly named.

## Current state (verified 2026-09-04 — do not re-explore)

| Fact | Anchor |
|---|---|
| The evaluator has a socket; the Critic does not | `placement/spawn.rs` — `evaluator_command_with` calls `apply_pane_env(…, PaneId::Evaluator, socket, …)`; `critic_command_with` sets only `CLAUDE_CONFIG_DIR`, the securestorage dir, terminal env |
| A socket is two variables, set in one place | `apply_pane_env` (`placement/spawn.rs:411`) — `FLEETOR_PANE` + `FLEET_SOCKET` |
| The absence is pinned today | `tests/placement.rs::the_critic_is_given_no_socket_so_fleet_send_inside_it_reaches_nothing`, comparing against `orch` holding both |
| Neither reader is enumerated | `pane.rs::is_fleet_member` → `!matches!(self, PaneId::Evaluator \| PaneId::Critic)` |
| The retro is defined, and it is an interview | `12-self-improving-loop.md:41` |
| The handoff wake seam exists | `fleet.rs:469` (watch), `:539` (`wake_evaluator`), `:59` (`EVENT_EVALUATOR_WAKE`) |
| The live snapshot contains no briefs | `runs::snapshot_live_run` writes `events.json`, `manifest.json`, `transcripts/` only |
| The proposal ledger exists as a concept | `12-self-improving-loop.md:367` — four fields; approve / defer / reject |
| `proposal ledger` is evaluator vocabulary | one of the six `EVALUATOR_WORDS` in `tests/dev_mode.rs` |
| The brief forbids prescription | `prompts/critic.md`, pinned by `critic.rs::tests::the_shipped_brief_keeps_the_citation_rule_and_the_ban_on_prescription` |

## Scope

**In:** the socket and the operator-controlled resolution gate (A); a multi-run working
directory (B); a PROPOSALS section feeding WP-12's ledger (C); the `fleet reply` fix.

**Out:**

- **A second proposal ledger.** WP-12 has one. The naming collision below is a cost of
  using it, not a reason to fork it.
- **Automated application of a proposal.** Every stage ends at a human.
- **The Critic initiating without the operator.** No timer, no auto-open at handoff, no
  "it seemed important". The control is the feature.
- **Teaching either brief about the Critic.** See guardrail 4.
- **The Critic reading the repository.** It has no target and keeps none — a pane that
  reads a run has no business holding the live checkout.

## Design sketch & open questions

### The operator's switch, and what it replaces

An earlier draft of this document gated the interview on `fleet handoff`, reasoning that
the work is finished there and nothing asked can change what was built. That is a sound
window and it is now the *recommended moment*, not the mechanism. Two reasons the switch is
better:

1. **The trade is the operator's to make.** An interview spends turns and can perturb a
   run. Deciding that is exactly the kind of call the operator already owns as the fleet's
   only writer. A hardcoded window makes the decision for them and calls it safety.
2. **Some lapses are only visible while they are happening.** A worker stuck in a
   pleasantry loop, two panes rediscovering the same fact — the run's first report found
   both. Waiting for handoff to ask about them means asking a pane to reconstruct a state
   of mind it has since compacted away.

The Activity feed records both edges. An operator who opens an interview mid-run should see
that decision in the log alongside its consequences.

### Testimony is not record

The brief's rule is *"you can cite what a pane wrote, not why it wrote it."* An interview
answer is technically written by the pane, but afterwards, to an assessor, about its own
conduct. Admitting it as archive evidence would quietly delete the rule.

- A third citation form: `interview <pane> <ts>`, visually distinct from `events.json seq N`
  and `transcripts/<pane>/<file>.jsonl line N`.
- A finding may be **corroborated** by testimony and never **established** by it. Every
  finding keeps an archive anchor.
- **An answer that contradicts the archive is itself a finding** — of the archive-anchored
  kind, because the contradiction is visible in the record. This is where lapses of
  judgement will actually surface.
- UNCITED survives. A question asked and not settled belongs there *with the answer
  recorded*, which is more useful than either half alone.

**The bar for stage A.** The first live report left six items in UNCITED. Three are
answerable by asking — the CLI's signature, what the briefs said about `fleet done`, the
intended recipient of two bare replies. Two are answerable by nobody — screen contents,
token cost. So: **an interview stage that converts fewer than three of six is not earning
its complexity**, and that is a measurement, not a feeling.

### Stage B: dynamics need three runs, not two

"Behaviour dynamics" only means anything across runs; one run cannot separate a pattern
from an incident. The cheap implementation is placing the Critic against a directory of
archives.

The discipline transfers: a cross-run claim needs a citation *per run*, and WP-12's own
example of ledger evidence is "the same pattern in runs 3 and 5". A pattern claimed from
fewer than three runs goes to UNCITED.

### Stage C: confine the prescription, do not relax it

The spike measured that removing the anti-prescription rule produces "solid work, maybe
more tests" — the exact failure the Critic exists to avoid. So stage C adds one section
where prescription is *required* and must carry all four ledger fields. The fourth —
**what would falsify it** — does the work: a Critic that cannot fill it has found a
preference, not a pattern. FINDINGS keeps the prohibition, and the test pinning it must not
be weakened to make PROPOSALS compile.

### Open questions

1. **Does an open interview stay open across a `/clear` or a pane restart?** *Recommended:
   yes — it is a property of the run, not of a pane's session.* A pane restarted mid-
   interview meets the Critic's name fresh, which is already the ordinary case.

2. **The dev-mode leak, sharpened.** With a socket, a Critic that has read the evaluator's
   first-contact message can ask `orch` about it. Options: filter evaluator events from the
   Critic's snapshot (a lie of omission, and the archive is the one thing that must not
   lie); accept it, since the veil's own arc records the containment as "free, and *not*
   airtight"; or refuse to open an interview at all when the run was a dev-mode run.
   *Recommended: the third — the only option that neither lies nor widens the leak, and it
   costs nothing, because a dev-mode run is being graded by the evaluator anyway.*

3. **What does a pane see?** An ordinary `fleet send` from `critic`. *Recommended: leave it
   exactly that.* Special rendering would be a second spelling of a pane identity, and panes
   already handle names they were not introduced to (D-068).

4. **The naming collision.** `proposal ledger` is one of the six veil needles, so the moment
   `prompts/critic.md` names the ledger, the Critic's own tripwire fails. This is the
   `answer key` collision again, and the same exemption is available. *Recommended: take it,
   and argue it in the decision entry rather than doing it quietly — two exemptions is the
   point where someone should ask whether the tripwire still measures what it thinks it
   does.*

## Session prompt

```
Implement WP-21 stage A only — the Critic interviews on the operator's word. Read
docs/roadmap/21-the-critic-interviews.md in full first, then building.md §1 and §9, then
12-self-improving-loop.md's Tier 1.4 section for the resolution-gate precedent and line 41
for what a retro is.

Confirm the `fleet reply` fix has landed before you start; stage A depends on it and the
defect is measured, not hypothetical.

The shape: the Critic gains FLEETOR_PANE and FLEET_SOCKET, exactly as the evaluator has
them via apply_pane_env, but only while the operator has opened the interview. Closed, it
is NOT AN ADDRESS — sends fail at resolution, never accepted-then-dropped, which is the
Tier 1.4 banned shape and has been argued and lost twice. Open and close each write a
notice to the event log.

It stays out of is_fleet_member with a socket in hand: no roster row, no broadcast leg, no
peer list. Do not edit prompts/orch.md, prompts/worker.md or VERBS. Do not relax the write
guardrail. The four veil tripwires must still pass and still match the evaluator only.

prompts/critic.md gains: the interview's citation form; the rule that a finding is never
established by testimony alone; and the in-remit/out-of-remit line — a lapse of judgement
about the run is in, a judgement about the code is out. Do not touch the anti-prescription
paragraph; that is stage C's business.

Stage A is done when a real pane on a real pty answers the Critic with the interview open,
and refuses to resolve with it closed. Record the Tier 2 decisions in decisions.md.
```

## Session exit checklist

- [ ] `decisions.md` entry for every Tier 2 default that moved (cite the D-number in the commit subject).
- [ ] No verb added: `VERBS`, the clap enum, `Op`, both prompt files unchanged — `git diff` empty on all four.
- [ ] Spike notes in `docs/notes/` if the interview's value was measured rather than assumed, with a `docs/README.md` index row.
- [ ] `docs/fleet-comms-map.md` updated — this puts a new participant on the message path.
- [ ] This doc: status → landed, "How it landed" appended.
- [ ] `00-index.md` row added and status column updated — the last act of the session.
