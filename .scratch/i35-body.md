## Parent

Part of #13 (WP-25 — the `Harness` seam, and codex as the first harness through it).
Roadmap: `docs/roadmap/25-codex-tui.md`. Decision trail: `decisions.md` C1–C24, amending M1–M28 where named.

## What to build

At the start gate — the same screen where the operator already chooses which repository the fleet is deployed against — each seat gains a harness and a model.

One row for the orchestrator, one row for the workers, and a disclosure that expands the workers row into four. The common case — one harness for all four workers — costs one selection; a mixed fleet is one disclosure away.

Each harness reports its login and plan as facts beside the model. A harness that is not logged in appears **disabled with the reason inline**, never hidden — a supported feature must never look unimplemented — with a **re-check button** so logging in from another terminal does not require restarting the app.

The last choice is remembered **per harness**, not per seat alone, so switching a pane to codex and back restores the model it had rather than a default.

A Claude Code orchestrator keeps a "default (your login)" sentinel, so today's behaviour stays reachable after the picker exists.

**No provider picker exists anywhere.** The orchestrator inherits and displays; workers get FLEETOR's.

Changing a model mid-run is the harness's own command, not a FLEETOR control. Two consequences to be honest about in the UI: the remembered choice goes stale the moment someone uses that command, and it is remembering what was last *launched*, not what a pane is *running*.

## Acceptance criteria

- [ ] An orchestrator row and a workers row, each carrying harness and model
- [ ] A disclosure expands the workers row into four independent seats
- [ ] Login and plan appear as facts beside the model for each harness
- [ ] A not-logged-in harness is disabled with the reason inline, never hidden
- [ ] A re-check button re-runs discovery without an app restart
- [ ] The last choice is remembered per harness
- [ ] A Claude Code orchestrator can still select 'default (your login)'
- [ ] No provider picker is added

## Blocked by

- #34 — WP-25 P3: the codex diagnostic probe on the host — three auth shapes, resolved posture, live model list

