# WP-16 — Dev mode

status: landed size: S/M
depends-on: — blocks: 15, 17
brief-cost: 0 — this package touches nothing under `prompts/`, and that is a requirement rather than an accident (see "Invariant guardrails").

## Outcome

FLEETOR gains a second posture. Everything the self-improvement arc adds — the
evaluator that reads a run and coaches `orch` (WP-15), the deny-paths that stop a
fleet editing the machinery judging it (WP-17) — exists inside a mode the
operator switched on deliberately and can see at a glance for as long as it is
on. On its own the mode does almost nothing, and that is the whole point: it is
the container, sized so the packages that fill it never have to argue about where
the switch lives or how to read it.

## Performance criteria

### Technical

- [x] `cargo test --workspace` green.
- [x] `cargo test --manifest-path src-tauri/Cargo.toml` green, including two new test targets:
      the `dev::tests` unit tests and `src-tauri/tests/dev_mode.rs`.
- [x] `npx tsc --noEmit && npx vite build` green.
- [x] The mode survives a restart, proven against a real file rather than asserted:
      `dev_mode_survives_a_restart_in_both_directions` writes with `write_at`, reads back with a
      cold `read_at` — which is exactly what a relaunched app does, since nothing caches the flag.
- [x] Exactly one source of truth. `dev_mode` in `~/.fleetor/config.json`, read only through
      `dev::is_enabled`. No `localStorage` copy, no second file, no boot-time snapshot.
- [x] `git diff` under `prompts/` is empty.
- [x] `the_delivery_path_cannot_read_dev_mode` fails if any of the nine files a message passes
      through ever mentions the mode.
- [x] `no_pane_is_briefed_about_dev_mode` fails if any prompt file, the brief renderer or the
      roster's `PaneId` ever mentions it.

### Semantic

- [x] An operator can never be in dev mode by accident: the band is in every view at once,
      it names itself in words as well as in colour, and only a real JSON `true` turns it on
      (a `"true"`, a `1` and a broken config are all off).
- [x] An operator can never wonder which mode they are in: the mode is on-screen or it is
      not, and the settings switch does not move until the write has actually landed.
- [x] With the mode off, the app is byte-identical to what it was, plus one settings row.
- [x] WP-15 and WP-17 can build on this without touching it. `dev::is_enabled()` is the
      whole surface, and a later package adding a branch adds it at its own call site.

## Invariant guardrails

**Tier 1.1 — everything runtime under `~/.fleetor`.** The flag is a key in
`~/.fleetor/config.json`, beside the target the operator already sets there.
`rm -rf ~/.fleetor` turns dev mode off along with everything else, and nothing is
written into the target repo.

**Tier 1.4 — nothing between `fleet send` and a pty.** The rule is usually read
as "add no queue, gate or limiter", but the cheaper form is *give the delivery
path nothing it could branch on*. A mode readable inside `deliver.rs` is one
`if` away from a delivery that behaves differently in dev mode, which is the
refusal §9.3 records as argued and lost twice (D-034). So no module on the
message path imports `dev` — not `deliver`, not `pty`, not the hub, not the wire
or message contracts, not the CLI — and `the_delivery_path_cannot_read_dev_mode`
is the tripwire that keeps that true after this session. The allowed shape for a
future mode-dependent behaviour is the one WP-19 is already held to: change what
*exists* (a window, an address, a deny path), never what a delivery *does*.

**§7 rule 5 — every terminal stays mounted.** Untouched. The band is chrome
above `.body`, not a stage view; no `.stage-view` gained or lost a condition, and
`TerminalGrid` never re-renders because of the mode. The band itself is
conditionally rendered, which rule 5 permits — it holds no buffer, and
`StartGate` sets the precedent for chrome that is simply absent when it does not
apply.

**§7 theme rules.** Warm only, no blue: the band is `--accent` (coral) text and
dot on `--bg-2` with a 2px coral underline. Coral rather than gold because §7
assigns coral to "needs or has attention" and a standing evaluation posture is
the app's one permanent attention state. Status is never colour alone (rule 3):
the dot is paired with the words DEV MODE and a sentence saying what the posture
costs. Flat (rule 1): a hairline underline is the whole of the emphasis — no
gradient, no shadow. Coral as *text* rather than as a fill is deliberate — the
light theme deepens `--accent` specifically so it passes AA as text
(`ui/src/styles.css:160`), and a filled coral band is the one place that tuning
does not help.

**WP-12 open question 4 — does `orch` know it is in dev mode?** Answered: no.
The mode is visible in the operator's UI and absent from `prompts/orch.md`,
`prompts/worker.md` and `fleet roster`. The veil covers the evaluator, not the
mode, but an `orch` told a mode exists is an `orch` invited to ask what is
different about it — and the veil is far cheaper to keep than to re-establish.
`no_pane_is_briefed_about_dev_mode` pins it.

