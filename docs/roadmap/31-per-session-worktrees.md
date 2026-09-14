# WP-31 — Worker worktrees belong to a session, not to a repo

status: not-started size: M
depends-on: 27 blocks: —
brief-cost: 0 — R26 already added `{branch}` and `{branch_prefix}`, and this package changes what they *render as*, not the template. If a notice or a new placeholder is added, this number changes with it.

## Outcome

A reopened run comes back to the files it left, instead of to whatever a later
session happened to leave in the shared checkout. Today every session on a repo
shares one set of worktrees, so reopening an older session lands its workers on a
later session's branch tip and uncommitted edits — and under R13 the worker is
told nothing about it. This is R23, decided 2026-09-11 and never built.

**Read R25 before anything else in this doc.** R23 was recorded and lost in the
same commit as R24: `d83fb98` carried both decisions, its code implemented only
R24, and R23 got no package, no index row and no issue. R24 was blocking — two
real sessions had been refused — and R23 blocked nothing *visible*, because a
shared worktree produces a reopen that looks completely correct. This package
exists because that failure mode is the interesting part: the bug is invisible by
construction, so only a test or a directory listing finds it.

## Performance criteria

### Technical

- [ ] `Layout::worktree` takes the lineage's `SessionsId` and the target, and a
      reopened run resolves to its **lineage root's** worktrees — the same unit R4
      uses for seat directories, not the new run's id.
- [ ] `ensure_worktree` creates per-session branches. Git refuses one branch in
      two worktrees, so this is forced by the layout change rather than chosen:
      `worker_branch`/`worker_branch_prefix` gain the session level and stay the
      only spelling.
- [ ] A test proves two sessions on the same target get different worktree paths
      **and** different branches, and that reopening the first returns to the
      first's path — driven through `ensure_worktree`, not asserted as literals.
- [ ] R26's agreement test still passes: the brief names the branch the worktree
      is actually on. It is the regression net for this whole change.
- [ ] **Delete cascades** (R15). `run_delete` removes the lineage's
      `pane-config/<lineage>/`, its worktrees (`git worktree remove`) and its
      branches, not only `runs/<id>/`. The doc comment claiming no cascade is
      needed is deleted with the behaviour it describes.
- [ ] Deleting a lineage leaves `git worktree list` clean in the target — no
      prunable stale entries.
- [ ] A pre-R23 lineage does not reopen, and the History row says so (R27),
      reusing R10's existing refusal rather than a new concept.
- [ ] `cargo test --no-fail-fast` green apart from `vendor_binary_tier`'s two
      known arms (R29, R30).

### Semantic

- [ ] An operator who reopens a two-week-old session sees their own files, and
      nothing in the UI has to explain why.
- [ ] The disk cost is visible rather than discovered: whatever grows per session
      is named somewhere the operator can read.

## Invariant guardrails

**Tier 1.1 — everything under `~/.fleetor`.** The new level goes inside
`_shell/worktrees/`; `rm -rf ~/.fleetor` must still remove every worktree FLEETOR
made. Branches, however, live in the **operator's target repository** — this
package creates refs outside `~/.fleetor` for the first time at this scale, which
is exactly why the delete cascade is a criterion and not a nicety.

**R13 — a reopened pane is told nothing.** That is what makes a stale worktree
dangerous rather than untidy: nothing will ever correct the worker's belief about
its files. It is also why the fix is structural rather than a warning.

**R23's cost, already accepted by the operator:** disk grows by one set of
checkouts per session, and every new session builds cold.

## Current state (verified 2026-09-12 — do not re-explore)

The layout as it stands, all per **target repo**, with no session level:

- `Layout::worktree` `src-tauri/src/placement/mod.rs:269` — `worktrees/<target
  slug>/worker-N`. Its doc comment still carries R7's superseded rationale.
- `target_slug` :355 — repo directory name + 4 hex digits of a hash of its
  absolute path. **Not a session id**, though it reads like one; this is what R23
  was mistaken for.
- `ensure_worktree` :1511 — the only builder, and the only caller path; it
  short-circuits on an existing `.git`, which is how a reopen silently adopts a
  later session's tree.
- `worker_branch_prefix` :374, `worker_branch` :379 — **the one spelling of a
  branch name**, landed by R26 (`9e7608c`). `ensure_worktree` calls it. This is
  the seam this package changes; before R26 the name was a `format!` at the call
  site and the briefs disagreed with it for a month.
- `shared_checkout_warning` :1418 — the fallback notice when no worktree could be
  made. Unrelated to this change, but it names branches, so it is in blast radius.

What already has a session level, and is the model to copy:

- `pane_config_run` :306 — `pane-config/<lineage root id>/<seat>/` (R4). For a
  reopened run this is the **parent's** id, which is precisely the rule worktrees
  need.
- `UNASSIGNED_BRANCH_PREFIX` :332 and `UNASSIGNED_SESSIONS` — the placeholder
  pattern for a value a fleet has not yet resolved.
