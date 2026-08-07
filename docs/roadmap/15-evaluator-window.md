# WP-15 — The evaluator window

status: landed size: L
depends-on: 13, 16 · soft 14 blocks: the proposal ledger
brief-cost: **0** — and that is a requirement rather than an accident. Nothing under
`prompts/` moved; the evaluator's brief is not a `prompts/` file and never will be.

## Outcome

The loop closes. `orch` calls `fleet handoff` believing it is reporting to the operator,
and a real `claude` it has never heard of wakes in a window of its own, reads everything
the run produced — the log, every pane's transcript including `orch`'s, and an answer key
that did not exist on disk a second earlier — and walks `orch` back through its own
decisions. Everything built in this arc so far was scaffolding for this: WP-13 made the
signal, WP-14 made `orch`'s reasoning readable, WP-16 made the container, WP-17 made it
safe to point a fleet at this repo. This is the first package where it becomes clear
whether a retro is genuinely useful or merely plausible-sounding.

The property that makes it worth having is an asymmetry, not a second opinion: **the
evaluator holds a correct answer `orch` has never seen.** A run that is not on a rewind
mission therefore gets no evaluator at all, and says so — a retro with no ground truth
converges on congratulation and teaches nothing (D-060).

## Performance criteria

### Technical

- [x] `cargo test --workspace` green — **213**, up from 210 (three new `pane.rs` tests; no
      other functional diff in `crates/`).
