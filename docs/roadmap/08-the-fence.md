# WP-08 — The Fence: private worker HOME

status: **landed** (2026-08-06, D-052) size: S/M
depends-on: 01 blocks: — (sequencing note with 06 below)
brief-cost: 0

## Outcome

Each worker gets a private `HOME` at `~/.fleetor/_shell/home/worker-N`, and the PATH built from the operator's HOME is fixed in the same commit. `~/.ssh`, the operator's real Claude config, and shell profiles stop being reachable *by name*. Imported from the autonomy build order (its #1 recommendation) because this roadmap raises the stakes: a command channel, receipts that run arbitrary check commands, and a merge flow all widen what a misbehaving worker can do. Default posture stays `open` (reproduces today's behavior apart from the private HOME) — deny rules wait for live evidence.

## Performance criteria

### Technical
- [ ] `worker_command` sets `HOME` to the private dir (created on spawn); orch is untouched — it is the operator's own agent.
- [ ] `augmented_path()` fixed in the same commit: today it bakes `~/.local/bin` and `~/.bun/bin` from the **app's** HOME (`spawn.rs:137-145`) — decide each entry deliberately (keep system dirs; drop or re-point operator-HOME dirs).
- [ ] Seeded minimal gitconfig in the private HOME (`user.name "fleet worker-N"`, an email, `init.defaultBranch` irrelevant) so worker commits keep working — the WP-06 interaction.
- [ ] The design's falsification test, run live once and recorded: ask a pane to read `~/.ssh` — it must fail *by path resolution*, and the finding goes in the notes doc.
- [ ] Breakage catalogue: `gh`, `git`, `nvm`-style tools that read HOME — what broke, what was seeded, what was deliberately left broken (`docs/notes/fence-notes.md`).
- [ ] Fake-pane tests green; L1/L2 seeding (`seed_config_dir`, `ANTHROPIC_API_KEY` removal) unaffected — `CLAUDE_CONFIG_DIR` already points elsewhere and stays authoritative.

### Semantic
- The fence is about *names and paths*, not OS sandboxing — an honest boundary honestly described. The notes doc says exactly what it does and does not stop.

## Invariant guardrails

- **Tier 1.7 adjacent:** this narrows ambient reach; it must never *widen* anything.
- **No OS-level sandboxing, no Posture Ladder** (rejected upstream), **no per-seat model changes** (that is the Seats design, not this).
- **Tier 1.1:** the private homes live under `~/.fleetor/` — `rm -rf ~/.fleetor` still removes everything FLEETOR made.

## Current state (verified 2026-08-06 — do not re-explore)

- `spawn.rs` never sets `HOME` today; workers inherit the operator's entire HOME. `augmented_path()` at `spawn.rs:137-145` reads `$HOME` at app runtime. Worker env assembly: `worker_command` `:77-105` (isolated `CLAUDE_CONFIG_DIR`, DeepSeek base URL/model/token, `env_remove`s at `:101-103`), `apply_pane_env` `:123-133`.
- Directory conventions: `~/.fleetor/_shell/{pane-config,worktrees}/worker-N` (`fleet.rs:129,133`) — `home/worker-N` is the natural sibling.
- Setting surface: post-WP-01, `prompts/launch.conf` exists for launch-time knobs; the posture default (`open`) belongs there.
- Sequencing: whichever of WP-06/WP-08 lands second re-validates worker commits (the gitconfig seed is why).

## Scope

### In
Private HOME creation + env wiring; PATH fix; gitconfig seed; falsification test + notes doc; `launch.conf` posture entry; decisions entry.

### Out
Deny rules / network policy; sandbox-exec or containers; orch fencing; model changes.

## Design sketch & open questions

1. **What else to seed.** Recommended: nothing beyond gitconfig — add files only when the breakage catalogue demands, each with a line of why.
2. **PATH entries.** Recommended: keep `/opt/homebrew/bin:/usr/local/bin` and the fleet-bin rung; drop operator-HOME rungs for workers (orch keeps today's PATH).

## Session prompt

```
You are working in /Users/bubblyducks/harness/fleetor. Read
docs/roadmap/08-the-fence.md in full, then building.md
§1 and §9. Execute WP-08: private HOME per worker under
~/.fleetor/_shell/home/worker-N, the augmented_path() operator-HOME fix in
the same commit, a seeded minimal gitconfig, the ~/.ssh falsification
check recorded in docs/notes/fence-notes.md with a breakage catalogue, and the
posture default in prompts/launch.conf. Orch untouched. No OS sandboxing.
Finish with the session exit checklist.
```

## Session exit checklist

- [ ] Full test matrix green (incl. spawn.rs L1/L2 pins).
- [ ] `docs/notes/fence-notes.md` committed with the falsification result.
- [ ] `decisions.md` entry.
- [ ] If WP-06 already landed: worker commit flow re-validated.
- [ ] `00-index.md` status updated.

## How it landed (2026-08-06)

Commit `17034af`, decision D-052, falsification and breakage catalogue in `docs/notes/fence-notes.md` (`~/.ssh` no longer resolves by name; exactly one file seeded, the worker `.gitconfig`, because WP-06 had made worker commits load-bearing). Private HOME per worker at `~/.fleetor/_shell/home/worker-N`; `launch.conf`'s `[fence] posture = open` documents why the key exists with only one value. The checkboxes above were not ticked by the landing session; the D-entry and the notes file are the record.
