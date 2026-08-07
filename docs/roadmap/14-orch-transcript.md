# WP-14 — `orch`'s own transcript and config dir

status: landed size: M
depends-on: — blocks: 15, 17
brief-cost: 0 — touches no `prompts/*`

## Outcome

The pane that makes every decision stops being the one pane nobody can read. `orch`'s
Claude Code session transcript moves inside `~/.fleetor` and is archived with the run
exactly like a worker's, so an evaluator asking *why* a run went the way it did gets the
orchestrator's reasoning and not only its half of the conversation. The same change gives
`orch` a directory this app is allowed to write policy into — which is where WP-17's deny
rules and hooks will live, and the reason this package moved ahead of the evaluator.

## Performance criteria

### Technical

- [x] `cargo test --workspace` and `cargo test --manifest-path src-tauri/Cargo.toml` green.
- [x] `npx tsc --noEmit && npx vite build` clean.
- [x] A test proves an archived run holds `transcripts/orch/` alongside `transcripts/worker-N/`,
      and that `orch`'s session was **moved**, not copied.
- [x] A test proves `orch` carries a fleet-owned `CLAUDE_CONFIG_DIR` **and** the variable
      that keeps its login — the pair, because either alone is the silent-wedge failure.
- [x] A test proves `orch` acquires none of the worker posture: no private `HOME`, no
      `ANTHROPIC_*`, no `--permission-mode`.
- [x] The L1/L2 pinned tests (`a_virgin_config_dir_gets_the_two_keys…`,
      `a_worker_carries_the_auth_token_and_never_the_api_key`) unchanged and green.
- [x] `rotate` still cannot fail a boot — no new failure path was added to it, because no
      code in `runs.rs` changed.
- [x] `rm -rf ~/.fleetor` still leaves the target repo untouched: everything added is
      `~/.fleetor/_shell/pane-config/orch/`.

### Semantic

- [x] `orch` is still the operator's own `claude` — same login, same plan, same `HOME`,
      same model, same permission posture. Verified against a real interactive pane, not
      inferred.
- [x] Nothing reads, copies, links or writes into the operator's own `CLAUDE_CONFIG_DIR`.
      D-059 refused that and it stays refused.
- [ ] (unverified — see "How it landed") `orch` reaching a prompt *inside the running app*.
      Verified as a standalone pty spawn with the identical environment; not yet seen in a
      real `npm run tauri dev` fleet.

## Invariant guardrails

**Tier 1.1 (repo boundary).** Everything this adds is one directory under
`~/.fleetor/_shell/`. `rm -rf ~/.fleetor` still removes all of it, and `_shell` is the
disposable half — a deleted orch config dir costs one re-seed on the next launch, which is
automatic.

**Tier 1.2 (every agent is a real, unmodified Claude Code process).** `orch` is still
`claude`, launched the same way, with two more environment variables. Nothing is patched,
wrapped or intercepted.

**Tier 1.3 (every pane is a real interactive TUI).** The thing that would break this
invariant is precisely the failure this package had to design around: a pane that reaches
its input box with no credential is a TUI in appearance only. See the spike.

**D-030's "orch is untouched" and D-052's asymmetry.** This is the first deliberate
narrowing of "untouched", and it is narrow on purpose: the config dir, and nothing else.
The Fence stays worker-only. `augmented_path()` still reads the operator's real `HOME`.
`orch` still spawns without `--permission-mode`. A test asserts each of those, because the
cheapest way to break this package is to make `orch` slightly more like a worker.

**Tier 1.4 (nothing in the message path).** Untouched — this is spawn-time and
rotation-time only, and rotation already ran before any pane or socket exists.

## Current state (verified 2026-08-07 — do not re-explore)