- [x] `cargo test --manifest-path src-tauri/Cargo.toml` green — **165**, up from 143.
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --features devmode` green — 165 too.
- [x] `cargo build --features devmode` succeeds against the real brief, and a build whose
      harness path is wrong **fails loudly naming the absolute path** rather than
      substituting anything (verified with `FLEETOR_EVAL_HARNESS=/nonexistent`).
- [x] `strings` on a default build finds **0** occurrences of the brief's version stamp;
      on a `devmode` build, 2. The default binary carries no evaluator prose.
- [x] `npx tsc --noEmit && npx vite build` green.
- [x] `crates/fleetor-server/tests/handoff.rs::a_handoff_asks_the_app_for_nothing_and_changes_no_later_send`
      is **untouched and still passes**. The wake does not move the `AppCommand` count.
- [x] `src-tauri/tests/dev_mode.rs::no_pane_is_briefed_about_the_evaluator` still passes,
      and gained `prompts/scaffolding.md` — a fragment composed into *both* briefs that
      nobody had been checking. One file left its list; see "The one test whose letter
      changed" below, which is the most important paragraph in this document.
- [x] The evaluator's brief has no `~/.fleetor/prompts/` override, pinned at the resolver
      and stated in `prompts/README.md`.
- [x] The second window's terminal is never conditionally rendered, and closing the window
      hides it rather than destroying its buffer.

### Semantic

- [x] **The veil holds at first contact.** `orch` is not told the evaluator exists, cannot
      find it in `fleet roster`, and never receives a broadcast leg from it. It learns the
      name when a message arrives from it — which is the intended experience.
- [x] **And it is not a wall.** `orch` can answer. `fleet reply` reaches the evaluator with
      no new code, because the first accepted delivery sets `last_inbound_from[orch]`.
- [x] The evaluator is a real interactive `claude` on a real pty (Tier 1.3) — typeable,
      plan mode, skills, all of it.
- [x] Outside dev mode the evaluator does not exist: no window, no pane, no brief, and no
      spawn path that can be reached.
- [x] A wake that does nothing always says why, unless the operator turned the mode off.

## Invariant guardrails

### Tier 1.4 — nothing in the message path. Held structurally, and one doc sentence had to be corrected.

The wake is a **second subscriber on the event bus D-020 built for downstream readers**.
`Hub::handoff` is byte-identical: still not `async`, still never touches `self.app`, still
answers `recorded` from its own append alone. The wake sends **no `AppCommand`**; it reaches
the registry through `spawn_pane`, the road `pty_spawn` takes, which no message travels.
The bus is persist-then-publish, so the handoff op is durable and answered whether or not
the wake ever runs, `broadcast::Sender::send` never blocks its publisher, and a lagging
follower recovers from the database. Nothing waits on any of it.

**The honest part.** WP-15 is the first thing that reads a handoff back, and
`Hub::handoff`'s own doc claimed *nothing anywhere* did. That sentence is now narrowed to
*nothing on the message path* — through the process, not silently. What `task.rs`'s
tripwire list actually bars is a read that goes on to **permit, order or refuse**
something; this permits nothing, orders nothing and refuses nothing. It changes what
*exists* — a window, an address — which is precisely the shape `16-dev-mode.md` named as
allowed and WP-19 is held to. `handoff.rs` is untouched and green.

### Tier 1.3 — a real interactive TUI, and §7 rule 5 twice over.

The evaluator is `claude` on a pty, spawned through the same `PaneRegistry::spawn` every
other terminal uses. §7 rule 5 now applies at two levels. Inside the window, the one
terminal is rendered unconditionally — no gate, no ternary — for the window's life. At the
window level, **closing is intercepted and turned into a hide**: destroying the webview
destroys the xterm and its scrollback, and there is no screen replay behind a pty, so a
reopened window would come back blank over a terminal that is still alive.

That handler is also where the second window's one real hazard lived. `on_window_event`
tore the **whole fleet** down on any `CloseRequested` — correct with one window, a six-pty
kill with two.

### Tier 1.5 — three outcome words, and the evaluator takes the honest one.

A message to the evaluator is `accepted`, because there is a live pty and bytes reached it.
This is the exact inverse of `PaneId::Operator`, and putting the two side by side is what
makes both legible: the human is **in the roster listing and has no terminal**; the
evaluator **has a terminal and is in no listing**. `has_pty()` and `is_fleet_member()` are
two questions that disagree in both directions, which is why neither is derived from the
other.

### Tier 1.7 — auto-approve. Narrowed, not widened, and said plainly so it is not re-litigated.

The evaluator spawns `--permission-mode auto`, which `orch` does not get. §9.2's escalation
trigger is *widening* auto-approve beyond a worker's worktree; this does not widen it. Its
write guardrail roots are **its own working directory alone** — not `_shell`, not the
operator's `[fence] allow` extras — so what auto-approve can actually change here is a
strict subset of what any worker's already could. The alternative was `manual`, which would
park the pane on its first `Read` of a run archive while looking exactly like a healthy
pane: the risk register's worst entry. Reads are open for it as for every pane, because the
hook is not registered for `Read` at all (D-065).

### D-030's regrowth warning — counted, not waved away.

Nine files outside this package's own, two of them core contracts (`pane.rs`, and the TS
mirror that moves with it). That is inside the size D-030 warned about and it is worth
stating rather than hiding: the arc doc pre-approved exactly this touch (WP-12's stream
table marks WP-15 "core? **yes — 2nd window, `PaneId`**"), and unlike the abandoned coach
pane this is product behind a mode the operator can see, not a test rig presenting itself
as one. The single largest lever against sprawl held: `crates/fleetor-server` and
`crates/fleetor-cli` have **no functional diff at all** — one corrected doc comment
between them.

## The one test whose letter changed — read this before restoring it

`src-tauri/tests/dev_mode.rs::no_pane_is_briefed_about_the_evaluator` grepped seven files
for `evaluat`. One of them was `crates/fleetor-core/src/pane.rs`, and **adding a `PaneId`
variant named `Evaluator` fails it on day one.**

The variant is forced. The retro is a two-way, logged conversation, and every name in this
system's record is a `PaneId`: the CLI parses one before the socket, `Hello` declares one,
`FleetEvent::Message` carries one from and to, and `fleet reply` resolves through a
`HashMap<PaneId, PaneId>`. The two ways around it are worse in kind rather than in degree —
a spelling chosen to slip past the grep is the workaround `building.md` §6 bans by name,
and reusing `operator` would put a lie in the log about who said what (D-051).

So `pane.rs` came off **that one test's** list and
`nothing_a_pane_can_read_ever_names_the_evaluator` took its place. It asserts the property
the grep was a proxy for — not "the source does not contain the word" but "no pane is ever
shown it" — over the four things that are a pane's whole world:

1. the **rendered** orchestrator brief, `{workers}` included;
2. a **rendered** worker brief, `{peers}` included;
3. `PaneId::roster()`, which is what spawns, what a broadcast fans out over, and what a
   peer list is built from;
4. the parse error a mistyped pane name produces — the one place a name nobody was briefed
   on leaks by accident, because a model reads its own stderr and acts on it.

**This is strictly more coverage, not less.** `pane.rs` could always have rendered a name
it never spelled literally and the grep would have passed. `pane.rs` stays on the list for
`no_pane_is_briefed_about_dev_mode`, which is untouched, and the prose files gained
`prompts/scaffolding.md`. Net: seven greps became seven greps plus four behavioural
assertions.

## Current state (verified 2026-08-07 — do not re-explore)

Every anchor below was read or run in the session that landed this.

**The identity**

| Anchor | What is there |
|---|---|
| `crates/fleetor-core/src/pane.rs:50` | `PaneId::Evaluator` — sorts last, because it appears in no enumeration |
| `crates/fleetor-core/src/pane.rs:89` | `is_fleet_member()` — **the whole veil in one predicate.** Deliberately not derived from `has_pty()` |
| `crates/fleetor-core/src/pane.rs:98` | `ParsePaneIdError::fmt` — the fleet's complete name list, and the comment saying not to add to it |
| `ui/src/fleet/types.ts:21` | `EVALUATOR`, the TS mirror, with the operator asymmetry stated |
| `ui/src/fleet/types.ts:46` | `paneKey` — rewritten to derive from `paneSlot`, so it cannot drift from the Rust half |

**The pty**

| Anchor | What is there |
|---|---|
| `src-tauri/src/pty.rs:293` | `channel_key` — now exhaustive on the variant. It used to `match pane.slot()`, giving **every** slotless name `orch`'s channel; a second one would have rendered its bytes in the orchestrator's terminal with no error |
| `src-tauri/src/pty.rs:244` | `roster()`'s one-line filter — the single place the fleet gets enumerated, feeding both `fleet roster`'s listing and a broadcast's legs |
| `src-tauri/src/pty.rs:466` | `a_running_evaluator_is_not_on_the_fleets_roster` — driven through a real spawn, and asserts it is still `writable` |

**Whether there is one, and what it is told**

| Anchor | What is there |
|---|---|
| `src-tauri/build.rs:68` | the panic. Names the absolute path, says there is no fallback and why, and gives two ways out |
| `src-tauri/src/evaluator.rs:39`, `:41` | `BRIEF` — `Some(include_str!(OUT_DIR/…))` under `devmode`, `None` otherwise. **The whole of "a default build has no evaluator"** |
| `src-tauri/src/evaluator.rs:51` | `OVERRIDE_REFUSED` — the exclusion as a thing in the source rather than a thing nobody did |
| `src-tauri/src/evaluator.rs:116` | `readiness` — the three conditions, each failing with its own reason |
| `src-tauri/src/evaluator.rs:137` | `mission_for` — derived from the target path, never configured |
| `src-tauri/src/evaluator.rs:169` | `render_brief` — a template missing a placeholder is refused, not rendered |
| `src-tauri/src/evaluator.rs:220` | `config_dir` — outside `pane-config/`, so the evaluator's own reasoning never lands in the archive the next generation reads |
| `src-tauri/src/evaluator.rs:238` | `reveal_answer_key` — before the pane, and an existing key is success |

**The wake**

| Anchor | What is there |
|---|---|
| `src-tauri/src/fleet.rs:549` | `spawn_evaluator_wake` — a follower of its own, so a slow wake cannot hold up the feed |
| `src-tauri/src/fleet.rs:584` | `wake_evaluator` — one wake, and what the operator is told in each case |
| `src-tauri/src/fleet.rs:632` | `open_evaluator_window` — built here or not at all; `show()` for one that exists |
| `src-tauri/src/fleet.rs:383` | `spawn_pane`'s arm — re-checks the brief, so "unreachable outside dev mode" is a property of the spawn site and not of its one caller |
| `src-tauri/src/fleet.rs:450` | the narrower guardrail roots |
| `src-tauri/src/lib.rs:115` | the close handler's label branch |
| `src-tauri/src/spawn.rs:156` | `evaluator_command` |

**Reading a live run**

| Anchor | What is there |
|---|---|
| `docs/notes/live-run-snapshot-notes.md` | the measurement, and the limit it cannot close |
| `src-tauri/src/runs.rs:287` | `snapshot_live_run` — `to_json` in place, never `freeze` |
| `src-tauri/src/runs.rs:332` | `copy_transcripts` — `harvest_transcripts`'s non-destructive twin, deliberately not a flag on it |
| `src-tauri/src/runs.rs:364` | `live_run_id` — the name the run *will* be archived under |

**The tests**

| Anchor | What is there |
|---|---|
| `src-tauri/tests/evaluator.rs` | eight: Tier 1.4 both directions, the wake's signature, the override refusal, no brief in this repo, §7 rule 5 at both levels, the default build |
| `src-tauri/tests/dev_mode.rs` | four: the two greps, and the behavioural veil pin that replaced `pane.rs`'s row |

## Scope

**In:** the identity and its veil; the second window and its lifecycle; the wake; the
live-run snapshot; the `devmode` feature and the build-time brief; the answer-key reveal;
the narrower guardrail roots; the tests.

**Out**, each considered and rejected rather than forgotten:

- **The proposal ledger.** WP-12 files it under this stream and says to build it *after*,
  since it only has content once retros produce proposals. Building it now would be
  guessing at fields.
- **A control to open the evaluator window by hand.** The wake is the trigger, deliberately
  — a button would make the retro something the operator starts rather than something
  `orch`'s own declaration causes, and the sequencing is the design.
- **Re-waking on a second handoff.** `orch` finding more work and handing back again is a
  real sequence (D-064 refuses to argue with it). It gets the evaluator that is already
  there, not a second one; the snapshot is rebuilt.
- **Recording the mode on the run manifest** (WP-16's open question). Still open. The
  snapshot's manifest records that the run is live, which is the field this package
  actually needed.
- **Anything in the `fleet` CLI.** It parses a `PaneId` and the hub answers; there is no
  arm to add, and the fact that there is none is asserted.
- **Killing the evaluator's pty when its window hides.** It is in the same registry as
  every other pane, so `kill_all` and `orphans.rs` already cover it — a separate registry
  would reopen the leaked-Opus money bug.

## Design sketch & open questions

**Why a `PaneId` variant** — see "The one test whose letter changed". It is forced by the
record's own types, and the alternatives are a lie or an obfuscation.

**Why the wake is a bus subscriber** — the alternatives were the hub notifying the app
(moves the `AppCommand` count, banned), triggering from the webview's `fleet://event`
listener (backend policy in a reloadable webview), or polling the database (the follower,
minus its latency). D-020 built the bus so appends could have downstream readers without
touching either loop.

