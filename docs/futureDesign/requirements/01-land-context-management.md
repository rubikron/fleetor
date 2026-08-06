# WP-01 — Land `feat/context-management` (prompts-as-files)

status: landed size: M
depends-on: — blocks: 02, 03, 04, 05, 06, 07, 08, 09
brief-cost: 0 (mechanical landing — no prompt content changes)

## Outcome

Every word a pane is told becomes an operator-editable file on master: `prompts/orch.md`, `prompts/worker.md`, fragments (`delivery-contract.md`, `broadcast-rule.md`), rendered with validated `{placeholders}` and overridable via `~/.fleetor/prompts/` with loud Activity notices. This is the substrate every brief-touching package (02, 03, 05, 06, 07) edits files instead of Rust string literals. It also brings the fleet's own design record — the Autonomy Designs doc and the two context-architecture docs — into the main tree.

## Performance criteria

### Technical
- [ ] All currently-uncommitted work in the `context-mgmt` worktree is committed on `feat/context-management`, then rebased (or merged — see open questions) onto master.
- [ ] `cargo test --workspace` green; `cargo test` in `src-tauri/` green (including the 9 headless real-pty tests against fake-pane); `npx tsc --noEmit && npx vite build` green.
- [ ] Override behavior demonstrated: a valid `~/.fleetor/prompts/orch.md` loads with an Info notice; a template missing a load-bearing placeholder is **refused** with a Warn notice and the built-in used; absence is silent. (The branch's `validate_orch`/`validate_worker` refuse templates that don't teach every `VERBS` entry.)
- [ ] Tier 1.1 re-run: `rm -rf ~/.fleetor && git worktree prune` leaves the target repo untouched.
- [ ] Junk excluded: `.DS_Store`, `docs/.Rhistory` are not committed.

### Semantic
- `prompts/README.md` remains a complete, current account of what a pane is told and how overrides work.
- `docs/context-architecture.md` and `docs/context-injection-flow.md` land under `docs/`; `future designs/` lands under `docs/futureDesign/plans/`.
- `building.md` §4.5 ("The briefs … defined in fleetor-core") is reconciled with the file-based reality.
- A `decisions.md` entry (next D-number) records the extraction: what changed, why the in-Rust default lost, what would reverse it.

## Invariant guardrails

- **Tier 1.1 / §4.5:** prompt files live in *this* app repo and render at spawn — never a `CLAUDE.md` in a pane's cwd (it would show in the target's `git status` and a worker could delete it). The branch's test pinning "brief goes in as a system prompt, never as a file in cwd" must survive the rebase.
- **Tier 1.2:** the harness owns pty, environment, and system prompt — file-based prompts are squarely inside that line (`building.md:16`).
- **Do not change prompt content here.** This package lands the mechanism byte-compatible with today's briefs. Content (and the `--system-prompt` flag switch) is WP-02. Keeping the landing mechanical is what makes a 40-commit rebase reviewable.

## Current state (verified 2026-08-06 — do not re-explore)

- Worktree: `.claude/worktrees/context-mgmt`, branch `feat/context-management`, tip `2e38ecf` = merge-base; master is **40 commits ahead** (UI overhaul, Settings tab, D-041 orphan sweep + `RunEvent::Exit` teardown, app icon).
- Uncommitted there (verified via `git status`): modified `crates/fleetor-core/src/brief.rs`, `src-tauri/src/{fleet,lib,spawn}.rs`, `src-tauri/tests/panes.rs`; untracked `prompts/` (6 files), `src-tauri/src/prompts.rs` (357 lines, `PaneContext::resolve`), `docs/context-architecture.md`, `docs/context-injection-flow.md`, `future designs/` (the Autonomy Designs HTML pair), plus junk (`.DS_Store`, `docs/.Rhistory`).
- The branch's `brief.rs`: `render_orch` L73, `render_worker` L83, `render` L100, `validate_orch` L109, `validate_worker` L115 — templates via `include_str!("../../../prompts/…")` with `{me}` `{peers}` `{workers}` `{delivery_contract}` `{broadcast_rule}` slots.
- Master-side conflict zones to expect: `src-tauri/src/lib.rs` (D-041 added the orphan sweep to `setup()` and the `RunEvent::Exit` backstop), `spawn.rs`, `panes.rs`.
- Master baseline being replaced: `crates/fleetor-core/src/brief.rs` — `VERBS` L17, `orch_brief` L22, `worker_brief` L63, `DELIVERY_CONTRACT` L105, anti-amplification clause L91, tripwire test L131-135 that cross-checks briefs against `message::frame_for_pane`.