- `PaneContext::branch_prefix` `src-tauri/src/prompts.rs:117`, set at the one
  place a fleet boots, `src-tauri/src/fleet.rs:1455`. The spawn path cannot
  compute a prefix — a worker's `cwd` is its worktree, not the target — so
  anything the session level adds has to ride here too.
- `render_orch` / `render_worker` `crates/fleetor-core/src/brief.rs:114` and
  `:131` — both take `branch_prefix`; `{branch}` and `{branch_prefix}` are
  rendered from it (R26).

The delete path, and the leak it already has:

- `runs::delete` `src-tauri/src/runs.rs:898` — removes `runs/<id>/` and nothing
  else.
- `fleet::run_delete` `src-tauri/src/fleet.rs:2487` — its doc comment says
  "Nothing else in the app refers to a run by id, so this needs no cascade."
  **That is already false**: `pane-config/<lineage>/` is keyed by the lineage root
  id. Twelve sessions' seat directories are on disk behind twelve rows today.

Measured on this machine, 2026-09-12, as the evidence R23 was never built: twelve
directories under `pane-config/`, three under `worktrees/` — `harness-test-f7f9`,
`repo-0730`, `stockTrading-b116`. All twelve worktrees are clean, so nothing is at
risk in the migration itself.

## Scope

**In:** the session level on worktrees and branches; the reopen resolving to the
lineage root; R15's delete cascade; R27's refusal for pre-R23 lineages.

**Out:**

- **Build output for archived sessions** — R23's own open question, and the part
  that can reach gigabytes. Name the cost; do not build a pruner here.
- **Whether `orch` moves off the target repo** — R23 left it open and nothing in
  this package forces it. Orch has no worktree and gets only `{branch_prefix}`.
- **`prompts/orch.md`'s hardcoded `_shell/worktrees/integration`** — a sibling of
  the per-target directories that needs the session level too, or two sessions
  merge into one tree. Real, and its own small piece of work; R25 records it.
- Anything in WP-29. The two packages are independent — this one is vendor-generic
  and changes both harnesses identically.

## Design sketch & open questions

1. **Path order: `<session>/<target>/worker-N` or `<target>/<session>/worker-N`?**
   **Recommended:** target first, session second, so one repo's worktrees stay
   together and a per-target cleanup is one directory walk. It also keeps
   `target_slug`'s collision argument intact at the level it was written for.
2. **Which id — the run's or the lineage root's?** **Recommended:** the lineage
   root's, without exception, because R4 already made that the unit a reopen
   shares and R12 makes a lineage one conversation. A new run id here would give
   every reopen a cold checkout and quietly restore the bug this fixes.
3. **`git worktree remove` or `rm -rf` plus `prune`?** **Recommended:** `remove`
   first and `prune` as the fallback, because `rm -rf` alone leaves the target's
   `.git/worktrees` entries behind and the operator sees them in
   `git worktree list` forever.
4. **Does the delete cascade land first, alone?** **Recommended:** yes. It is a
   verified leak *today*, it is reviewable on its own, and R27 has just made
   twelve rows deletable — so it is the half with immediate value. Landing it
   second means writing it against a layout that is itself new.
5. **What does the operator see when a pre-R23 row refuses?** **Recommended:**
   R10's existing "cannot be reopened" treatment with a reason naming the session
   layout, not a new refusal concept.

## Session prompt

```
Read docs/roadmap/31-per-session-worktrees.md in full, then building.md §1 and §9,
then decisions.md's R23, R25, R26 and R27 — R25 is the one that explains why this
package exists at all, and R26 is the seam you will be changing.

Do not re-explore anything in the doc's "Current state" section; those anchors were
verified 2026-09-12. Re-verify them mechanically before you commit and say so if any
moved — they moved once already this week when R26 landed.

Work TDD, as R26 did: the test that fails first is the one that proves two sessions
on the same target get different worktrees and that reopening the first returns to
the first's path. Drive it through `ensure_worktree` rather than asserting literals;
R25 is the record of what comparing two literals costs.

Land the delete cascade first and alone (open question 4), then the session level.

This needs no live spend and no model turns. If you find yourself wanting a fleet to
prove something, say so and stop — a directory listing found every fact in this doc.
```

## Session exit checklist

- [ ] `decisions.md` entry for every Tier 2 default that moved, citing the
      R-number in the commit subject; R23 gets a "how it landed" note.
- [ ] `docs/runtime-layout.md` and `docs/context-architecture.md` updated — both
      still document `worktrees/<target-slug>/worker-N` on branch `fleet/worker-N`,
      which was already wrong before this package and will be wronger after.
- [ ] R27's refusal visible in History, not only in the backend.
- [ ] This doc: status → landed, "How it landed" appended.
- [ ] `00-index.md` status column updated — the last act of the session.
