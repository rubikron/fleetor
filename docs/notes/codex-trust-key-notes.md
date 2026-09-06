# codex trust-key notes

**Decision: C34.** Issue #25, the WP-25 P2 spike named by C16.

**Version stamp: `codex-cli 0.153.4`** (npm `@openai/codex`, macOS 15.7.4, arm64) — the
same build every arm in `codex-spike-notes.md` was recorded against.

Re-runnable in one command, **zero tokens**:

```
python3 examples/codex-spike/trust_probe.py
```

A **sibling** of `probe.py`, not an edit to it (C13). Same conventions: loud PASS/FAIL per
arm, exit code equal to the number of failed arms, a loud clean skip when `codex` is
absent, a drift note when the running build is not the recorded one.

**Never rewritten** (`building.md` §4). Findings append.

---

## The question

C16 recorded an observation and named its limit. `spawn::project_key` canonicalizes
(`std::fs::canonicalize`) and matches **exactly**; the operator's own `~/.codex/config.toml`
carries `[projects."/Users/bubblyducks/harness"]` — a **parent** of this repository — which
*suggested* codex resolves trust by root rather than by exact path. **Observed, not
verified.** Getting it wrong is the sharpest failure in this arc: every pane parks on an
unanswerable dialog while every `fleet send` reports `accepted`.

## The answer

**Neither, as stated. It is a two-candidate exact lookup, and canonical.** A directory is
trusted when *either* of two paths appears verbatim as a `[projects."…"]` key:

1. the **canonicalized cwd**, or
2. the **git root** the cwd resolves to — and for a **linked git worktree that root is the
   main repository**, not the worktree's own `git rev-parse --show-toplevel`.

There is **no ancestor walk**. A key for a plain parent directory does not trust its child.

The operator's `/Users/bubblyducks/harness` entry is not evidence of by-root resolution:
`~/harness` is a plain directory that is not a git repository at all, `~/harness/fleetor`
is the repository inside it, and neither candidate for a pane in `fleetor` is `~/harness`.
The entry is there because codex was once run in `~/harness` itself. **C16's inference is
falsified** — arm `parent-of-repo-key-does-not-trust-repo` reproduces exactly that shape
and gates.

## The instrument

The first-run gate renders itself into the pty, which makes it observable with no human:

```
> You are in /private/tmp/codex-trust/plain/parent/child
  Do you trust the contents of this directory?
  Working with untrusted contents comes with higher risk of prompt injection.
  Trusting the directory allows project-local config, hooks, and exec policies to load.
› 1. Yes, continue        2. No, quit
```

A trusted pane never draws it and reaches the composer instead, resolving `directory:
<cwd>` in its header and `· <cwd>` in its status line. Each arm boots a real `codex` TUI on
a real pty with a fabricated `CODEX_HOME`, drains until one marker appears, and kills the
pane **before anything is typed into it**. That is the entire zero-token story: no arm
completes a turn, and the fabricated `[model_providers.probe]` points at a loopback port
with nothing listening, so there is not even a capture server to answer.

Three states, not two. An arm that renders **neither** marker is `INCONCLUSIVE` and counts
as a failure. Treating "no gate" as "trusted" would let a pane that crashed before drawing
pass every arm silently — the exact class of lie D-034 exists to refuse.

Two mechanical traps cost real time and are recorded so nobody pays them twice:

- **A `pty.fork` pty is zero-sized**, and codex draws *nothing at all* into it. Every arm
  read as INCONCLUSIVE until `TIOCSWINSZ` set 40×120. `probe.py`'s typing arm never noticed
  because it asserts on the request body, not on the screen.
- **The header elides long paths** — past roughly 44 characters it renders
  `directory: /private/tmp/…/parent/child`. A marker that only looked for the header
  reported two genuinely trusted panes as inconclusive. The status line carries the full
  path, so both spellings are checked.

## The matrix

Every row is an arm in `trust_probe.py`. `entry` is what was written into the fabricated
`config.toml` as `[projects."<entry>"] trust_level = "trusted"`. Paths are shown relative
to `/private/tmp/codex-trust`; `repo` is a git repository, `wt` is a linked worktree of it,
`plain/*` is a plain nest with no git anywhere.

| # | entry | cwd | result |
|---|---|---|---|
| 1 | *(none)* | `plain/parent/child` | **gate** |
| 2 | `plain/parent/child` | `plain/parent/child` | trusted |
| 3 | `plain/parent` | `plain/parent/child` | **gate** |
| 4 | `plain/parent/child` *(via `-c`, not persisted)* | `plain/parent/child` | **gate** |
| 5 | `/tmp/codex-trust/plain/parent/child` *(unresolved spelling)* | `plain/parent/child` | **gate** |
| 6 | `plain/parent/child` *(canonical)* | `/tmp/codex-trust/plain/parent/child` | trusted |
| 7 | `repo` | `repo/sub/deep` | trusted |
| 8 | `repo/sub` | `repo/sub/deep` | **gate** |
| 9 | *(root of the fixture, a plain parent of `repo`)* | `repo/sub` | **gate** |
| 10 | `wt` | `wt` | trusted |
| 11 | `wt` | `wt/inner` | **gate** |
| 12 | `repo` | `wt/inner` | trusted |

Rows 3 and 9 are the by-root question and they answer it: no ancestor walk, with or without
git. Rows 7 and 8 are what root resolution *does* mean: the git root counts, an intermediate
directory does not. Rows 10–12 are the finding that decides issue #30.

