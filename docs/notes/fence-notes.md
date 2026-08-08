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
| **rustup / cargo** *(added 2026-08-08, D-069)* | `cargo build` works; `~/.cargo/bin/cargo` is a **symlink to `rustup`**, which dispatches on `argv[0]` and resolves toolchains under `$HOME/.rustup` | **two separate failures, and which one you get depends on how the app was launched.** With the operator's login PATH inherited: `error: rustup could not choose a version of cargo to run` (exit 1), writing one file — an empty `settings.toml` — into the worker's private HOME. With a GUI launch's PATH: `cargo: command not found` (exit 127), because neither `/opt/homebrew/bin` nor `/usr/local/bin` holds a cargo and those are the only two non-operator rungs `worker_augmented_path()` supplies | **fixed by D-069** — the reversal condition D-052 wrote for itself, exercised as written. `RUSTUP_HOME` and `CARGO_HOME` are both pointed at fleet-owned directories under `_shell/`, and the shims a worker resolves are seeded there. See the dated section below for the arms. |
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

---

# The Fence and the Rust toolchain — measured (D-069, 2026-08-08)

`examples/fence-toolchain-spike/toolchain.sh`, thirteen arms. This section is
appended rather than folded in: the catalogue above gained one row, and
everything below is the evidence behind it.

## Why the earlier measurement was not enough

WP-17 open question 1 recorded the breakage from
`write-guardrail-notes.md` §3.4: rustup cannot choose a toolchain under a
private HOME. That arm — and every other arm in `allowlist.sh` — ran with
`PATH="$PATH"` (`allowlist.sh:84`), the **operator's login PATH**, which carries
`~/.cargo/bin`. So it never asked whether a worker can find `cargo` at all, and
that turns out to be a different question with a different answer.

Every arm here states its PATH explicitly and runs under
`env -u CARGO_HOME -u RUSTUP_HOME -u RUSTUP_TOOLCHAIN`. Two PATHs are compared
throughout:

- `WPATH_TERM` — `{fleet-bin}:/opt/homebrew/bin:/usr/local/bin:` + the operator's
  login PATH. What `npm run tauri dev` inherits.
- `WPATH_GUI` — the same head with launchd's default (`/usr/bin:/bin:/usr/sbin:/sbin`)
  as the tail. What a Finder launch inherits, **inferred, not confirmed** — see
  arm 1b.

## What a worker can and cannot find (arm 1)

| tool | under `WPATH_TERM` | under `WPATH_GUI` |
|---|---|---|
| `cargo` | `~/.cargo/bin/cargo` | **NOT FOUND** |
| `rustup` | `~/.cargo/bin/rustup` | **NOT FOUND** |
| `rustc` | `~/.cargo/bin/rustc` | **NOT FOUND** |
| `cc` | `/usr/bin/cc` | `/usr/bin/cc` |
| `git` | `/opt/homebrew/bin/git` | `/opt/homebrew/bin/git` |

`/opt/homebrew/bin/cargo` and `/usr/local/bin/cargo` are both **absent** on this
machine. Those are the only two rungs `worker_augmented_path()` contributes that
are not the fleet's own, so **no PATH FLEETOR builds can find a cargo unless the
inherited tail happens to carry one.** `~/.cargo/bin/cargo` is a symlink to the
`rustup` binary (11 MB, real) — the shim mechanism is rustup dispatching on
`argv[0]`, which matters for the fix.

## Arm 1b did not answer its question, and says so

The intent was to measure what a Finder-launched app inherits, by launching a
minimal `.app` bundle through `open` and having it dump its PATH. **It came back
inconclusive**: a sentinel variable and a deliberately mangled PATH rung both
appeared in the probe's output, so `open` forwarded the calling shell's
environment rather than launchd's. The probe measured a terminal.

Recorded rather than quietly dropped, because two other methods failed first and
the next person will reach for them:

- `launchctl getenv PATH` is **empty** on this machine — which is why `WPATH_GUI`
  uses launchd's built-in default. That is supporting evidence, not proof.