## Current state (verified 2026-08-07 — do not re-explore)

Every anchor below was read in the session that wrote this doc.

**The flag and its one reader**

| Anchor | What is there |
|---|---|
| `src-tauri/src/dev.rs:41` | `pub const CONFIG_KEY: &str = "dev_mode"` — one spelling, referenced by every reader and by the tripwire |
| `src-tauri/src/dev.rs:50` | `pub fn is_enabled() -> bool` — **the one read.** Re-reads the file; nothing caches it |
| `src-tauri/src/dev.rs:57` | `pub fn set_enabled(enabled: bool) -> Result<bool, String>` — writes, then reads back, and answers with what is *stored* |
| `src-tauri/src/dev.rs:62`, `:69` | `read_at` / `write_at` — the same two against a named file, the seam the round-trip test uses |
| `src-tauri/src/dev.rs:80` | `parse_dev_mode` — only a real JSON `true` is on; a string, a number, a missing key and unparseable text are all off |
| `src-tauri/src/dev.rs:95`, `:101` | `dev_mode_get` / `dev_mode_set` — the two Tauri commands. Neither touches `FleetState`, so both work on the start gate |
| `src-tauri/src/lib.rs:17`, `:98` | the module, and the two commands in `invoke_handler` |

**Where it is stored**

| Anchor | What is there |
|---|---|
| `src-tauri/src/fleet.rs:144` | `config_path()` — `~/.fleetor/config.json`, now `pub(crate)` so `dev` can read it |
| `src-tauri/src/fleet.rs:870`, `:876` | `write_config_key` / `write_config_key_at` — **the one writer of that file**, so a second setting cannot grow a second spelling of merge-don't-clobber that drops the first |
| `src-tauri/src/fleet.rs:893` | `merge_config_key` — the pure merge. `write_target` (`:862`) is now a one-liner over it; `merge_target` survives only as a test-local wrapper |

**The tripwires**

| Anchor | What is there |
|---|---|
| `src-tauri/tests/dev_mode.rs:23` | `SPELLINGS` — the four forms a search would find (`dev_mode`, `devmode`, `dev mode`, `dev-mode`), matched case-insensitively |
| `src-tauri/tests/dev_mode.rs:57` | Tier 1.4: nine delivery-path files, none of which may mention the mode |
| `src-tauri/tests/dev_mode.rs:79` | the veil: five prompt files plus `brief.rs` and `pane.rs` |
| `src-tauri/src/dev.rs:114` | the restart round trip, on a real file in a temp dir |

**The UI**

| Anchor | What is there |
|---|---|
| `ui/src/fleet/api.ts:85`, `:91` | `fetchDevMode` / `setDevMode` — the two invokes, wrapped once like every other command |
| `ui/src/ui/useDevMode.ts:17`, `:32` | `DevModeControls` and the hook. `enabled` is `boolean \| null` — on, off, and not-yet-answered are three states, and the third is why the banner does not flash |
| `ui/src/components/DevModeBanner.tsx:22` | the band: `.dot.dot--accent` + `DEV MODE` + the note |
| `ui/src/App.tsx:69`, `:160`, `:215` | the hook, the conditional band between `TopBar` and `.body`, and the prop into `SettingsPanel` |
| `ui/src/components/SettingsPanel.tsx:54` | the `Development` group — the only trace of the mode while it is off |
| `ui/src/components/SettingsPanel.tsx:71` | the switch, `disabled` until the backend has answered |
| `ui/src/styles.css:404` | `.devbar` and its two children |
| `ui/src/styles.css:1398` | `.setting-row__error` — gold, not red: nothing is broken, the preference simply did not take |

**Unchanged, and verified so:** everything under `prompts/`, `crates/fleetor-cli/`,
`crates/fleetor-server/`, `src-tauri/src/deliver.rs`, `src-tauri/src/pty.rs`.

## Scope

**In:** the persisted flag and its single Rust reader; the two commands; the band;
the settings switch; the two tripwire tests.

**Out:**

- **Anything that branches on the mode.** Nothing in this repo reads
  `is_enabled()` today except the command that reports it. The evaluator window
  is WP-15's, the deny paths are WP-17's, and a placeholder branch here would be
  a guess at both.
- **An Activity notice when the mode is toggled.** Tempting — the feed is where
  "the app changed posture" would ordinarily go — but it would need `FleetState`,
  it would not work on the start gate where the toggle actually gets used, and
  the band already answers the question the notice would answer. If a *run*
  should record which mode it ran in, that belongs to whoever archives runs for
  the evaluator to read, with the mode as a field on the run manifest rather than
  as prose in the log.
- **A `devmode` cargo feature.** D-060 names one, for compiling the evaluator's
  brief in from a separate repo. That is a property of *that* brief and belongs
  to the package that ships it. A cargo feature is a build-time switch; this is a
  runtime mode the operator flips without a rebuild, and conflating them would
  make dev mode need a recompile.