- `src-tauri/src/spawn.rs:70` — `ENV_CC_SECURESTORAGE_DIR`, the keychain-namespace variable.
- `src-tauri/src/spawn.rs:109` — `orch_command`, now taking `config_dir`.
- `src-tauri/src/spawn.rs:137` — `worker_command`, which `env_remove`s the same variable.
- `src-tauri/src/spawn.rs:305` — `seed_config_dir`, unchanged and now used by both panes.
- `src-tauri/src/fleet.rs:160` — `pane_config_dir(PaneId)`, one root for every pane's dir.
- `src-tauri/src/fleet.rs:351` — the `PaneId::Orch` arm of `spawn_pane`: create cwd, seed
  config dir, notice, spawn.
- `src-tauri/src/fleet.rs:395` — `orch_config_dir_notice`, kept pure so its wording is pinned.
- `src-tauri/src/runs.rs:129` — `rotate`. **Unchanged.**
- `src-tauri/src/runs.rs:226` — `harvest_transcripts`, which already walked `pane-config/*`.
- `src-tauri/src/runs.rs:261` — `write_agent_view`, whose manifest prose named orch's absence.
- `docs/notes/orch-config-dir-notes.md` — the spike this package rests on (CC 2.1.224).
- `docs/notes/context-gauge-notes.md` — the worker transcript path, measured at CC 2.1.223.

## Scope

**In:** a fleet-owned `CLAUDE_CONFIG_DIR` for `orch` at
`~/.fleetor/_shell/pane-config/orch/`, seeded at the spawn site; the keychain variable that
keeps the operator's login; the Activity notice naming the one failure mode; the doc and
manifest prose that said orch's transcript is never archived.

**Out:**

- **Any deny rule or hook.** That is WP-17's package. This one only builds the house.
- **A context gauge for `orch`.** WP-04's gauge divides by `WORKER_WINDOW_TOKENS`, and
  `orch`'s window is the operator's business (D-054). A transcript being archivable does
  not make it meterable.
- **Copying, linking or reading anything out of the operator's config dir.** `orch` gets a
  new directory going forward and starts empty. This is the constraint, not a shortcut.
- **Restoring the operator's memory, skills, subagents or MCP servers to `orch`.** They are
  read from `CLAUDE_CONFIG_DIR` and they are the price of the move — see the D-entry.
- **Detecting a logged-out `orch` from the pty stream.** A scraper watching pane output for
  a string would be new machinery on the pane path for one string in one CC version. The
  notice is the mitigation.

## Design sketch & open questions

**Spiked first, and the spike inverted the design.** The obvious implementation — set
`CLAUDE_CONFIG_DIR`, seed it, done — **logs `orch` out**, and does so invisibly: the pane
reaches its input box, so every `fleet send` reports `accepted` and only the model's turns
fail. `claude` namespaces its macOS Keychain service name by a hash of the config dir, so a
new directory is a new, empty login. `docs/notes/orch-config-dir-notes.md` reads the
derivation out of the CC 2.1.224 binary and measures four arms of a real interactive pane.

The escape hatch is `CLAUDE_SECURESTORAGE_CONFIG_DIR`, **defined and empty**, which selects
the unsuffixed service name — the entry the operator's own `/login` already wrote. It moves
the credential *lookup*, nothing else: `claude` performs the identical keychain read it
performs today, and FLEETOR opens nothing of the operator's.

*Decided by this session:* the directory is **persistent and per-pane**, not per-run, even
though WP-12 said "a per-run `CLAUDE_CONFIG_DIR`". A per-run directory would re-onboard
`orch` every launch and throw away the `machineID`/`userID` continuity a worker's dir keeps.
The property WP-12 actually wants — each run's transcripts belong to that run — is already
delivered by rotation *moving* them out, exactly as it does for a worker.

*Decided by this session:* it lives at `_shell/pane-config/orch/`, a sibling of
`pane-config/worker-N/`, rather than somewhere operator-facing like `~/.fleetor/prompts/`.
Two reasons: `harvest_transcripts` already walks that root, so archiving cost zero lines;
and the contents are fleet-owned policy plus a disposable session cache, which is what
`_shell` is for.

