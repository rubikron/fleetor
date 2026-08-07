# WP-17 — The write guardrail

status: landed size: M
depends-on: 14, 16 blocks: any improve run
brief-cost: 0 — no `prompts/*.md` template moved; `prompts/launch.conf` is launch
settings, not words a pane is told

## Outcome

A fleet pointed at FLEETOR's own source can no longer wander out of its own
worktree by accident. Each pane runs with a `PreToolUse` hook that refuses a tool
call which would write outside its own roots, tells the pane which path and which
rule so it can correct itself, and puts every attempt on the operator's Activity
feed. This is what WP-12 names as *"the only thing standing between an
auto-approving worker and the evaluator's own code"*, and it lands before the
first improve run rather than after the first accident.

## Performance criteria

### Technical

- [x] `cargo test --workspace` — 210 tests, green.
- [x] `cargo test --manifest-path src-tauri/Cargo.toml` — 143 tests, green,
      including the 11 real-pty tests in `panes.rs` unchanged.
- [x] `npx tsc --noEmit && npx vite build` — clean; no UI file touched.
- [x] The hook mechanism verified against the installed Claude Code **before**
      anything was built on it (`docs/notes/write-guardrail-notes.md` §1). WP-12's
      deny-layer 3 stops being "unverified".
- [x] `src-tauri/tests/write_guardrail.rs::the_delivery_path_cannot_read_the_write_guardrail`
      — the Tier 1.4 tripwire, in the shape of `tests/dev_mode.rs`.
- [x] The decision is exercised through the **installed** artifact — the
      `settings.json` command `guardrail::install` wrote, run by a real shell
      against a real `PreToolUse` payload. No second copy of the policy.

### Semantic

- [x] A refused pane can tell a guardrail from a broken path, and says so in its
      own words rather than looping — measured, `write-guardrail-notes.md` §1.
