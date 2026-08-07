# `~/.fleetor` — the runtime layout

Everything FLEETOR writes at runtime lives here, and nowhere else — that is Tier 1.1: `rm -rf ~/.fleetor && git worktree prune` leaves the target repo untouched, minus kept feature branches. This map gathers what was previously spread across the README, `prompts/README.md`, `docs/notes/fence-notes.md` and half a dozen D-entries.

```
~/.fleetor/
  config.json        the operator's target repo, if one is configured
  testbed/           the embedded fallback project (logstat), materialized by
                     src-tauri/src/testbed.rs when no target is set
  runs/              PAST RUNS, the long-term memory (WP-11, D-058). One directory
    index.json       per archived run, each holding a frozen single-file state.db
    <timestamp>/     nothing appends to. index.json carries the operator's labels
                     and is a CACHE — delete it and list() rebuilds everything but
                     the names. A sibling of _shell/ rather than a child, because
                     _shell is disposable and this is the part worth keeping.
  prompts/           OPERATOR OVERRIDES for the shipped prompt files — same names as
                     the repo's prompts/ (orch.md, worker.md, the fragments,
                     launch.conf). Read once at fleet start; absent files fall back
                     to the compiled-in copies. prompts/README.md has the rules.
  _shell/            everything the running fleet owns
    fleet.sock         the unix socket the `fleet` CLI dials (fleetor-ipc)
    state.db           the event log of the LIVE run only — one SQLite table,
                       `events`, WAL mode. Archived to `runs/` at the next start
    run.json           what the live run is: when it started, what it points at
    panes.pids         the orphan ledger: spawned pids, swept at next startup
                       (src-tauri/src/orphans.rs)
    pane-config/       one CLAUDE_CONFIG_DIR per worker: the seeded .claude.json
      worker-N/        that gets a pane past onboarding, and projects/<cwd-slug>/
                       <session>.jsonl — the transcript the context gauge reads
                       (docs/notes/context-gauge-notes.md)
    worktrees/         one git worktree per worker slot, branches fleet/worker-N;
                       reviewed work merges to fleet/integration, never trunk (D-050)
    home/              the Fence (D-052): a private HOME per worker
      worker-N/        so ~/.ssh and the operator's dotfiles stop resolving by
                       name; exactly one file seeded — .gitconfig, because
                       receipts made worker commits load-bearing
                       (docs/notes/fence-notes.md)
```

Points that bite:

- **The orchestrator is exempt from most of this.** It runs as the operator's own `claude` — their login, their config, their HOME — in the target repo (or the testbed). Only workers get `pane-config/`, `home/` and worktrees; that asymmetry is the product, not an accident.
- **Workers must not see `ANTHROPIC_API_KEY`.** They talk to DeepSeek via `ANTHROPIC_BASE_URL` with the key from the repo-root `.env`; the spawn env is curated in `src-tauri/src/spawn.rs`, and the three env settings there each wedge a pane forever if wrong (`prompts/README.md` names them).
- **`prompts/` overrides take effect at the next fleet start**, not live — they are read once at bootstrap (D-042).
- **The Fence is a fence, not a sandbox** (D-052): name resolution stops; absolute paths, `SSH_AUTH_SOCK` and inherited PATH entries do not. `docs/notes/fence-notes.md` carries the breakage catalogue.
- **Nothing under `_shell/` is precious — but `runs/` is** (D-058). `_shell` holds only the live run and the machinery around it, and a deleted `~/.fleetor` costs history, never correctness of the target repo. `runs/` is where that history now accumulates, so it is the one directory here worth backing up.
- **Never copy `state.db` on its own.** Between runs it is typically a few KB while the whole log sits in an uncheckpointed `state.db-wal`; a backup that took the one file would capture an empty database. `docs/notes/run-rotation-notes.md` measures this — it is why archiving leaves WAL mode first.
