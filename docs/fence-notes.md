# The Fence — private worker HOME, measured (WP-08)

Measured 2026-08-06, git 2.x, macOS. The claim under test: **once a worker's
`HOME` is a private directory instead of the operator's real one, `~/.ssh`, the
operator's real Claude config and shell profiles stop being reachable *by
name*.** This document is the transcript, plus what it does not prove and what
it deliberately breaks.

This is a fence, not a sandbox — the design's own framing, restated because it
is the only honest one. Nothing here stops a process that opens
`/Users/operator/.ssh/id_ed25519` by its absolute path, or one that reads
`$HOME` out of `/proc`-equivalent introspection and constructs the real path
itself. What it stops is *name resolution*: every tool that asks the OS "where
is my home" — `~` expansion in a shell, `git`, `gh`, a config loader using
`dirs::home_dir()` — gets sent to a directory that does not contain any of the
operator's real files, because `HOME` itself now points there.

## What was seeded, and why

`worker_command` sets `HOME` to `~/.fleetor/_shell/home/worker-N`
(`fleet.rs::worker_home_dir`, a sibling of `pane-config` and `worktrees` — same
`_shell` root, same per-slot layout, still under `~/.fleetor` so `rm -rf
~/.fleetor` still removes everything FLEETOR made, Tier 1.1). The directory is
created on spawn (`spawn::seed_worker_home`, called from `fleet::spawn_pane`
before the process exists — same reasoning as the config-dir L1 seed already
documented there).

**Exactly one file is seeded: `.gitconfig`.**

```
[user]
	name = fleet worker-N
	email = worker-N@fleetor.local
```

This is the WP-06 interaction `00-index.md` records under "Contention
warning": once `HOME` stops pointing at the operator's real one, the global
`user.name`/`user.email` git used to read from `~/.gitconfig` are gone too, and
a worker's first `git commit` — `fleet done`'s first step — fails outright: no
author, no commit, and a receipt that names a commit which was never made.
Nothing else is seeded. The spec's own recommendation (open question 1) was
"nothing beyond gitconfig — add files only when the breakage catalogue
demands, each with a line of why," and the catalogue below found nothing else
load-bearing enough to add.

Written once, on first spawn, and left alone after that — unlike
`seed_config_dir`'s merge-on-every-spawn (that file has to absorb operator
edits and trust-flag changes across target switches; a worker's private HOME
has no legitimate second writer, so re-seeding on every launch would only risk
clobbering something a future breakage-catalogue entry seeded on purpose).

## The falsification test, run live

Live shell processes, not a model — this measures path resolution, and a
`claude` pane resolves paths the identical way a `sh` does. Full transcript:

```
$ env -i HOME=<worker-1's private dir> PATH=/usr/bin:/bin sh -c '
    echo "HOME=$HOME"
    ls ~/.ssh
    echo "exit=$?"
    cat ~/.gitconfig
    git config --get user.name
    git config --get user.email
  '
HOME=.../_shell/home/worker-1
ls: .../_shell/home/worker-1/.ssh: No such file or directory
exit=1
[user]
	name = fleet worker-1
	email = worker-1@fleetor.local
fleet worker-1
worker-1@fleetor.local
```

`~/.ssh` fails *by path resolution* — `ls` looked in the private HOME, found
nothing, and said so with the ordinary "No such file or directory," not a
permission error and not a hang. `~/.gitconfig` resolves to the **seeded**
file, not the operator's real one (compare: the operator's real `~/.gitconfig`
carries their actual name and email, confirmed separately from an unfenced
shell — different content entirely). `git config` reads the seed. The
mechanism is exactly `HOME` substitution; nothing more sophisticated is
running, and nothing more sophisticated needed to.

## The WP-06 debt, re-validated under the fenced env

`00-index.md`'s contention warning made this WP-08's obligation: WP-06 landed
first, so this package re-validates a real worker commit and a peer's
three-dot diff **with the private HOME in place**, not just with a gitconfig
seed reviewed on paper. Same method as `docs/peer-review-notes.md` — a
throwaway repo, worktrees built with the exact `ensure_worktree` command — run
again with each worker's shell wrapped in `env -i HOME=<its private dir> ...`.