- `ps eww -p <pid> | tr ' ' '\n' | grep '^PATH='` is **actively wrong here.** The
  login PATH contains `/Applications/VMware Fusion.app/Contents/Public`, a rung
  with a space in it; the splitter truncates PATH at that space and loses every
  rung after — including `~/.cargo/bin`. Run naively it reports "no cargo rung"
  for a process that has one.

So: **`WPATH_GUI` remains an inference**, and the arms below treat it as one.
The inference is load-bearing in one direction only — it says the failure is
*worse* than §3.4 recorded, never better — and the fix does not depend on it,
since arm 2b shows the login-PATH case is broken too.

## The two baselines (arms 2, 2b)

| arm | PATH | exit | what happened |
|---|---|---|---|
| 2 | `WPATH_GUI` | **127** | `sh: cargo: command not found`. Nothing written anywhere. |
| 2b | `WPATH_TERM` | **1** | `error: rustup could not choose a version of cargo to run`. One file written: `<private HOME>/.rustup/settings.toml`. |

Arm 2b reproduces §3.4 exactly, with the confound removed. Arm 2 is the failure
§3.4 could not see: under a GUI launch a worker fails **earlier and differently**,
before rustup is ever reached.

## Where each candidate's writes land

Every build is the same crate — one crates.io dependency, so the registry is
exercised. Counts are `find <dir> -newer <ref> -type f` after each arm.

| arm | `RUSTUP_HOME` | `CARGO_HOME` | cargo found via | exit | `~/.cargo` | `~/.rustup` | `_shell/` |
|---|---|---|---|---|---|---|---|
| 3 · both real | operator | operator | `~/.cargo/bin` rung | 0 | **2** | 0 | 0 |
| 4 · relocated cargo home | operator | `_shell/cargo` | `~/.cargo/bin` rung | 0 | **0** | 0 | 8 |
| 5a · proxy, `cargo` only | operator | `_shell/cargo` | seeded proxy | **101** | 0 | 0 | 0 |
| 5b · proxy, full shim set | operator | `_shell/cargo` | seeded proxy | **0** | **0** | 0 | 53 (worktree) |
| 9 · mirrored rustup home | `_shell/rustup` | `_shell/cargo` | seeded proxy | **0** | **0** | **0** | 53 (worktree) |

The two files arm 3 writes into the operator's `~/.cargo` are
`.global-cache` and `registry/index/index.crates.io-*/config.json`.
(`write-guardrail-notes.md` §3.1 recorded one; `.global-cache` is the second, and
the difference is a newer cargo, not a different measurement.)

**Arm 4 answers the load-bearing question: a relocated `CARGO_HOME` works.**

**Arm 5 answers the other one, with a condition.** rustup *does* dispatch through
a proxy symlink outside its own install directory — but only with the full shim
set. 5a, with `cargo` alone on PATH, fails at
`could not execute process 'rustc -vV' (never executed)`: cargo resolves, then
looks for `rustc` by name and finds nothing. 5b adds `rustc`, `rustup`,
`rustdoc`, `cargo-fmt`, `cargo-clippy`, `rustfmt`, `clippy-driver` — all
symlinks to the same `rustup` binary, exactly as `~/.cargo/bin` does it — and
builds clean.