## Scope

### In
Commit the worktree's work; rebase across the 40 commits; re-validate everything; relocate `future designs/` → `docs/futureDesign/plans/`; reconcile `building.md`; decisions entry; PR in repo style.

### Out
Any prompt content change; any new placeholder slot; the `--system-prompt` flag switch (WP-02); deleting the worktree (operator's call after merge).

## Design sketch & open questions

1. **Rebase vs merge.** Recommended: commit first (checkpoint), then `git rebase master` — conflicts are localized to `lib.rs`/`spawn.rs`/`panes.rs` and the branch's history is one logical change. Fall back to a merge if the rebase fights.
2. **Where `future designs/` lands.** Recommended: `docs/futureDesign/plans/fleetor-autonomy{,-plain}.html` — beside this requirements directory, since it is the roadmap's source material.
3. **Override test.** Recommended: yes, add one test against a temp override dir (load / refuse / absent) — the three notices are load-bearing operator UX.

## Session prompt

```
You are working in /Users/bubblyducks/harness/fleetor. Read
docs/futureDesign/requirements/01-land-context-management.md in full, then
building.md §1 and §9. Execute WP-01: land the feat/context-management
worktree (.claude/worktrees/context-mgmt) onto master — commit its
uncommitted work, rebase across the ~40 commits of divergence, keep the
landing byte-compatible with today's rendered briefs (no content changes,
no flag changes), relocate the design docs as scoped, and re-validate with
the full test matrix (cargo test --workspace; cargo test in src-tauri/;
npx tsc --noEmit && npx vite build). Honor the invariant guardrails
section. Finish with the session exit checklist, including the decisions.md
entry and updating docs/futureDesign/requirements/00-index.md status.
```

## Session exit checklist

- [x] Full test matrix green (workspace 63; src-tauri 44 lib + 9 real-pty; `tsc --noEmit` and `vite build` clean).
- [x] `decisions.md` entry appended — **D-042**, not D-041. The branch guessed its own number before master landed the orphan sweep; four doc comments citing D-041 were renumbered.
- [x] `building.md` §4.5 reconciled.
- [x] `prompts/README.md` accurate.
- [ ] PR — **not applicable.** Superseded: see "How it landed".
- [x] `00-index.md` status: WP-01 → landed.

## How it landed (2026-08-06)

Four things went differently from the plan above. WP-02..09 should read this before assuming the doc's "current state" section still holds.

- **`feat/blackboard`, not master.** All nine packages land on one integration branch off master (`a05ab83`). Nothing was pushed and master was not touched.
- **Merged, not rebased.** `feat/context-management` is checked out in its own worktree, so rewriting its history was off the table. Its uncommitted work became two commits there (`304e8d7` prompts mechanism, `718e0db` docs), then `git merge` into `feat/blackboard`. **One conflict**, in `src-tauri/src/lib.rs`: both sides added a module declaration on the same line. Kept both. D-041's orphan sweep in `setup()` and its `RunEvent::Exit` teardown are untouched — neither is near what this branch changed. `spawn.rs` and `panes.rs` auto-merged clean.
- **`prompts/worker.md` was reworded back to master's text.** The branch had quietly changed one sentence while extracting it ("separate Claude Code *instance*", dropping "The human is watching `orch`, not you"). This package is a mechanism change: all five rendered briefs were captured from the pre-merge binary and diffed byte-for-byte against the post-merge ones, and are **identical**. `--append-system-prompt` and `--permission-mode auto` are unchanged, and `launch.conf` carries the old `DEEPSEEK_BASE_URL` / `MODEL_FLASH` constants verbatim. WP-02 owns both the flag switch and the content.
- **Tier 1.1 was verified by reading, not by running.** `rm -rf ~/.fleetor` on an unattended machine is not a test worth running against the operator's real directory. The relevant property is checked at the source: the only new filesystem access is `PaneContext::resolve`, which **reads** `~/.fleetor/prompts/` and writes nothing anywhere. No new write path was added, into the target repo or otherwise.