```
### worker-1 commits under its fenced env, no -c user.* override
$ env -i HOME=<worker-1 home> PATH=/usr/bin:/bin sh -c '
    cd target/worktrees/worker-1
    echo "pub fn parse() {}" > parser.rs
    git add parser.rs
    git commit -q -m "worker-1: a parser nobody else has seen"
    git log -1 --format="%H %an <%ae> %s"
  '
9681f8a... fleet worker-1 <worker-1@fleetor.local> worker-1: a parser nobody else has seen
```

Author and email come from the seed, not from any ambient config — there is
none reachable. `fleet done`'s receipt would name this commit correctly.

```
### worker-2 commits its own unrelated work, under ITS OWN fenced env
$ env -i HOME=<worker-2 home> PATH=/usr/bin:/bin sh -c '
    cd target/worktrees/worker-2
    echo "pub fn lex() {}" > lexer.rs
    git add lexer.rs && git commit -q -m "worker-2: its own unrelated work"
  '
9d3d440... fleet worker-2 <worker-2@fleetor.local> worker-2: its own unrelated work

### from worker-2's worktree, under worker-2's fenced env — no fetch, no shared checkout
$ env -i HOME=<worker-2 home> PATH=/usr/bin:/bin sh -c '
    cd target/worktrees/worker-2
    git remote -v                              # empty — confirmed no remote configured
    git log --oneline HEAD..fleet/worker-1      # 9681f8a worker-1: a parser nobody else has seen
    git diff HEAD...fleet/worker-1              # the three-dot form
    git rev-parse --git-dir                     # .../target/.git/worktrees/worker-2 (private)
    git rev-parse --git-common-dir              # .../target/.git (shared)
  '
```