- **Turning the mode off from the band.** The band is a status strip. A control
  there would be the app's most visible button and would sit one mis-click from
  ending an evaluation posture mid-run.

## Design sketch & open questions

**Why config.json rather than `localStorage`.** The theme
(`ui/src/ui/useTheme.ts`) and the nav selection (`usePersistedNav.ts`) both live
in `localStorage`, and dev mode looks like one more of those. It is not: WP-15
and WP-17 read the mode from code that has no webview. Storing it in
`localStorage` and mirroring it to Rust would be two copies of one fact, and the
copy that is wrong is the one somebody reads. Logged as **D-061**.

**Why read fresh rather than snapshot at bootstrap.** `Fleet::target` is
snapshotted deliberately — half a fleet in one repo and half in another is
incoherent. A mode has no equivalent half-state *today*. When WP-15 gives it one
(a window that exists or does not), that window's lifecycle is WP-15's decision,
and a cached flag here would have pre-empted it.

**Why the switch does not move optimistically.** A mode that persists is the
entire feature. A switch showing "on" over a config that says otherwise is
showing the wrong app, so the write lands first and the answer is what gets
rendered.

**Open question — should an archived run record the mode it ran under?** An
evaluator comparing generations needs to know a run was made in the posture it is
judging. The mode is not currently written to the run manifest. Left open on
purpose: it is one field, and the package that knows what the evaluator reads
should choose its name. Not a blocker for WP-15 or WP-17.

**Test coverage limit, stated plainly.** The Rust side is covered
comprehensively — the parse truth table, the restart round trip on a real file,
the config-merge interaction with `target`, and the two tripwires. The React
side has none, because **this repo has no frontend test framework and adding one
is a Tier 2 dependency decision that is not a builder's to make.** What that
leaves unverified by machine: that the band actually renders, that it is legible
in both themes, and that the switch reflects what was stored. The band's markup
and CSS were reviewed against §7 rule by rule; they were not seen on screen in
this session.

## Session prompt

```
Implement WP-16 — dev mode. Read, in this order and in full:

  1. docs/roadmap/16-dev-mode.md — this doc
  2. docs/roadmap/12-self-improving-loop.md — the arc it belongs to; note open question 4
  3. building.md §1 (tiers), §7 (theme + layout), §9 (when to ask rather than decide)

Build the operator-triggered mode: persisted in ~/.fleetor/config.json, loud in
the UI when on, invisible when off beyond the control that turns it on, and
readable from Rust through exactly one function.

Hard constraints, none negotiable:
  - Tier 1.1 — nothing written outside ~/.fleetor.
  - Tier 1.4 — no module on the path from `fleet send` to a pty may read the mode.
  - §7 rule 5 — every terminal stays mounted; hidden panes use `.is-hidden`.
  - §7 theme — warm only, no blue, status is dot + text label, flat.
  - Do not edit any file under prompts/. If you believe you must, stop and report.

Tier 2 defaults that move get a three-line decisions.md entry (append only).
Exit: cargo test --workspace, cargo test --manifest-path src-tauri/Cargo.toml,
npx tsc --noEmit && npx vite build — all green, output pasted into the report.
Then this doc's status, and 00-index.md's status column, as the last act.
```

## Session exit checklist

- [x] `decisions.md` entry for every Tier 2 default that moved — **D-061** (the mode's home).
- [x] No verb was added or changed; `prompts/`, `VERBS` and the clap enum are untouched.
- [x] No spike was needed — nothing here touched an unknown.
- [x] As-built docs: nothing on the message path changed, so `fleet-comms-map.md` is unaffected.
      This doc plus D-061 is the as-built record for the mode.
- [x] This doc: status → landed, "How it landed" appended.
- [x] `docs/README.md` and `00-index.md` status column updated.

## How it landed

Small, as designed — one new Rust module, one new test target, one hook, one
component, two CSS blocks and a settings group.

Two things came out differently from the sketch. **The config writer was
generalised rather than duplicated:** `write_target`'s merge-don't-clobber logic
became `write_config_key` / `merge_config_key`, and `dev_mode` goes through the
same one, so a future third setting cannot grow a third spelling of it.
`merge_target` survives only as a wrapper inside `fleet.rs`'s test module, which
keeps those tests reading as being about the target.

**The Tier 1.4 guardrail became a test rather than a note.** The plan was to
simply not import `dev` from the delivery path. That is a fact about today's code
and nothing preserves it, so it is now a test that reads the nine files a message
passes through and fails on any mention of the mode — the move
`crates/fleetor-core/src/task.rs` makes with its own tripwire list. The same test
pins the veil across `prompts/` and the brief renderer, which is what makes "we
answered open question 4 the cheap way" survive the session that answered it.