*(5b's `cargo fmt --check` failed with "'cargo-fmt' is not installed for the
toolchain". Checked: it fails identically in the operator's own shell — the
`rustfmt` component is not installed on this machine. Pre-existing, not the
proxy's doing.)*

## The finding that changed the design (arms 6b, 9, 9b)

The plan going in was `RUSTUP_HOME` = the operator's real `~/.rustup`, on the
grounds that it is read-only in practice. **Arm 6b falsifies that.**

A `rust-toolchain.toml` pinning an **installed** channel (arm 6a) writes nothing.
A `rust-toolchain.toml` pinning an **uninstalled** channel — `1.74.0`, an
ordinary thing for a real repository to carry — downloads and installs it, with
no prompt and no confirmation:

```
  35981  files written under the operator's ~/.rustup
           ~/.rustup/update-hashes/1.74.0-aarch64-apple-darwin
           ~/.rustup/toolchains/1.74.0-aarch64-apple-darwin/bin/rustdoc
           ~/.rustup/toolchains/1.74.0-aarch64-apple-darwin/bin/cargo
```

~1.2 GB, into a directory `rm -rf ~/.fleetor` does not reach. That is Tier 1.1
broken by a file in the target repo.

**Arm 9 costs 4 KB and fixes it.** A fleet-owned `RUSTUP_HOME` at
`_shell/rustup`, holding a copy of `settings.toml` (98 bytes) and one symlink per
toolchain pointing into the operator's real `~/.rustup/toolchains/`, builds
clean with zero writes anywhere outside the worktree.

**Arm 9b is the decisive one.** The same uninstalled `1.74.0` pin, run against
the mirrored `RUSTUP_HOME` (and with `1.74.0` uninstalled from the operator's
first, so any hit there would be fresh):

```
  35981    $WORK/fleetor/_shell/rustup
  0        $WORK/fleetor/_shell/cargo
  0        ~/.rustup
  0        ~/.cargo
```

The identical 35,981 files, landing inside `~/.fleetor` instead. **Tier 1.1
holds:** a runaway toolchain download is now a directory the operator can delete,
not a surprise in their home.

## `cargo install` — the escalation, proven rather than reasoned (arm 7)

The argument against pointing `CARGO_HOME` at the operator's own `~/.cargo` was
that `cargo install` would plant a binary on the operator's login PATH, and that
the write guardrail cannot stop it because `cargo install X` names no path. Both
halves measured:

| arm | `CARGO_HOME` | where the binary landed |
|---|---|---|
| 7a | `_shell/cargo` | `_shell/cargo/bin/spikey` — inside `~/.fleetor`, deletable |
| 7b | operator's `~/.cargo` | **`~/.cargo/bin/spikey`** — on the operator's login PATH |

7b is accident-shaped, which is the threat model this repo committed to (D-065):
a worker improving a Rust project deciding to `cargo install cargo-nextest` is
entirely reasonable behaviour, and nothing in FLEETOR would refuse it or report
it. *(The spike uninstalls its own binary; the run confirmed it was gone.)*

## Two workers, one cargo home (arm 8)

Both workers built concurrently against a single shared `_shell/cargo`, both
exit 0. worker-1's log carries `Blocking waiting for file lock on package cache`
— cargo's own lock, doing its job. **A shared cargo home is correct**; there is no
need for one per worker, and per-worker `target/` directories already live in each
worktree.

## Footprint (arm 10)

| | |
|---|---|
| the operator's `~/.rustup` | 1.9 GB |
| the operator's `~/.cargo` | 348 MB |
| the fleet's `CARGO_HOME`, after eight builds of a one-dependency crate | **616 KB** |
| the fleet's mirrored `RUSTUP_HOME`, seeded | **4 KB** |

Per-worker copies of either were never plausible; a shared, fleet-owned pair of
directories costs kilobytes until something downloads a toolchain, and then it
costs what that toolchain costs — in a place the operator can delete.

## What this still does not stop

Named for the same reason the `~/.ssh` caveat above is:

- **The seeded shims reach the operator's `rustup` binary by absolute path**, and
  the mirrored toolchains are absolute symlinks into `~/.rustup/toolchains/`. The
  Fence never claimed to stop absolute paths; this is the same category as
  `SSH_AUTH_SOCK`, and it is what makes a 4 KB mirror possible instead of a 1.9 GB
  copy.
- **`rustup` itself is on a worker's PATH** (it must be — arm 5a shows the shim
  set is all-or-nothing). `rustup toolchain install`, `rustup update`,
  `rustup component add` all now write into `_shell/rustup` rather than the
  operator's, which is the containment claim — not that a worker cannot run them.
- **A worker can still delete or corrupt the mirror.** `_shell/` is inside every
  pane's write-guardrail roots by design (D-065), and that is deliberate: the cost
  is a re-seed at the next fleet start, never the operator's toolchain.
- **`WPATH_GUI` is an inference** (arm 1b). If a Finder launch turns out to
  inherit the full login PATH after all, the seeded proxy is redundant rather
  than wrong — `~/.cargo/bin` would sit later on PATH than the fleet's own rung,
  and the fleet's shims would still win.
