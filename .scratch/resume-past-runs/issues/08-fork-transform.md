# 08: Fork transform

**What to build:** Forking a frozen past run into a new live run, as a pure transform with no pty: the new live `state.db` is a copy of the frozen one (carrying the task board + event history forward), it records its `parent`, and it emits a restore request plus an as-left worktree request per seat. The frozen original is never written.

**Blocked by:** 07, 04.

**Status:** ready-for-agent

- [ ] Forking a frozen `runs/<id>/` produces a new live run whose `state.db` is a copy of the frozen `state.db`.
- [ ] `LiveMeta` gains `parent: <id>`; the fork records it.
- [ ] The fork emits a transcript-restore request and an as-left worktree request for each seat (consumed by 09; not executed here).
- [ ] The frozen original is asserted byte-unchanged after a fork (physical isolation, D-058).
- [ ] Tested exactly as the existing `runs.rs` tests (plant a frozen run, fork, assert files); no pty spawned.