`git diff HEAD...fleet/worker-1` produced the same clean added-file diff
`peer-review-notes.md` recorded originally (`parser.rs` as a new file, nothing
rendered as a deletion) — with worker-2's own `$HOME` pointing at a directory
that has never seen worker-1's files, config, or credentials. The mechanism
`peer-review-notes.md` measured (`--git-dir` private, `--git-common-dir`
shared, so a peer's commit is already local with nothing fetched) does not
route through `HOME` at all — it is a property of `git worktree`, independent
of environment — and this re-run confirms fencing `HOME` does not disturb it.
**Peer review over the shared object database still works, unchanged, under
the fence.**

## Breakage catalogue

What was checked, live, against the real tools on this machine:

| Tool | Under the operator's real HOME | Under a worker's fenced HOME | Verdict |
|---|---|---|---|
| `git commit` | reads `~/.gitconfig` for author | reads the **seeded** `.gitconfig` | **fixed by the seed** — this is the whole reason it exists |
| `git worktree` / `git diff HEAD...` | n/a — doesn't read `HOME` | identical — object DB access is by `--git-common-dir`, not `HOME` | **unaffected** |
| `gh` (GitHub CLI) | `gh auth status` → logged in, real account, real token | `gh auth status` → "You are not logged into any GitHub hosts" | **broken, deliberately left broken** — `gh`'s credential store lives under `HOME` (`~/.config/gh`), and nothing seeds it. A worker that ran `gh pr create` today would fail cleanly with a login prompt, not silently act as the operator. Nothing in the current design has a worker call `gh` — worktrees plus `fleet done` receipts are the whole flow — so there is nothing to fix yet; if a future package wants `gh` inside a worker, that is a credential-scoping decision for that package, not a silent side effect of this one. |
| `nvm` | sources `~/.nvm/nvm.sh`, `NVM_DIR` defaults to `$HOME/.nvm` | not installed on this machine — **not measured live**; documented from `nvm`'s own default (`NVM_DIR="$HOME/.nvm"` unless overridden) | **left broken by the same mechanism as `gh`**: a worker would see no Node versions installed under `$NVM_DIR`, cleanly (nvm reports "no versions installed"), not by inheriting the operator's. Not seeded — no version manager is part of any package's scope yet. |
| `CLAUDE_CONFIG_DIR` (`claude` itself) | n/a | still points at `worker_config_dir` regardless of `HOME` | **unaffected, unchanged** — this was already an explicit env var independent of `HOME`, exactly as the spec's Current-state note says |
| shell profiles (`~/.zshrc`, `~/.bashrc`) | sourced by an interactive login shell | not sourced by `claude`'s pty at all (neither before nor after this change — `CommandBuilder` execs the program directly, no shell in between) — but if anything *did* try to source `~/.zshrc` by name under the fenced `HOME`, it would find nothing | **fenced, though it was never reachable through this pty in the first place** — named here because the spec's Outcome line names it explicitly |
| ambient env vars unrelated to `HOME` (`SSH_AUTH_SOCK`, `AWS_PROFILE`, etc.) | inherited from the app's own process, same as today | **still inherited, unchanged** — this package does not touch any env var other than `HOME` and the two PATH rungs; `SSH_AUTH_SOCK` in particular still lets `ssh`/`git+ssh` authenticate through the operator's running agent even with `~/.ssh` unreadable by name | **explicitly out of scope** — named so nobody reads the `~/.ssh` result above as "SSH access is fenced." It is not; only *file* access by the `~/.ssh` path is. Deny rules for env vars are the deferred Posture Ladder, not this package. |

## The PATH fix, landed in the same commit

`augmented_path()` (`spawn.rs`) reads `$HOME` from the **app's own process** —
the operator's real HOME, since the app itself is launched from the operator's
shell — and prepends `{home}/.local/bin:{home}/.bun/bin` ahead of system dirs.
Before this fix, every worker's PATH carried those two rungs regardless of the
worker's own (now private) `HOME`: a name pointed straight at the operator's
own tooling, which would have quietly defeated fencing `HOME` at all — a
worker could still resolve `~/.local/bin/some-operator-script` by name via
PATH, just not via `~`.

**Landed:** `augmented_path()` is now orch-only (unchanged: full inherit, its
docstring says so explicitly). A new `worker_augmented_path()` keeps the
fleet-bin rung and the two system dirs (`/opt/homebrew/bin:/usr/local/bin`),
and drops both operator-HOME rungs — the design sketch's own recommendation
(open question 2), taken as written. `apply_pane_env` now takes the computed
PATH as a parameter rather than computing it itself, so each caller
(`orch_command` / `worker_command`) picks the right one explicitly instead of
one shared function silently serving both.

**What this does not do**, named for the same honesty reason as the `~/.ssh`
caveat above: the *inherited* tail of PATH (`{existing}`, whatever the app's
own process PATH already was) is untouched for both orch and workers. If
FLEETOR is launched from a terminal whose exported PATH already contains the
operator's `~/.local/bin` (an ordinary shell-profile default), that directory
is still reachable through the inherited tail for a worker — this fix removes
only the two rungs `augmented_path()` itself computes and adds, not the PATH
the app was launched with. Stripping the inherited tail down to a fixed list
is a sandboxing decision (picking exactly which binaries a worker may ever
resolve) that this package's spec rules out by name ("no OS-level sandboxing,
no Posture Ladder"). The gap is real and is exactly the shape of gap `[fence]
posture = open` in `prompts/launch.conf` exists to eventually replace with
something stricter, once there is live evidence to act on rather than a guess.

## Semantic summary

The fence is about names and paths, not OS sandboxing (the design's own
framing). What it stops, confirmed live: `~/.ssh`, the operator's real
`~/.gitconfig`, and (per the breakage catalogue) `gh`'s and `nvm`'s stored
credentials/state, all become unreachable *by the `~` / `$HOME` spelling* from
inside a worker pane. What it does not stop, also confirmed or reasoned
above: absolute-path access to the operator's real files (nothing prevents a
worker from typing `/Users/operator/.ssh/id_ed25519` if it somehow knew that
path), non-`HOME`-keyed ambient credentials (`SSH_AUTH_SOCK` and similar),
and — for PATH specifically — whatever directories were already on the
inherited PATH before FLEETOR's own additions. `posture = open` in
`prompts/launch.conf` names this precisely: it reproduces today's behavior
apart from the private HOME, and it is not a promise that anything beyond
name resolution changed.