Recorded against both binaries on this machine and identical on both — see *the shim*, below.

## The git-worktree finding, which is the one that matters

FLEETOR puts panes into **linked git worktrees**, and that is where the rule bites:

- a key for the worktree trusts the worktree **root only** (row 10), and **not one directory
  below it** (row 11);
- a key for the **main repository** trusts the worktree and everything under it (row 12).

Row 11 is the trap. A per-worktree entry looks correct, boots correctly, and then gates the
moment a pane's cwd is anything but the worktree root — and `git rev-parse --show-toplevel`
inside the worktree returns the worktree, so the obvious way to compute the key produces the
key that *doesn't* cover subdirectories. The resolved root for a linked worktree is the main
repository, which is what row 12 demonstrates.

## Canonicalization

Rows 5 and 6 settle the macOS case, and they settle it asymmetrically:

- an entry written `/tmp/codex-trust/plain/parent/child` is **ignored** for a cwd that
  resolves to `/private/tmp/...` — the gate appears;
- an entry written `/private/tmp/...` **is** honoured for a cwd spelled `/tmp/...`.

So codex canonicalizes the cwd and then matches the key literally. The key must be stored
resolved. This is precisely why `spawn::project_key` calls `std::fs::canonicalize`, and the
same call is correct for codex — the *shape* of the key differs, the canonicalization does
not.

## With no entry at all

The pane parks on the gate (row 1). `1. Yes, continue` is pre-selected but nothing presses
it: no arm ever saw a gated pane proceed on its own, though the longest any was watched is
the probe's 14-second budget, so "parks indefinitely" is the reasonable reading rather than
a measured one. What *is* measured is that it is **observable without a human**: the literal
string `Do you trust the contents of this directory` reaches the pty within about three
seconds, which is what every gate row above detects.

The non-interactive path fails differently and more loudly: `codex exec` in the same
directory prints

```
Not inside a trusted directory and --skip-git-repo-check was not specified.
```

and exits — but that message conflates the trust check with the git-repo check, so it is not
a clean trust observable in a directory that *is* a git repository. The TUI gate is the
ground truth, and the TUI is what a pane actually runs.

## No flag opens the gate — and `-c` does not either

Tried against an untrusted directory, all still gated:

| attempt | result |
|---|---|
| `-s workspace-write -a never` | gate |
| `--dangerously-bypass-approvals-and-sandbox` | gate |
| `-c projects."<path>".trust_level="trusted"` | gate |

The third is the surprise and the load-bearing one. That override is **not** a parse
failure: `-c projects."<path>".trust_level="bogus"` and the same value written into
`config.toml` both fail config loading with the identical error, so the dotted path does
reach the projects table. The gate reads the **persisted** configuration. **Trust must be
written into the seeded `CODEX_HOME`'s `config.toml`; it cannot be passed on the command
line.**

Recorded as *tried and rejected* rather than *unconsidered*, in the manner of C5's narrow
lever.

## What issue #30 should implement

For each pane, when seeding its private `CODEX_HOME`, write **two** trust entries into
`config.toml` — not one:

```toml
[projects."<canonicalized worktree path>"]
trust_level = "trusted"

[projects."<canonicalized main-repository path>"]
trust_level = "trusted"
```

The second is the entry that does the work (rows 11, 12); the first costs one table and
covers the case where the pane's cwd is the worktree root and nothing has been resolved
through git at all. Both paths go through `std::fs::canonicalize` — an unresolved key is
silently ignored (row 5).

The main-repository path is the parent of `git rev-parse --path-format=absolute
--git-common-dir`; inside a linked worktree `--show-toplevel` returns the **worktree** and
is the wrong input. Seeding it is not a widening of Tier 1.7: the entry lives in a
per-pane, disposable `CODEX_HOME`, and what a trusted project may *do* is still bounded by
the seatbelt `codex-spike-notes.md` arm 2 measured.

The conformance assertion for checkpoint 14 is therefore two-part, and neither part is an
artifact assertion alone: that FLEETOR wrote both keys canonically, and that a pane booted
in a subdirectory of its worktree renders no gate.

## Not measured

- **The mechanism, only the behaviour.** Codex's source was not read. That the resolved
  root is "the parent of the git common dir" is inferred from rows 10–12, which are also
  consistent with "the main worktree of the repository" by some other spelling. Any of
  those spellings produce the same path for the worktrees FLEETOR creates.
- **Submodules, bare repositories, and a worktree whose main repository has itself moved.**
  Untried.
- **Whether accepting the gate writes the entry back, and where.** No arm ever answers the
  dialog, deliberately — answering it is a write, and this spike writes to no real config.
- **`trust_level` values other than `"trusted"`.** Only that a bogus value fails config
  loading outright.

## The shim, and why it is in these notes

On the machine this was recorded on, `codex` on `PATH` is **not** the vendor binary: it is a
cmux wrapper shim that execs `/Applications/cmux.app/Contents/Resources/bin/cmux-codex-wrapper`,
which in turn runs the npm `codex` with `--dangerously-bypass-hook-trust` added. That banner
appears in every pane the probe boots through `PATH`.

The whole matrix was therefore run twice — through the shim and directly against
`/opt/homebrew/bin/codex` — and **every arm gave the identical verdict both ways**, so the
shim is not a confound for trust. `trust_probe.py` honours `CODEX_BIN` so the direct run is
one command, and it prints the binary it drove. Worth knowing for the other measurements in
this suite too: anything recorded through `PATH` on this machine was recorded through a
wrapper.
