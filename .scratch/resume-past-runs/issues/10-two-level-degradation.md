# 10: Two-level degradation

**What to build:** Resume degrades gracefully instead of failing loudly. If the target repo is gone, Resume is refused with a clear reason and View is still offered. If a single worker's `branch@commit` is gone, that seat comes back View-only with a banner explaining why while the rest of the fleet resumes live — and that seat behaves like an idle pane the others can still `fleet send` to.

**Blocked by:** 09.

**Status:** ready-for-agent

- [ ] Target repo missing/moved → Resume refused with a stated reason; View remains available for the same run.
- [ ] One seat's `branch@commit` missing → that seat returns View-only with an explanatory banner; the other seats resume live.
- [ ] A View-only seat inside a live fleet is indistinguishable from an idle/exhausted pane: peers can `fleet send` to it, it simply does not answer.
- [ ] Both degrade paths mutation-checked (stub the degrade → suite fails), in the spirit of C79.
