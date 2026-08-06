# WP-NN — <name>

status: not-started size: S/M/L
depends-on: <WP numbers, or —> blocks: <WP numbers, or —>
brief-cost: <±N tokens if this touches prompts/*, else 0 — and expect the guess to run ~4× low once a verb's *rules* are written; see D-053>

<!--
This template is extracted from WP-01..09, which were each executed by a fresh session
from the doc alone. The standard it has to meet: a session reading building.md plus this
one doc knows what to build, what not to build, which invariants apply and why, where the
code is, and how the session ends. Delete these comments as you fill it in.

To file a new package: copy this file to the next zero-padded number, fill it in, add a
row to 00-index.md's table and an edge to its dependency graph. Update the Status column
there as the last act of the session that lands it.
-->

## Outcome

<!-- What this enables, in vision terms — one short paragraph, not a feature list. -->

## Performance criteria

### Technical

<!-- Checkable items: exact commands, tests to pin, diffs that must be empty. -->

- [ ] …

### Semantic

<!-- Which part of the vision this serves; how a reviewer judges it beyond the tests. -->

- [ ] …

## Invariant guardrails

<!-- The building.md §1 clauses this package brushes, the argument that already happened
     (cite D-numbers), and the ALLOWED shape. If the design wants a gate, a queue, or a
     transform in the message path — stop and file a Tier-1 question instead. -->

## Current state (verified <date> — do not re-explore)

<!-- file:line anchors, verified in the session that writes this doc. This section is why
     the implementing session doesn't need explore agents. Re-verify anchors mechanically
     before committing the doc. -->

## Scope

**In:** …

**Out:** <!-- pruning is the point — the vision is also what it isn't. -->

## Design sketch & open questions

<!-- Recommended default per question. Anything touching an unknown is spike-first
     (building.md §4): examples/ throwaway + a version-stamped docs/notes/<topic>-notes.md.
     Where the spike and this doc disagree, the spike wins. -->

## Session prompt

<!-- Ready to paste verbatim into a fresh session. Name: this doc (read in full first),
     building.md §1 and §9, the spike-first rule where applicable, and the exit checklist.
     If the work needs live spend, say so — spend is named and operator-approved first
     (building.md §9.5). -->

```
…
```

## Session exit checklist

- [ ] `decisions.md` entry for every Tier 2 default that moved (cite the D-number in the commit subject).
- [ ] If a verb was added or changed: both prompt files + `VERBS` + the clap enum + pinned tests move in one commit.
- [ ] Spike notes committed to `docs/notes/`, version-stamped, with a `docs/README.md` index row.
- [ ] As-built docs updated in the same PR (`docs/fleet-comms-map.md` for anything on the message path).
- [ ] This doc: status → landed, "How it landed" appended.
- [ ] `00-index.md` status column updated — the last act of the session.
