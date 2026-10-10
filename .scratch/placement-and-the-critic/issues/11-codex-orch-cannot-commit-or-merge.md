# 11: A codex orch seat cannot commit or merge in the target

**What to fix:** An orch seat running codex has the target repo as its cwd, and codex's `workspace-write` sandbox keeps a `.git` inside the workspace read-only. So orch can neither `git commit` nor `git merge` a worker's branch.

**Blocked by:** nothing. Split from issue 10, which fixed worker seats only (D-115).

**Status:** needs-triage

## Seen

Measured 2026-10-07 on codex-cli 0.161.0 through `codex sandbox`, arm F of `examples/codex-commit-spike/commit.sh`:

- `git commit` fails: `Unable to create '<repo>/.git/index.lock': Operation not permitted`.
- `git merge fleet/probe` fails: `Unable to create '<repo>/.git/ORIG_HEAD.lock'`.

Not seen in a live run. That a live codex orch gets this policy is read, not run: the sandbox trio is written for every seat (`src-tauri/src/placement/codex.rs`, before the `operators_own_seat` branch) and `place_orch` seeds with cwd = the target.

## Likely fix (measured in the spike, not built)

Listing `<repo>/.git` itself in `sandbox_workspace_write.writable_roots` lets both pass (arm F2). That also opens `.git/hooks` and `.git/config`, which issue 10's fix kept closed for workers, so it is a separate Tier 1.7 widening and needs the operator's pick. No narrower set was searched for.

## Done when

- [ ] A codex orch merges a worker's branch during a live run.
- [ ] The real-binary tier covers it beside issue 10's test.
