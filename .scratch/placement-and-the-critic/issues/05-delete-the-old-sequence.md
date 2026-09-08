# 05: The old sequence deleted

**What to build:** The interface shrink the whole refactor was for. With every pane kind now placed through one module, the improvised sequence in the orchestrating module is dead: the long match with its three shapes, the guardrail installation helper beside it, the evaluator brief helper, and the location helpers that placement now owns. They go.

The process-shaping module's public surface goes with them. Its seeding and command-building functions each have exactly one production caller, which is placement — one adapter, so the seam was hypothetical rather than real. They become internals reachable only through placement, so nothing outside it can call them in the wrong order.

This is where the deletion test pays out. Before, deleting the placement module would put the ordering back in the caller *and* in the guardrail test. After, there is one place the order lives, and adding a pane kind costs one variant rather than a new arm in a long untested match.

**Blocked by:** 04 (The evaluator placed, and the guardrail test drops its copy).

**Status:** ready-for-agent

- [ ] All five panes spawn exactly as before; no operator-visible change
- [ ] The bring-up match, the guardrail helper and the evaluator brief helper are gone from the orchestrating module
- [ ] The process-shaping module exposes no seeding or command-building function outside placement
- [ ] The duplicated spellings of the per-project directory layout converge on the layout value; no module re-derives it independently
- [ ] The layout is held on the fleet's live state; the host is not, because it is discovered per spawn
- [ ] No behaviour change: the suite is green with no test deleted, weakened, or marked ignored
- [ ] The orchestrating module is materially smaller, and what remains of it is bootstrap, the operator's composer, target configuration and the run commands