- [x] Nothing legitimate wedges. The two writes a pane cannot work without
      (`cargo build` into `~/.cargo`, `git commit` into the target's `.git`) were
      measured, not assumed, and are pinned by a test.
- [x] The operator can extend the allowlist without a rebuild, through the
      configuration surface that already exists.

## Invariant guardrails

**Tier 1.4 — nothing in the message path.** The one that needed care. A hook that
could delay or refuse a *delivery* is what §9.3 records as argued and lost twice.
This hook governs a pane's own tool calls and is built so that is structurally
true, not merely intended: it is installed into a config directory at spawn and
consulted by Claude Code inside the pane's own process, and nothing between `fleet
send` and a pty imports `guardrail`, mentions it, or can read it.
`tests/write_guardrail.rs` pins both directions — the delivery path cannot see the
guardrail, and the guardrail cannot reach `Hub`, `AppCommand`, `PaneRegistry`,
`Op` or `deliver::`. Its only outputs are a settings file and a journal line, and
the journal is drained by a task of its own that no delivery ever waits on.

**Tier 1.7 — auto-approve never exceeds the worker's worktree. This narrows it.**
Say so plainly so a later reader does not re-litigate: workers already spawn
`--permission-mode auto` scoped to their worktree, and this refuses a **subset** of
what that allowed. §9.2's escalation trigger is *widening* auto-approve; making it
smaller does not trip it and needs no Tier-1 question. WP-12 §"Tier 1.7" is the
argument that already happened: during an improve run a worker's worktree *is* a
checkout of this repo, so auto-approve legitimately covers the dev-mode code and
the evaluator's source — Tier 1.7 is not violated, it stops being a meaningful
limit for that one case, and this package is the limit that replaces it.

**Tier 1.1 — everything under `~/.fleetor`.** The hook script, the settings file
and the journal all live under `_shell`. Nothing is written into the operator's
repo, and `rm -rf ~/.fleetor` removes the guardrail with everything else.

**Tier 1.3 — every pane is a real interactive TUI.** Untouched: a hook is Claude
Code's own mechanism, configured through its own settings file. Nothing forks or
patches CC (Tier 1.2), and the pane is as typeable as it was.

## Current state (verified 2026-08-07 — do not re-explore)

The implementation, since this doc is written after it landed:

- `src-tauri/src/guardrail.rs:90` — `roots_for`: the pane's own cwd, `_shell`,
  and the operator's extras. `:80` `policy_dir` is the one directory inside the
  roots that is still refused; `:74` `journal_path`.
- `src-tauri/src/guardrail.rs:106` — `install`: writes the script into the pane's
  config dir and merges the hook into its `settings.json`. `:152` `hook_command`
  builds the argv (the policy is the command line); `:190` `merge_hook` keeps the
  operator's own settings and hooks (D-062 invites them).
- `src-tauri/src/guardrail.rs:239` — `Journal`, the cursor the feed reads;
  `:270` `notice_for` turns one line into a `Notice`.
- `src-tauri/src/guardrail.rs:56` — the decision itself, baked in with
  `include_str!` for the reason the briefs are (D-042). `:64` the interpreter is
  `/usr/bin/python3` by absolute path; `:70` the tool matcher, which is what makes
  "reads stay open" a property of the wiring.
- `src-tauri/src/write_guardrail.py:253` — `offenders`, the decision;
  `:215` `bash_write_targets`, the write-position scanner; `:273` `refusal`, the
  words a pane sees; `:87` the tools it will answer about at all.
- `src-tauri/src/fleet.rs:401` — `install_guardrail`, called at the spawn site
  (`:362` for `orch`, `:377` for a worker) for the reason `seed_config_dir` is:
  the roots come from the cwd the pane is actually about to run in.
- `src-tauri/src/fleet.rs:433` — `spawn_guardrail_feed`, started at `:291`.
- `src-tauri/src/prompts.rs:52` — `LaunchConfig::fence_allow`; `:231` the
  `[fence] allow` arm; `:249` `absolute_root`, which refuses a relative one.
- `prompts/launch.conf:28` — the `[fence]` section D-052 created, now carrying
  `posture` (`:40`) and the documented `allow` key (`:62`).

What it builds on:

- `src-tauri/src/fleet.rs:132` `shell_dir`, `:162` `pane_config_dir`,
  `:177` `worktree_dir`, `:509` `worker_cwd` (and its shared-checkout fallback).
- `src-tauri/tests/dev_mode.rs:57` — the tripwire this package's own test is
  modelled on.

## Scope

**In:** writes. A `PreToolUse` hook per pane, refusing `Bash`, `Write`, `Edit`,
`MultiEdit` and `NotebookEdit` calls that would write outside the pane's roots; a
refusal message written to be recovered from; a `Notice` on the Activity feed per
attempt; an operator-extensible allowlist.

**Out**, and each of these was considered and rejected rather than forgotten:

- **A read denylist.** A Bash command a hook allows can read anything internally,
  so blocking reads is friction wearing enforcement's clothes. WP-12's own
  "Hiding" section already says the only guardrail that enforces is the file not
  being there.
- **A full read+write allowlist.** Its allowlist has to cover every toolchain
  path, and the failure when it does not is a pane that wedges while looking
  exactly like a healthy one — the risk register's worst entry.
- **Network blocking.** D-063 settled that the answer key's mitigation is a
  post-hoc transcript audit, not egress control.
- **Stripping the inherited environment.** `docs/notes/fence-notes.md` calls this
  "the sandboxing decision the spec rules out by name."
- **Fixing the rustup breakage the spike found** (open question 1). Real, measured,
  and D-052's to fix.

## Design sketch & open questions

The design is `docs/notes/write-guardrail-notes.md` — it was written from the
spike, not the other way round, and where the two disagreed the spike won twice
(§3.1 and §3.2 each changed the Bash rule). One sentence of it here: **for
`Write`/`Edit`/`MultiEdit`/`NotebookEdit` the destination is a field in the tool
call, so the refusal is enforcement; for `Bash` the hook can only see paths the
command actually names, so the scope is stated intent** — which is exactly right
for a threat that is accident rather than adversary, and is why `cargo build` and
`git commit` still work.

Three questions left open on purpose.

1. **A worker cannot build Rust, and it is not this package's doing.** The Fence
   redirects `HOME`, rustup resolves `$HOME/.rustup`, and a worker's private home
   has no toolchains — measured in `write-guardrail-notes.md` §3.4, with the
   guardrail entirely uninvolved. `fence-notes.md`'s catalogue checked `gh` and
   reasoned about `nvm`; it never checked rustup. This is precisely the "live
   evidence that a worker legitimately needs a tool" D-052 reserved as its own
   reversal condition, and the fix is a Fence decision with its own D-entry
   (seeding or scoping `RUSTUP_HOME`/`CARGO_HOME`), not a root on this allowlist.
   **Recommended:** file it against D-052 before the first improve run, because an
   improve run is a Rust build.
2. **Is writes-only enough?** Asked and answered by the operator, recorded here so
   the answer is not re-derived: no, and deliberately. A pane can still read the
   ledger, past retros and anything else on the disk; WP-12's "Hiding" section is
   honest that only absence enforces. What this buys is that a pane cannot
   *change* anything outside its worktree, which is the half that matters for a
   fleet editing its own platform. **Recommended:** leave it; revisit only if a
   real run shows a read that contaminated a retro.
3. **`/tmp`.** Not on the allowlist; nothing in the spike needed it. If live
   running shows panes reaching for scratch files constantly it is one
   `allow = /tmp` line, which is what the extension point exists for.
   **Recommended:** wait for evidence.

## Session prompt

<!-- Landed. Kept so a session picking up the follow-ups has the frame. -->

```
Read docs/roadmap/17-write-guardrail.md and docs/notes/write-guardrail-notes.md in
full, plus building.md §1 and §9. WP-17 has landed; you are picking up open
question 1 — a worker cannot run `cargo build` under the Fence, because rustup
resolves $HOME/.rustup and the private HOME has no toolchains. That is D-052's to
fix, spike-first (examples/ throwaway, findings appended to
docs/notes/fence-notes.md, version-stamped), with its own decisions.md entry
naming what got seeded or scoped and why. Do not widen the write guardrail's
allowlist to work around it — the guardrail never sees a `cargo build` at all.
```

## Session exit checklist

- [x] `decisions.md` entry (D-065), citing D-052 and D-062.
- [x] No verb was added or changed; `brief-cost` is 0.
- [x] Spike notes in `docs/notes/write-guardrail-notes.md`, version-stamped, with
      a `docs/README.md` index row.
- [x] `docs/runtime-layout.md` updated — the tree gained `guardrail.jsonl` and two
      files per pane config dir.
- [x] This doc: status → landed, "How it landed" appended.
- [x] `00-index.md` status column updated.

## How it landed

Three arms, in the order the risk demanded. The gate first: WP-12 listed
`PreToolUse` interception of `Bash` as unverified, so nothing was built until a
real `claude -p` proved a deny both stops the tool call and reaches the model as
words it can act on — and, in the strong form, that it beats an explicit
`--allowedTools`. Then the scanner against 49 real commands. Then the allowlist
spike, which is the one that earned its keep: it found that a worker's `git
commit` writes eight files into the target repo's `.git`, so a guardrail built as
a real write sandbox would have wedged every worker on `fleet done`'s first step —
and that `cargo build` writes into `~/.cargo` while naming nothing at all. Both
findings pushed the Bash rule to **stated intent**, and both are now tests.

The allowlist ended up exactly as the operator specified it, which is worth
recording: nothing was added because of the spike, because every write found
outside the roots was one no command named, so no extra root would have changed
anything. The one addition inside the roots is a subtraction —
`_shell/pane-config/` is refused, because a pane that can edit its own
`settings.json` can switch the guardrail off between two tool calls.

The configuration went to `prompts/launch.conf`'s `[fence]` section rather than
`~/.fleetor/config.json`, following D-052's own stated intent for that key ("so a
future one has somewhere to land without a new setting surface"): `launch.conf` is
the hand-edited, documented, override-once-at-bootstrap file, while `config.json`
holds app state the app itself writes. No third mechanism was invented.

One thing the spike found and this package deliberately did not fix: a worker
cannot build Rust at all under the Fence, for reasons that predate this work by a
package. It is open question 1, and it belongs to D-052.
