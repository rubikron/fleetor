# 03: Workers placed

**What to build:** A worker pane comes up through the placement module. That means everything a worker needs before its process exists is decided and made true in one place: its own git worktree or the announced fallback to the shared checkout, its configuration seed for that exact directory, its private `HOME` with the git identity its commits need, the fleet-owned Rust toolchain or the deliberate absence of those settings, its write-guardrail roots, its context-gauge source, and its command.

This is the ticket that buys the tests the architecture review was written about. Four properties the codebase currently describes in prose and does not check become assertions, all through one seam against a scratch directory.

The most valuable of them is re-seeding after a target switch. Trust is keyed by absolute project path, so pointing the fleet at a new repository means every pane's seed must be re-applied for its new working directory. The code calls this the single most likely way to reintroduce the failure it was written to prevent, and nothing tests it. A pane that misses it still opens, still shows a terminal, and still reports `accepted` for every message — into a dialog it will never leave.

**Blocked by:** 02 (Layout and Host, with orch placed through them).

**Status:** ready-for-agent

- [ ] Four workers spawn into their own worktrees exactly as before; no operator-visible change
- [ ] A test places a worker for one target, then for a second, and asserts both project entries survive in the configuration — the re-seeding rule, checked for the first time
- [ ] A test asserts the returned command carries the fleet-owned private `HOME`, and that an inherited API key in the environment does not survive into it
- [ ] A test asserts that a bare machine — no toolchain discovered — produces a command with the toolchain settings **absent**, not present and pointing at a directory that does not exist
- [ ] A test asserts that a target which is not a git repository produces the shared-checkout fallback and a notice naming what is degraded about peer review
- [ ] A test covering the successful worktree path against a real repository created in a scratch directory
- [ ] The context-gauge source is recorded before the process exists, as it is today
- [ ] Workers and the orchestrator both route through placement; the evaluator still uses the old path and still spawns
