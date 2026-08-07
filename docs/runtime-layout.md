# `~/.fleetor` — the runtime layout

Everything FLEETOR writes at runtime lives here, and nowhere else — that is Tier 1.1: `rm -rf ~/.fleetor && git worktree prune` leaves the target repo untouched, minus kept feature branches. This map gathers what was previously spread across the README, `prompts/README.md`, `docs/notes/fence-notes.md` and half a dozen D-entries.

```
~/.fleetor/
  config.json        the operator's target repo, if one is configured
  testbed/           the embedded fallback project (logstat), materialized by
                     src-tauri/src/testbed.rs when no target is set
  runs/              PAST RUNS, the long-term memory (WP-11, D-058/D-059). A
    index.json       sibling of _shell/ rather than a child, because _shell is
                     disposable and this is the part worth keeping. index.json
                     carries the operator's labels and is a CACHE — delete it and
                     list() rebuilds everything but the names.
    <timestamp>/     one directory per archived run, WRITTEN TO BE READ BY AN
      state.db       AGENT: a `cat`/`jq` job, no SQLite and no running app needed
      events.json      the whole log as one JSON array, oldest first; each row is
                       the event as it crossed the wire plus its `seq` and `ts`
      manifest.json    what the run is, what each file here holds, and what the
                       data does NOT cover
      transcripts/     each pane's own Claude Code session .jsonl files, MOVED
        orch/          here at archive time so they do not accumulate in
        worker-N/      pane-config/. `orch` included since WP-14 (D-062)
  dev/               DEV MODE ONLY (WP-15, D-066). Absent unless an evaluator has
                     woken. A SIBLING of _shell/, not a child, and that placement
                     is load-bearing: every pane's write guardrail has _shell as a
                     root, so a retro laid out inside it would be writable by the
                     fleet being judged.
    retro/           the run the evaluator reads, in the archive's own shape
      <run-id>/      named after the directory the run WILL get under runs/
        events.json    the live log, read in place read-only through the same
                       archive::to_json rotation uses — never a copy of state.db,
                       which between checkpoints is a 4 KB file with no schema
        manifest.json  says outright that this is a mid-run snapshot and that the
                       archive under runs/ supersedes it
        transcripts/   COPIED, not moved: every pane is alive and Claude Code
                       still has these files open (docs/notes/live-run-snapshot-notes.md)
    evaluator-config/  the evaluator's CLAUDE_CONFIG_DIR — deliberately NOT under
                       _shell/pane-config/, because harvest_transcripts walks that
                       directory and would file the evaluator's own reasoning into
                       runs/<id>/transcripts/, handing the next generation's fleet
                       the past retros WP-12 requires to be absent
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
    guardrail.jsonl    the write guardrail's refusal journal (WP-17, D-065): one
                       line per refused tool call, drained onto the Activity feed
                       once a second. Emptied at every fleet start — last run's
                       refusals went to runs/ with the rest of its log
    pane-config/       one CLAUDE_CONFIG_DIR per pane: the seeded .claude.json
      orch/            that gets a pane past onboarding, and projects/<cwd-slug>/
      worker-N/        <session>.jsonl — the transcript the context gauge reads
                       (docs/notes/context-gauge-notes.md) and the one rotation
                       archives. orch/ is WP-14 (D-062); it is also where the
                       write guardrail lives, since this app may write policy here
                       and never into the operator's own ~/.claude:
        settings.json    the PreToolUse hook, merged into whatever the operator
                         has already put there
        write-guardrail.py  the decision, rewritten from the binary every spawn
    worktrees/         one git worktree per worker slot, branches fleet/worker-N;
                       reviewed work merges to fleet/integration, never trunk (D-050)
    home/              the Fence (D-052): a private HOME per worker
      worker-N/        so ~/.ssh and the operator's dotfiles stop resolving by
                       name; exactly one file seeded — .gitconfig, because
                       receipts made worker commits load-bearing
                       (docs/notes/fence-notes.md)
```

Points that bite:

- **The orchestrator is exempt from most of this.** It runs as the operator's own `claude` — their login, their model, their HOME — in the target repo (or the testbed). Only workers get `home/` and worktrees; that asymmetry is the product, not an accident. Since WP-14 it shares exactly one thing with them: a `pane-config/` directory of its own.
- **Workers must not see `ANTHROPIC_API_KEY`.** They talk to DeepSeek via `ANTHROPIC_BASE_URL` with the key from the repo-root `.env`; the spawn env is curated in `src-tauri/src/spawn.rs`, and the three env settings there each wedge a pane forever if wrong (`prompts/README.md` names them).
- **`prompts/` overrides take effect at the next fleet start**, not live — they are read once at bootstrap (D-042). **There is exactly one brief with no override here, and it is not in this directory:** the evaluator's (D-066). It is compiled into the binary from a separate repo behind the `devmode` cargo feature, because an override path would put the rubric on disk where every pane can read it by absolute path and a worker's auto-approve could write to it. `prompts/README.md` §"The one brief with no override" is the account.
- **`dev/` is the only thing under `~/.fleetor` a fleet pane cannot write into** (D-066). Every other directory here is inside some pane's guardrail roots — the evaluator's are its own working directory alone, and nobody else's include `dev/`.
- **The Fence is a fence, not a sandbox** (D-052): name resolution stops; absolute paths, `SSH_AUTH_SOCK` and inherited PATH entries do not. `docs/notes/fence-notes.md` carries the breakage catalogue.
- **The write guardrail is about writes only, and about paths a command *names*** (D-065). Each pane may write under its own cwd and `_shell/`, and nowhere else; reads are not restricted at all. What it cannot see is a write no command states — `cargo build` into `~/.cargo`, and a worker's `git commit` into the target repo's `.git`, both measured in `docs/notes/write-guardrail-notes.md` and both of which a pane cannot work without. `pane-config/` is inside a root and still refused, because a pane that edits its own `settings.json` switches the guardrail off.
- **Nothing under `_shell/` is precious — but `runs/` is** (D-058). `_shell` holds only the live run and the machinery around it, and a deleted `~/.fleetor` costs history, never correctness of the target repo. `runs/` is where that history now accumulates, so it is the one directory here worth backing up.
- **`orch`'s transcript is archived with the run** (D-062, closing the gap D-059 named). It used to sit in the operator's own `CLAUDE_CONFIG_DIR` outside `~/.fleetor`, and this app still does not reach in there — instead `orch` was given a directory of its own at `pane-config/orch/`. **Two environment variables make that safe and they move together:** `CLAUDE_CONFIG_DIR` alone would log `orch` out, because `claude` hashes the config dir into its Keychain service name, and the pane would reach its input box with no credential while every `fleet send` reported `accepted`. `CLAUDE_SECURESTORAGE_CONFIG_DIR` set to the *empty string* selects the operator's own entry. `docs/notes/orch-config-dir-notes.md` measures both, at CC 2.1.224.
- **`orch` does not inherit the operator's Claude Code setup any more** (D-062). User `CLAUDE.md`, skills, subagents, MCP servers, settings and hooks are read from `CLAUDE_CONFIG_DIR`, so `orch`'s are now whatever is in `pane-config/orch/` — which this app seeds with two onboarding keys and nothing else. The operator may put their own files there; nothing else writes into it.
- **Never copy `state.db` on its own.** Between runs it is typically a few KB while the whole log sits in an uncheckpointed `state.db-wal`; a backup that took the one file would capture an empty database. `docs/notes/run-rotation-notes.md` measures this — it is why archiving leaves WAL mode first.
