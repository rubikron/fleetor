# 07: As-left worktree + git provenance

**What to build:** The archive records each worker's `branch@commit`, and placement can recreate a worktree **as the worker left it** — a non-resetting checkout of the recorded commit, distinct from the existing `ensure_worktree` that resets the branch to HEAD. Foundation for Resume's filesystem restoration.

**Blocked by:** 03.

**Status:** ready-for-agent

- [ ] `PaneRecord` gains `branch@commit` per worker seat, captured at archive time; a fresh run's manifest carries it.
- [ ] `placement` gains a non-resetting as-left worktree add distinct from `git worktree add -B`.
- [ ] Placement test asserts the as-left checkout lands the recorded commit and does **not** reset the branch (the property separating it from `-B`).
