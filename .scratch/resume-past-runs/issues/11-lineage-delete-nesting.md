# 11: Lineage, delete, nesting

**What to build:** History reflects resume chains and stays under the operator's control: a resumed run shows its lineage via `parent`; a run can be explicitly deleted; and a resumed run, once stopped, becomes a normal frozen, viewable, re-resumable run so chains nest without special cases.

**Blocked by:** 09.

**Status:** ready-for-agent

- [ ] History shows resume chains as one thread via each run's recorded `parent`.
- [ ] Explicit per-run delete removes a past run; keep-all remains the default (no auto-pruning until disk growth is measured).
- [ ] A resumed (forked) run that is then stopped is archived as an ordinary frozen run and can itself be viewed and resumed (story 30), with its `parent` intact.
