# Peer review over the shared object database — the measurement behind WP-06

Measured 2026-08-06, git 2.x, macOS. The claim under test is the one the
worktree reconsideration was settled on (`00-index.md`, operator decision 3):
**a reviewer can read a peer's branch from its own worktree, with no fetch, no
shared checkout, and no write access to anything the peer owns.** If that were
false, peer review would need either a shared checkout (which collapses review
semantics — one checkout reported five times) or cross-worktree access (a Tier
1.7 security escalation, `building.md` §9.2, above a builder).

It is true, and it costs zero lines of code. This document is the transcript.

## Method

A throwaway repo, built with plain git in a scratch directory — no FLEETOR
binary, no tokens, no model. The worktrees are created with **exactly** the
command `src-tauri/src/fleet.rs::ensure_worktree` runs:

```
git worktree add -B fleet/worker-<N> <dir>
```

Both workers then commit, so the branches have genuinely **diverged** — the
realistic case, and the one where the diff form matters.

## Transcript

```
$ git init target && cd target && git commit -m "the operator's trunk"
### target HEAD: a3558f4 on master

### exactly what ensure_worktree runs
$ git -C target worktree add -B fleet/worker-1 worktrees/worker-1
Preparing worktree (new branch 'fleet/worker-1')
HEAD is now at a3558f4 the operator's trunk
$ git -C target worktree add -B fleet/worker-2 worktrees/worker-2
Preparing worktree (new branch 'fleet/worker-2')
HEAD is now at a3558f4 the operator's trunk

### each worker commits its own work in its own worktree
worker-1 HEAD: 3d5e093   ("a parser nobody else has seen")
worker-2 HEAD: 35d81f9   ("its own unrelated work")
```

Everything below runs **from `worktrees/worker-2`**. No `cd` into worker-1, no
`git fetch`, no remote configured at all (`git remote -v` prints nothing).

```
$ git log --oneline HEAD..fleet/worker-1
3d5e093 worker-1: a parser nobody else has seen

$ git diff HEAD...fleet/worker-1
diff --git a/parser.rs b/parser.rs
new file mode 100644
--- /dev/null
+++ b/parser.rs
@@ -0,0 +1 @@
+pub fn parse() {}

$ git show fleet/worker-1 --stat --oneline
3d5e093 worker-1: a parser nobody else has seen
 parser.rs | 1 +

$ git rev-parse --git-dir
target/.git/worktrees/worker-2      ← worker-2's own index and HEAD
$ git rev-parse --git-common-dir
target/.git                         ← the object database, shared
```

The last two lines are the mechanism in full. Each worktree gets a **private**
`git-dir` (its own `HEAD`, its own index — which is why four workers can commit
concurrently without colliding) while `--git-common-dir` points all of them at
**one** object store. A peer's commit is therefore already local: `git cat-file
-t <worker-1's sha>` from worker-2 answers `commit`, with nothing fetched.

## The trap the worker brief names

Two dots and three dots are not interchangeable here, and the wrong one is the
one a model reaches for first:

```
$ git diff HEAD..fleet/worker-1        # ← WRONG
diff --git a/lexer.rs b/lexer.rs
deleted file mode 100644               # ← worker-2's OWN work, as a deletion
--- a/lexer.rs
+++ /dev/null
@@ -1 +0,0 @@
-pub fn lex() {}
diff --git a/parser.rs b/parser.rs
new file mode 100644
+pub fn parse() {}
```

Two-dot diffs the two tips against each other, so everything the *reviewer* has
done since the fork renders as a deletion by the *author*. A reviewer reading
that would open with "you deleted my lexer," which is both false and the kind of
false a peer cannot easily talk you out of. Three-dot diffs against the merge
base and shows only what the author actually added.

This is why `prompts/worker.md` teaches the three-dot form with the reason
attached, and why `brief.rs` pins both the form and the sentence explaining it as
literals. The rule without the reason is a rule a rewrite deletes.

## The merge step, and the repo-boundary test

`prompts/orch.md` teaches the merge as a worktree rather than a checkout, so orch
never moves the branch the operator is standing on. Run verbatim:

```
$ git -C $I merge --no-ff fleet/worker-1 -m "parser, reviewed by worker-3"
Merge made by the 'ort' strategy.
$ git -C $I merge --no-ff fleet/worker-2 -m "lexer, reviewed by worker-1"
Merge made by the 'ort' strategy.

$ git -C target rev-parse --abbrev-ref HEAD
master                              ← never moved
$ git -C target status --porcelain
                                    ← 0 changed files
```

Then Tier 1.1's boundary test, on the same repo:

```
$ rm -rf worktrees && git -C target worktree prune
$ git -C target worktree list
target  a3558f4 [master]            ← the target itself, and nothing else
$ git -C target branch
  fleet/integration
  fleet/worker-1
  fleet/worker-2
* master
```

Clean, minus kept feature branches — which is precisely the carve-out Tier 1.1
already names. Trunk is untouched, in the working tree and in the ref.

## What this does not prove

- **The shared-checkout fallback.** When the target is not a git repo,
  `worker_cwd` puts every worker in the target directory itself: no per-worker
  branches, one working tree, and `git diff fleet/worker-N` has nothing to
  compare. Review does not degrade there so much as stop meaning anything, which
  is why WP-06 made that notice say so in words rather than only reporting that a
  worktree failed.
- **That a worker cannot write into a peer's worktree.** Nothing in git prevents
  it; what prevents it is that auto-approve is scoped to the worker's own
  worktree (Tier 1.7) and the worker brief says not to. Read-only is a boundary
  plus a rule, not a git guarantee — worth knowing before anyone calls it
  sandboxing.
- **Anything about a live model.** Whether a DeepSeek Flash worker actually
  reaches for the three-dot form after being told to is WP-09's shakedown.
