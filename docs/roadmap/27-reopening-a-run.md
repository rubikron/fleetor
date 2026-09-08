# WP-27 — Reopening a run: History becomes a session switcher

status: not-started size: L
depends-on: — blocks: —
brief-cost: 0 — R13 injects nothing into any pane and re-passes no brief, so no file under `prompts/` changes. If that reverses, this number does too.

**Visual guide: https://claude.ai/code/artifact/5b6ff072-faeb-49cf-9372-73bd41f0776c**

Four states drawn to FLEETOR's own tokens — the list, the confirm, landing in the pane view, and the refusal — each annotated with the decision it renders. Where this doc and the mockup disagree about *layout*, the mockup wins; where they disagree about *behaviour*, this doc wins. The five gaps the mockup exposed are in [Open questions](#open-questions), not in it.

> **Numbering.** The parked branch `wp-26-view-resume-past-runs` carries its own `docs/roadmap/26-resume-past-runs.md`. That work is preserved but is explicitly **not** the basis for this design (operator's call, 2026-09-07 — see `decisions.md`, "History redesign"). This doc takes 27 so the two never collide at one number.

## Outcome

A past run stops being something you read about and becomes somewhere you can go back to. Clicking a row in History tears down the fleet you have open, archives it, and brings that run's five panes back as their own real vendor sessions — landing you in the ordinary pane view with its messages, its tasks and its activity already there. The panes pick up mid-thought because the vendor resumed them, not because FLEETOR reconstructed anything.

The capability is a harness answer, not a Claude Code feature. A fifteenth checkpoint says how a harness reopens a session it recorded, and a harness that cannot do so says so explicitly and still registers — so a third TUI is a spec entry, and the runs that used it are honestly marked rather than silently broken.

## Performance criteria

### Technical

- [ ] `cargo test` green across the workspace and the shell; `cargo build` produces no warnings; `npm run build` (`tsc --noEmit && vite build`) clean.
- [ ] A new conformance test walks `harness::registered()` and fails if any harness leaves checkpoint 15 unanswered — mutation-checked by deleting Claude Code's answer and confirming it turns red.
- [ ] A test drives rotation and asserts `manifest.json` carries `panes.<seat>.session_id` for every seat that recorded one, and `panes.<seat>.harness` beside it.
- [ ] A test asserts rotation **copies** rather than moves: after `rotate`, the session file is present in both `pane-config/<run-id>/<seat>/` and `runs/<id>/transcripts/<seat>/`, byte-identical.
- [ ] A test asserts an archived run's directory holds only that run's sessions — write two runs' sessions and confirm run 1's archive does not contain run 2's.
- [ ] A test asserts the reopen refusal (R8) fires from the manifest **before** any teardown: a run with a seat missing `session_id` refuses and the live registry is untouched.
- [ ] A test asserts delete is lineage-scoped (R15): deleting a row that has been reopened twice removes all three `runs/<id>/` directories, their index entries and `pane-config/<root-id>/`, and leaves every archive outside that lineage present.
- [ ] `src-tauri/tests/views.rs`'s rail/restore-list tripwire still passes with whatever the History view becomes.
- [ ] A render test (the `tests/*_probe/render.tsx` + `renderToStaticMarkup` shape already used by `pane_head_renders.rs`) proves a row that cannot be reopened renders its reason and offers no open affordance — a dropped reason fails a test, not a screen.
- [ ] A test asserts the Claude Code trust key is seeded against the **resolved** cwd (R16c) — seed a path through a symlinked parent and confirm the key names the realpath. Without it a reopened pane sits on a trust gate forever.
- [ ] No file under `prompts/` changes; a diff against it is empty.

### Semantic

- [ ] The three feed views need **no past-run-aware code**. If `MessageFeed`, `TaskBoard` or `EventFeed` gains a branch about archives, R1 was not actually used and the design has regressed to the shape it replaced.
- [ ] A reviewer can open a row, work in a resumed pane, and see the conversation continue with no seam — no injected preamble, no re-stated brief, no "you were reopened" line.
- [ ] Every row that cannot be reopened says *why*, in a sentence naming the seat or the harness responsible, and confirms what is still intact.
- [ ] A third harness needs no change to `ui/`, to `runs.rs`, or to any reopen path — only a spec entry. The tripwire for this is the conformance suite, not a promise here.

## Invariant guardrails

**D-058 — a run is a database, physically isolated.** Its stated invariant is that *a new run cannot be corrupted by an old one*. R1 copies a frozen `state.db` into the live slot: the parent is read, never opened for writing, so the invariant holds. What R1 gives up is the weaker "each log holds only its own events," which D-058 never claimed. **Allowed shape:** copy on the way in; the archive is never reopened for append.

**D-059 — transcripts are moved so archives don't accumulate their predecessors.** R3 reverses the *mechanism* and keeps the *goal*: rotation copies instead of moving, and R4's per-run seat directory is what keeps an archive to one run's sessions. **Allowed shape:** selection by directory. **Forbidden:** copying a flat seat directory wholesale, which is the accumulation bug D-059 exists to prevent.

**D-030 — worker transcript reconstruction is abandoned outright.** That refusal stands untouched. It forbids *FLEETOR* rebuilding a pane's context from redrawn ANSI. R6 has the vendor reopen a file the vendor wrote, addressed by an id the vendor minted. FLEETOR invents nothing and parses no transcript for this feature. **The comment in `ui/src/components/RunHistory.tsx:1-16` says resume is "not an omission to fill in later" — it is being rewritten deliberately, not deleted quietly, and the rewrite must state which distinction makes it wrong.**

**Tier 1.4 — nothing in `runs.rs` may reach the message path.** Its module doc says so and R6 keeps it: the session id is read at rotation, after the run is over, so nothing polls a live pane. **Forbidden:** capturing session ids from a running fleet.

**Tier 1.1 — the operator's own directories are not reached into.** Everything here lives under `~/.fleetor`. `pane-config/` gains a level; nothing new is read from `~/.claude` or `~/.codex` beyond the seeding that already happens.

**Tier 1.6 — a claim is something somebody made, not a state something asserted.** A row's counts are derived from its own log. `reopened N×` is derived from lineage, not stored as a mutable number.

## Current state (verified 2026-09-07 — do not re-explore)

**The archive, as built.**
- `src-tauri/src/runs.rs:234` `rotate` — runs at the top of `fleet_bootstrap` (`fleet.rs:1376`), before the store is opened and before any pane exists. Every failure path degrades; rotation may not fail a boot.
- `runs.rs:292` `archive_files` — freeze then move, falling back to moving all three WAL files.
- `runs.rs:351` `harvest_transcripts` → `runs.rs:422` `walk_transcripts` with `Take::Move`. **This is what R3 changes.**
- `runs.rs:505` `transcript_locations` — the union over `harness::registered()`, deduplicated.
- `runs.rs:650` `write_agent_view` — writes `events.json` + `manifest.json` at archive time.
- `runs.rs:562` `snapshot_live_run` — the same shape for a *live* run, copying transcripts. Used by the evaluator and the Critic (`placement/mod.rs:1166`, `:1236`). **Unaffected by this package; do not disturb it.**
- `runs.rs:77` `RunRecord`, `:113` `LiveMeta`, `:144` `PaneRecord` (has `harness`, `model`, `transcript_format`; **no `session_id` yet**).
- `runs.rs:185` `begin`, `:213` `record_pane`, `:706` `list`, `:752` `delete`, `:760` `events`.
- `crates/fleetor-db/src/archive.rs:59` `freeze`, `:78` `digest`, `:122` `events`, `:143` `to_json`.
- `crates/fleetor-db/src/lib.rs` — one table, `events(seq, ts, kind, payload)`; `migrations.rs` is append-only.

**The harness seam.**
- `src-tauri/src/placement/harness.rs:93` `HarnessSpec` — fourteen numbered checkpoints plus `name`/`mark`/`login` as identity. **Checkpoint 15 goes here.**
- `harness.rs:758` `Transcript` (subdir, file_ext, format, transport), `:793` `Transport` — "both arms are implemented by `crate::runs`", enforced by a non-exhaustive match.
- `harness.rs:1183` `trait Harness`, `:1301` `command_args`. Claude Code's impl at `:1683`.

**Layout.**
- `placement/mod.rs:224` `Layout::pane_config(pane)` → `pane_config_root(shell).join(pane)`; root at `:281`. **Five real call sites — this is the R4 seam.**
- `placement/mod.rs:261` `Layout::worktree(target, slot)` — namespaced by target slug, **not** by run.
- `placement/mod.rs:1341` `ensure_worktree` — short-circuits on an existing `.git`, so an existing worktree is reused untouched; only a missing one is recreated, with `-B` (resets to HEAD).
- `placement/mod.rs:638` `enum RunSource { Live, Archived(String) }` — `Archived` is constructed nowhere.

**Reading.**
- `context_gauge.rs:217` `latest_transcript` — `max_by_key` on mtime. Accumulated sessions in one seat directory do not confuse the gauge.
- `fleet.rs:1818` `spawn_follower` — `bcast.follow(0)`, so the UI replays the whole live log then tails. R1's copied log therefore reaches the three views with no change.
- `fleet.rs:2402` `runs_list`, `:2408` `run_events`, `:2417` `run_rename`, `:2424` `run_delete`, `:2435` `run_export`.

**UI.**
- `ui/src/components/RunHistory.tsx` (261 lines) — two faces, catalogue and one opened run. **R5 deletes the second face.**
- `ui/src/fleet/useRuns.ts` (133 lines) — `OpenRun`, `split`, `openRun`/`close`. **R5 deletes `OpenRun` and its split.**
- `ui/src/components/Sidebar.tsx:45` the `View` union, `:160` `WORKSPACE`.
- `ui/src/components/TerminalGrid.tsx:135` — all four worker panes are mounted-and-hidden, so **every seat spawns on mount** and an ordinary run has all five sessions. This is why R8's whole-run refusal is cheap.

**Vendor facts, verified from `--help` on this machine 2026-09-07.**
- `claude --resume <session-id>` takes an id directly, no picker. `claude --session-id <uuid>` assigns one. `claude --continue` takes the most recent, scoped by project. `claude --fork-session` mints a new id on resume.
- `codex resume [SESSION_ID]` takes a UUID or a session name; `--last` takes the most recent; the picker filters by cwd unless `--all`. **Codex has no `--session-id` and no `--name` at launch** (full top-level flag list checked) and no per-session export subcommand.
- Carried over under R0 from the parked branch's shakedown, **not** re-measured: stock `codex resume` opens an interactive working-directory picker that a fenced pane cannot answer; `-c tui.resume_cwd=current` pre-answers it. Measured against codex 0.153.4.

## Scope

**In:**
- Rotation copies transcripts instead of moving them, into a per-run seat directory (R3, R4).
- `pane-config/<run-id>/<seat>/`, and a reopened run pointing its seats at its lineage root's directory (R4).
- Checkpoint 15 on `HarnessSpec` + `Harness`: read a seat directory's resumable id, produce the argv that reopens it, or declare the harness cannot (R6, R10).
- `PaneRecord::session_id` and the run's session-directory id in `manifest.json` (R6, R9).
- The reopen operation: confirm → teardown → rotate → copy the frozen log into `_shell/state.db` → spawn five resumed panes (R1, R2).
- History as a session switcher: one row per lineage, click to open, rows that cannot open saying why, Export/Rename/Delete secondary (R5, R12, R8, R10).
- Lineage-scoped delete: one row is one session (R15, superseding R9's refcount half).
- A `Notice` when a resumed worker's worktree was used by a later run (R7).
- A harness-authoring guide carrying all fifteen checkpoints (R11).

**Out:**
- **A read-only vendor-rendered view of a past run.** Explicitly rejected (R5). There is no way to look at a past run without reopening it; the Critic and evaluator still read archives directly and are untouched.
- **Anything injected into a resumed pane** — no brief, no time-gap line, no worktree warning (R13).
- **Restoring worktree state.** A resumed worker gets its worktree as it stands (R7).
- **Per-seat degradation.** All five or none (R8).
- **Going back to an earlier point in a lineage.** The sessions moved on; only the newest is reopenable (R12).
- **Auto-pruning beyond R15's lineage delete.** No retention policy, no size cap.
- **Cross-machine anything.** Local only, no backend.
- **`RunSource::Archived`.** Still constructed nowhere; pointing a Critic at a past run is a different gesture and stays unbuilt.

## Design

The settled decisions are in `decisions.md` under "History redesign — resume past runs" as **R0–R15**, each with its rejected alternatives and the reason. That section is authoritative; this is the shape they add up to.

**A run's state is three things, separated the same way.** Its events are a database (`runs/<id>/state.db`). Its sessions are a directory (`pane-config/<run-id>/<seat>/`). Its evidence is a copy of both (`runs/<id>/`). R4 is the whole trick: making sessions a *directory* per run means selecting a run's sessions needs no query, no mtime window, and no vendor schema — which is what keeps a third harness to a spec entry.

**Reopening is a rotation with a seeded log.** `fleet_bootstrap` already rotates; reopening is that same path with two differences — teardown happens first (rotation's transcript walk is only safe with no pane alive), and the new `_shell/state.db` is a copy of `runs/<id>/state.db` rather than an empty database. The panes then spawn against the lineage root's seat directories with checkpoint 15's argv.

**The log copy is what makes the UI trivial.** The task board has no table; it is folded from `task` events. Copying the parent's log forward is what brings the board back, and it is also why Messages, Tasks and Activity need no past-run-aware code — `spawn_follower` replays from seq 0 and finds that run's events already there.

**A lineage is the unit the operator sees, and the unit Delete acts on.** Reopen R and you get R′; both archives persist and both export, but they share one live session, so History shows one row. Trying to offer both would hand back R′'s panes under R's label. Delete follows the same unit (R15): the row's archives are snapshots of one conversation, so removing the row removes all of them and the lineage's session directory, and reaches nothing outside it.

## Open questions

Recommended default given for each. The five marked **(mockup)** were found by drawing the UI, not by the interview.

1. **Nothing names which session is open. (mockup)** Once History is a switcher, the pane view should say what you are typing into. **Recommended:** the topbar carries the run's label, and marks a reopened one. Drawn in the mockup as `reopened · Sep 3`.

2. **The duration chip spans a lineage. (mockup)** Is `2h 14m` this sitting or wall-clock across three sittings weeks apart? **Recommended:** show the newest sitting's duration and let `reopened N×` carry the rest; a summed duration across a fortnight's gap means nothing.

3. **The live session is not in the list. (mockup)** It appears only after the next rotation. As "History" that was a mechanism; as "your sessions" it reads as a gap. **Recommended:** show it first, marked as the one you are in and not clickable.

4. **Every archive on disk today is unopenable. (mockup)** They predate checkpoint 15. First launch after this ships shows a list where nothing opens. **Recommended:** the empty-ish state says it once at the top of the list rather than repeating a reason on every row.

5. **What the Delete confirm says. (mockup, narrowed by R15)** Delete now removes a whole conversation — three archives and its sessions for a row reopened twice. **Recommended:** the confirm states the count ("3 archived sittings and their sessions"), since the row shows one label and the operator has no other way to know what is behind it.

6. **What happens to the existing flat `pane-config/<seat>/`?** R4 changes the layout. **Recommended:** leave the old directories in place, untouched and unreferenced, and start writing `pane-config/<run-id>/<seat>/`. Deleting an operator's existing sessions to tidy a layout change is not a trade this package gets to make.

7. **Does a fresh per-run seat directory start cold in a way that matters?** R4's flagged risk, now **partly measured** (R16c): Claude Code's two-key seed covers it *only if* the trust key names the resolved path — otherwise the reopened pane hangs on a trust gate. Codex's snapshot allowlist (C39) is still unchecked for sufficiency, as is a credential-seeded Claude Code reopen. **Recommended:** fold both into S1, where the seeding code is already in hand.

8. **Disk growth is unmeasured.** R3 keeps a session live *and* archived; R4 keeps a directory per run. **Recommended:** measure across ten runs before deciding whether R15's lineage delete is the whole retention story.

**Spikes: done** (2026-09-08, `docs/notes/reopen-spike-notes.md`, R16). What they settled:

- **Neither vendor forks on resume**, so R3/R4/R6 are mechanically sound. A resumed Claude Code turn appends to the same `.jsonl` (one `sessionId` throughout, equal to the filename stem); a resumed codex turn appends to the same thread, zero new threads.
- **codex's working-directory picker is cwd-*dependent*, not unconditional** — correcting the fact WP-27 carried. Keep `-c tui.resume_cwd=current` as a defence for the *mismatch* case only: under R7 cwds normally match, but a worktree recreated elsewhere raises a gate that R13 guarantees nobody answers, and the pane hangs.
- **Claude Code's trust key is keyed by the RESOLVED path**, widening D-030. R4's fresh-per-run seat directory raises this gate on every reopen, so seeding must use the realpath or the pane sits on "Quick safety check" forever.

**Still unmeasured, and it is open question 7's real content:** every Claude Code turn in the spike returned `Not logged in`, so that half spent nothing and its no-fork result proves the *session plumbing* appends, not that a successful assistant turn does. The credential is in the keychain and the product seeds it (`harness.rs` `write_operator_login`); the probe does not. **A credential-seeded re-run through the product's own seeding path closes this, and should happen inside S1 rather than as a separate spike** — by then the seeding code is in hand.

## Slices

Vertical tracer bullets. Each ends in something demoable; none is a foundation laid for a later slice.

**S1 — one run reopens, Claude Code only.** R3, R4, R6, R1, R2, R5 for the happy path. Click the newest row, the fleet comes back, Messages shows its history. No refusals, no lineage display, no codex. *Demo: click a row, type into orch, it remembers.*

**S2 — honesty.** R8's whole-run refusal from the manifest before teardown, R10's declared refusal in the spec and on the row, the pre-checkpoint-15 rows, the conformance test. *Demo: a run that cannot open says exactly why, and the live fleet survives the attempt.*

**S3 — lineage.** R12's one-row-per-lineage, `reopened N×`, R15's lineage-scoped delete, R7's worktree `Notice`. *Demo: reopen twice, see one row, delete it and watch every other row survive.*

**S4 — codex parity and the guide.** Checkpoint 15's codex answer, the `tui.resume_cwd` finding re-verified, R11's harness-authoring guide covering all fifteen. *Demo: a mixed fleet reopens; a reader can register a third harness from the doc alone.*

## Session prompt

```
Read docs/roadmap/27-reopening-a-run.md in full first, then building.md §1 and §9, then
the "History redesign — resume past runs" section of decisions.md (R0–R14). The visual
guide is https://claude.ai/code/artifact/5b6ff072-faeb-49cf-9372-73bd41f0776c — layout
questions are answered there, behaviour questions by the doc.

Do not re-explore the codebase for anything in the doc's "Current state" section; those
anchors were verified 2026-09-07. Re-verify them mechanically before you commit, and say
so if any moved.

The vendor spikes are DONE — read docs/notes/reopen-spike-notes.md before you start, and
re-run examples/reopen-spike/probe.py --reuse (free) if you want to see the arms go green.
Three findings change what you build: neither vendor forks on resume; codex's cwd picker
fires only on a cwd MISMATCH; and Claude Code's trust key must name the RESOLVED path or a
reopened pane hangs on a trust gate. One thing the spike could not measure is a live turn
through a fenced Claude Code config dir (it seeds no credential) — close that inside S1,
where the product's own seeding is in hand, and say plainly in your report whether it held.

Build S1 only, and stop there for review. S1 is: rotation copies transcripts instead
of moving them; pane-config gains a run-id level; checkpoint 15 lands on HarnessSpec and
Harness with Claude Code's answer; manifest.json carries session_id per seat; the reopen
operation runs confirm -> teardown -> rotate -> copy the frozen log into _shell/state.db ->
spawn five resumed panes; History's second face and useRuns' OpenRun split are deleted and
a click on a row reopens. Refusals, lineage display and codex are S2-S4 and are out of
scope for this session.

The rewrite of RunHistory.tsx's header comment is deliberate, not incidental. It currently
refuses this feature by name. Replace it with the distinction that makes the refusal
inapplicable — D-030 rejected FLEETOR reconstructing a pane's context; this has the vendor
reopen a file it wrote itself, addressed by an id it minted. Do not delete the paragraph
silently.

Nothing under prompts/ may change. A diff against it must be empty.

Finish with the session exit checklist at the foot of the doc.
```

## Session exit checklist

- [ ] `decisions.md` entry for every Tier 2 default that moved (cite the R- or D-number in the commit subject).
- [ ] If a verb was added or changed: both prompt files + `VERBS` + the clap enum + pinned tests move in one commit. *(None expected — brief-cost 0.)*
- [ ] Spike notes committed to `docs/notes/`, version-stamped with the vendor versions measured, with a `docs/README.md` index row.
- [ ] As-built docs updated in the same PR — `docs/runtime-layout.md` for the `pane-config/<run-id>/` change, and `docs/fleet-comms-map.md` if anything touched the message path (it should not).
- [ ] The harness-authoring guide exists and covers checkpoint 15 (R11) — S4, but note its absence at every earlier slice.
- [ ] This doc: status → landed, "How it landed" appended, and any spike-corrected claim fixed in place.
- [ ] `00-index.md` status column updated — the last act of the session.
