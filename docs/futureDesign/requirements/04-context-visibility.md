# WP-04 — Context visibility: the Loadout counter + live gauge

status: not-started size: M
depends-on: 01 blocks: — (makes WP-03 decisions informed)
brief-cost: +~30 tokens (one orch line about the roster's context column)

## Outcome

The operator and the orchestrator can *see* how much context each worker is operating with — the prerequisite for deliberate `/compact`/`/clear` decisions (WP-03). Two instruments:

1. **Spawn-time counter** (the autonomy doc's shipped Loadout verdict, ~50 lines): an estimate of each pane's starting context (rendered system prompt + anything else injected at launch), one Activity line per launch.
2. **Live gauge**: a read-only sampler over each worker's own Claude Code session transcript (JSONL under the worker's isolated `CLAUDE_CONFIG_DIR`), surfacing "worker-2 ≈ 61% of window" in the UI band — and to orch as a new column in `fleet roster` (the surviving half of the Open Loops design), costing zero new verbs.

## Performance criteria

### Technical
- [ ] Observer-only: nothing in the message path reads context data; the delivery diff is empty.
- [ ] `PaneState` stays exactly `Spawning|Live|Dead` — no `Idle`/`Working` inference (`pane.rs:107-119` documents why).
- [ ] Unmeasured = **absent, never faked** (decisions.md L155: "band metrics we don't yet track are omitted, not faked"). A stale or unreadable transcript renders as absence, not zero.
- [ ] Spike first: `docs/notes/context-gauge-notes.md`, **version-stamped** (the transcript path/schema is CC-version-dependent — same risk class as `tui-spawn-notes.md`, same treatment). Locate the per-session JSONL under `~/.fleetor/_shell/pane-config/worker-N`, identify the token-usage fields, confirm they update mid-session.
- [ ] `fleet roster` gains an optional context field on `PaneEntry` (`crates/fleetor-core/src/pane.rs:145`) threaded through `AppCommand::Roster` (`src-tauri/src/pty.rs:226` roster) — optional so absence serializes as absence; the frontend renders unknown/missing fields as nothing, so `ui/src/fleet/types.ts` and the band component are extended in the same session.
- [ ] Samples are **not** persisted to the event log (keeps the three-kind log clean). Live Tauri channel only, plus at most one `Notice` on first crossing of ~80% per pane per session.
- [ ] Stretch (only if trivial): a restart counter — how often panes get restarted — the pre-measurement the Carryover design asked for.

### Semantic
- Every displayed figure carries its honesty label: `≈`, "from transcript, may lag."
- Orch prompt gains one line: context figures come from `fleet roster`; stale or absent means *unknown*, not zero — decide accordingly.

## Invariant guardrails

- **"Omitted, not faked"** is the ruling precedent (decisions.md L155). No placeholder numbers, no extrapolation presented as measurement.
- **No thresholds that act.** The 80% Notice informs; nothing refuses, warns-and-blocks, or auto-compacts. The observer proposes; an agent (or the operator) decides — WP-03 is the acting hand.
- **Orch's own gauge is out of scope**: orch runs under the operator's real config dir — reading it is an ownership/privacy call above the builder. Display "—".

## Current state (verified 2026-08-06 — do not re-explore)

- Worker config isolation: `CLAUDE_CONFIG_DIR` per worker at `~/.fleetor/_shell/pane-config/worker-N` (`src-tauri/src/fleet.rs:129`); seeded by `spawn.rs:190` `seed_config_dir`.
- No context/token tracking exists anywhere today; `FleetEvent` is frozen at `Message|PaneState|Notice` (`event.rs:20`) with a module doc explaining why speculative kinds are banned.
- The UI band / dashboard: one cell per pane, visible in every view (`building.md` §7); `TopBar.tsx` and the band components are the render targets.
- CC 2.1.223 is the installed version to stamp measurements against.

## Scope

### In
Spawn-time estimate + Activity line; transcript sampler (backend, read-only); band display; `PaneEntry` optional field + roster threading + TS types; the single 80% Notice; spike notes; prompt one-liner (with `VERBS` untouched — no new verb).

### Out
Orch-pane tracking; budgets or refusals of any kind; auto-compaction; persisting samples to SQLite; tokenizer dependencies (estimate is fine — Loadout's own verdict).

## Design sketch & open questions

1. **Poll vs on-demand.** Recommended: sample on-demand when `fleet roster` is asked, plus a slow UI timer (~10 s) for the band. No hot loops over five JSONL files.
2. **Estimation method.** Recommended: read CC's own usage numbers from the transcript if present; else chars/4. Label stays `≈` either way.
3. **Window size.** The denominator (model context window) differs per model; recommended: a Tier 2 constant per pane class, recorded in `decisions.md`.

## Session prompt

```
You are working in /Users/bubblyducks/harness/fleetor. Read
docs/futureDesign/requirements/04-context-visibility.md in full, then
building.md §1, §4, §9. Execute WP-04: spike the worker transcript format
first (docs/notes/context-gauge-notes.md, version-stamped against the installed
claude), then build the spawn-time starting-context estimate (one Activity
line per launch) and the live read-only gauge — UI band display plus an
optional context column on fleet roster's PaneEntry. Observer-only:
message path untouched, PaneState unchanged, absent-never-faked, no
acting thresholds beyond one informational ~80% Notice per pane per
session. Finish with the session exit checklist.
```

## Session exit checklist

- [ ] Spike notes committed, version-stamped.
- [ ] Full test matrix green; message-path diff empty.
- [ ] `decisions.md` entry (gauge source, window-size constants).
- [ ] Orch prompt line added; validation green (no verb changes).
- [ ] `00-index.md` status updated.
