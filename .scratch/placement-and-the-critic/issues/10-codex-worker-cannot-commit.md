# 10: A codex worker cannot commit in its worktree

**What to fix:** A worker seat running codex can edit files in its worktree but cannot `git commit`. Its work never reaches its own branch, so there is nothing for a reviewer to read and nothing for orch to merge.

**Blocked by:** nothing.

**Status:** needs-triage

## Seen

Live run on 2026-10-06, target `/Users/bubblyducks/harness/harness-test`, worker-1 on `codex` / `gpt-5.6-luna`, the other seats on `claude-code`. Worker-1 reported:

> Commit blocked by sandbox: worktree gitdir/object DB under /Users/bubblyducks/harness outside writable root (index.lock Operation not permitted).

The claude-code workers committed normally in the same run.

## Cost in that run

- Worker-1's line-count work was folded into worker-2's commit by hand.
- Its `fleet done` receipts read `7bf43b7 + uncommitted changes`, the first with exit 1.
- About five of the run's 35 messages were this failure and its workaround.

## Likely cause (read, not tested)

A worker's worktree lives under `~/.fleetor/_shell/worktrees/…`, but a git worktree keeps its index and the shared object database in the **target repo's** `.git` (`.git/worktrees/<name>/` and `.git/objects/`). Codex panes are placed with `sandbox_mode = "workspace-write"` (`src-tauri/src/placement/codex.rs`, `sandbox_keys`), and I found no `writable_roots` entry there. So the sandbox allows writes under the pane's cwd and refuses the write to the target's `.git`.

## Likely fix (not tried)

Add the target repo's `.git` directory to the codex pane's `sandbox_workspace_write.writable_roots` when the seat is a worker. Check whether the write guardrail's roots for claude-code panes (`guardrail.rs`, `roots_for`) already make the same allowance, and keep the two in step.

## Done when

- [ ] A codex worker commits in its worktree during a live run.
- [ ] Its `fleet done` receipt names a commit with no "uncommitted changes".
- [ ] A conformance check covers it, so a codex version change cannot silently bring it back.