**Why the snapshot rather than pointing at `_shell`** — measured, in
`docs/notes/live-run-snapshot-notes.md`. Pointing a model with Bash at `_shell` hands it
the live writer's database (`sqlite3` opens read-write by default) and the fleet's open
session files, and makes every citation the brief demands drift as the run keeps appending.
Deferring the retro to the next launch is not a smaller version of the feature — the panes
are dead by then, and the retro is a conversation with a live `orch`.

**Open question 1 — is `auto` right for the evaluator?** Recorded rather than left implicit,
because it is the one call in this package that is Tier-1-adjacent. The argument is in
"Invariant guardrails" above. **Recommended:** leave it; the reversal is one word in
`spawn::evaluator_command` and it costs the operator a lot of clicking.

**Open question 2 — the retro directory accumulates.** `~/.fleetor/dev/retro/<run-id>/`
holds a copy of the run's transcripts, and nothing prunes it. Rotation does not clear it
(it is outside `_shell` on purpose). **Recommended:** wait for a real cycle to say whether
the sizes matter; the answer when it bites is the same one D-058 reserved for `runs/` — a
size column and an operator-driven delete before automatic expiry.

**Open question 3 — a mission is derived from the target path, so a workspace moved
elsewhere gets no evaluator.** That is the honest failure (it says so on the feed), but it
means `FLEETOR_EVAL_WORKSPACES` has to match between the harness and the app.
**Recommended:** leave it. A second place to write the mission name down is a second place
for it to be wrong, and the app already reads the same env var the harness does.