*Open, and WP-17's to answer:* whether the deny rules go in
`pane-config/orch/settings.json` or arrive via `--settings`. This package deliberately
writes neither.

## Session prompt

```
Read docs/roadmap/14-orch-transcript.md in full, plus building.md §1 and §9, and
docs/notes/orch-config-dir-notes.md — the spike wins over anything in this doc that
disagrees with it (building.md §4). "Current state" is verified; do not re-explore.

Implement WP-14: orch gets its own CLAUDE_CONFIG_DIR under ~/.fleetor, seeded so it keeps
the operator's login and behaviour, and its transcript is archived at rotation like a
worker's.

Do NOT implement deny rules or hooks — that is WP-17. Do not reach into the operator's
own CLAUDE_CONFIG_DIR for anything. Do not let orch acquire worker isolation: no private
HOME, no ANTHROPIC_* override, no --permission-mode. If closing this gap turns out to
require touching the operator's config dir or changing orch's auth or HOME, stop and
report — that is a Tier 1 conflict.

Exit: the checklist at the bottom of this doc.
```

## Session exit checklist

- [x] `decisions.md` entry for every Tier 2 default that moved (D-062, cited in the commit subject).
- [x] If a verb was added or changed: N/A — this package adds no `fleet` verb.
- [x] Spike notes committed to `docs/notes/orch-config-dir-notes.md`, version-stamped, with a `docs/README.md` index row.
- [x] As-built docs updated in the same PR: `docs/runtime-layout.md`, `docs/context-architecture.md`, `docs/context-injection-flow.md`.
- [x] This doc: status → landed, "How it landed" appended.
- [x] `00-index.md` status column updated — the last act of the session.

## How it landed

One commit. The implementation is small — three product files, about forty lines — because
the spike did the work.

**The spike found the trap this package existed to walk into.** `CLAUDE_CONFIG_DIR` alone
reaches a prompt and is not logged in: `Not logged in · Run /login` on the mode line and
`API Usage Billing` where the control arm says `Claude Max`. That is the risk-register
failure verbatim — a pane that looks healthy while every send into it is accepted and every
turn fails. Reading the credential-service derivation out of the CC binary turned that from
a blocker into a two-line fix, and it is why `orch_command` sets two variables that must
move together.

**Rotation needed no change at all.** `harvest_transcripts` walks `_shell/pane-config/*`
and files each directory by name, so putting `orch`'s config dir at `pane-config/orch`
made the archive include it without a line of code in `runs.rs`. Only prose changed there:
the doc comment and the manifest string that told a cold reader `orch`'s transcript was
elsewhere. The `pane_config_dir(PaneId)` rename is the whole structural change —
`worker_config_dir(slot)` became a function of the pane, and both callers use it.

**Verified.** 102 shell unit tests + 11 pty tests, 195 workspace tests, `tsc --noEmit &&
vite build` clean. Four spike arms against a real interactive `claude` 2.1.224, three of
them free; one print-mode turn on the smallest model proved the transcript lands at
`<config dir>/projects/<slug>/<uuid>.jsonl`, which is character-for-character the shape
rotation walks.

**What the operator loses, stated plainly:** `orch` no longer reads their user `CLAUDE.md`,
skills, subagents, MCP servers, settings or hooks — those come from `CLAUDE_CONFIG_DIR`,
and `orch`'s is now a fleet-owned directory holding two onboarding keys. D-062 records why
that was accepted rather than worked around, and the two ways to reverse it.

**Not verified: `orch` has not been seen reaching a prompt inside the running app.** The
environment it will get was spawned by hand in a pty and reached `Claude Max` and an input
box; no fleet has been started since. The first launch is also the first look, and it is
also the launch that will produce the first archived run containing `transcripts/orch/`.