## Session exit checklist

- [x] `decisions.md` entry — **D-066**.
- [x] No verb was added or changed; `VERBS`, the clap enum and every `prompts/*.md` template
      are untouched, so `brief-cost` is 0.
- [x] Spike notes in `docs/notes/live-run-snapshot-notes.md`, version-stamped, with a
      `docs/README.md` index row.
- [x] As-built docs: `docs/fleet-comms-map.md` and `docs/runtime-layout.md` both updated.
- [x] This doc: status → landed.
- [x] `docs/README.md` and `00-index.md`'s status column updated.

## How it landed

Close to the sketch, with four things that were not in it.

**The channel-key collision was a live bug, not a new-variant problem.**
`channel_key` matched on `slot()`, so `PaneId::Operator` already resolved to `orch`'s
channel — harmless only because nothing spawns the operator. Any second slotless identity
would have rendered its bytes in the orchestrator's terminal, and there is no failure
signal for listening on the wrong name.

**The close handler was the second one.** `on_window_event` tore the whole fleet down for
any window's close. With one window that was right; the moment there are two it is a
six-pty kill on closing the wrong one.

**The capability file scopes by window label**, and a window missing from it gets no
`invoke` and no `listen` at all — a terminal that can never spawn or receive a byte, with
no error to say why. Both the label and the reason are now in that file.

**The evaluator's config dir had to leave `pane-config/`.** `harvest_transcripts` walks
that directory and files what it finds into `runs/<id>/transcripts/`. An evaluator housed
there would archive its own session — its reasoning, its rubric applied, its verdict — into
the run archive that the *next* generation's fleet reads. That is "past retros", one of the
three things WP-12 requires to be absent, and it would have arrived by a code path nobody
wrote.

**Not verified, stated plainly:** no fleet has been started since this landed, so the
window has never been seen on screen, no evaluator has ever reached a prompt, and no retro
has happened. Everything above is tests, a spike and a build. That is the same honest gap
D-062 and D-065 record, one package larger — and the first live run is what open question 1
and the whole "is a retro useful or merely plausible" question actually rest on.
