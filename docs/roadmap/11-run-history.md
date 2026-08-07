# WP-11 — Run history: past runs as long-term memory

status: in-progress size: L
depends-on: — blocks: the evaluation sandbox
brief-cost: 0 — touches no `prompts/*`

## Outcome

Today every fleet start appends to one event log, so run 4 reads run 1's messages as
its own and the operator has no way to look at what the fleet did last Tuesday. This
package makes a **run** a first-class thing with a beginning, an end, a name and its
own database. Past runs become long-term memory the operator can browse — and, in the
work that follows this, the evidence an evaluator reads to compare one cycle against
another. A new run cannot be corrupted by an old one because it never shares a file
with it.

## Performance criteria

### Technical

- [ ] `cargo test --workspace` and `cargo test --manifest-path src-tauri/Cargo.toml` green.
- [ ] `tsc --noEmit && vite build` clean.
- [ ] A test proves rotation preserves an event log left by a killed writer — the
      uncheckpointed-WAL case from `docs/notes/run-rotation-notes.md`, not a
      cleanly-closed database.
- [ ] A test proves a fleet start with an unreadable previous database still boots,
      and emits a `Warn`.
- [ ] A test proves the run index survives its own deletion — rebuilt by scanning
      `runs/*/`, with labels the only thing actually lost.
- [ ] `rm -rf ~/.fleetor` still leaves the target repo untouched (Tier 1.1 unchanged).

### Semantic

- [ ] Opening a past run cannot write to it, send anything, or spawn anything — a
      reader looking at history has no reachable control that acts on the live fleet.
- [ ] The History list tells the operator which run is which without opening any of
      them: when it ran, what it was pointed at, how much happened.
- [ ] A run archived from a crashed fleet is as complete as one archived from a clean
      quit. Teardown is not load-bearing.

## Invariant guardrails

**Tier 1.1 (repo boundary)** — everything this adds lives under `~/.fleetor/runs/`.
No new writes to the target repo, and `rm -rf ~/.fleetor` remains total.

**Tier 1.4 (nothing in the message path)** — rotation happens at bootstrap, *before*
the store is opened and long before any pane or socket exists. It is not on the
delivery path and must never acquire a way to be. A History view that could send,
replay-into, or re-run anything would be a dispatcher; this is a diary of diaries.

**Tier 1.6 (outcomes, not intentions)** — an archived run records what happened. The
index's counts are derived from the archived log at archive time, never asserted.

**D-030's regrowth warning** — the nearest failure mode here is a History view that
grows a "resume this run" button. There is nothing to resume: the panes are gone and
their context died with them. A past run is readable and nothing else.

## Current state (verified 2026-08-07 — do not re-explore)

- `src-tauri/src/fleet.rs:132` — `shell_dir()`, the `_shell` root.
- `src-tauri/src/fleet.rs:227` — `let dir = shell_dir();` inside `fleet_bootstrap`.
- `src-tauri/src/fleet.rs:237` — `SqliteStore::open(&dir.join("state.db"))`. **The one
  place the store is opened.** Rotation goes immediately above this line.
- `src-tauri/src/fleet.rs:216` — `#[tauri::command] fleet_bootstrap`, idempotent: it
  returns early on line 223 if a fleet already exists, so rotation runs exactly once
  per app launch.
- `src-tauri/src/lib.rs:82` — the `invoke_handler!` list new commands are added to.
- `crates/fleetor-core/src/store.rs` — the `Store` trait, three methods. A past run
  needs only `events_since` and `latest_seq`; no new trait, no fourth seam (building.md §3).
- `crates/fleetor-db/src/lib.rs` — `SqliteStore::open`, WAL enabled in `init`,
  migrations run on open. **Opening an archived run migrates it** — see the open
  question below.
- `ui/src/components/Sidebar.tsx:39` — `export type View = "fleet" | "messages" |
  "tasks" | "activity" | "settings"`.
- `ui/src/components/Sidebar.tsx:61` — `ICONS: Record<View, ReactNode>`.
- `ui/src/components/Sidebar.tsx:110` — `WORKSPACE`, the nav array.
- `ui/src/App.tsx:43` — `usePersistedNav()` holds the active view.
- `ui/src/App.tsx:159–191` — the `stage-view` blocks; every view is mounted and hidden
  with `.is-hidden`, never conditionally rendered (building.md §7 rule 5, L7).
- `ui/src/components/{MessageFeed,TaskBoard,EventFeed}.tsx` — the three components a
  past run replays into.
- `ui/src/fleet/useFleet.ts` — the live hook; splits the stream into `feed`,
  `messages`, `commands`, `tasks`. A past run needs the same shape from a different source.
- `ui/src/fleet/api.ts` — the one place a Tauri command name is spelled.

## Scope

**In:** rotate-on-start with checkpoint; `~/.fleetor/runs/<id>/state.db`; a rebuildable
`runs/index.json`; auto-suggested and editable labels; delete; a History view listing
past runs; opening one read-only into the existing Messages, Tasks and Activity views
behind a banner.

**Out:** cross-run search or diffing (the evaluator's job, and it reads the files
directly). Exporting. Compaction or retention policy — runs accumulate until the
operator deletes them, and a size column is how they find out. Anything that resumes,
replays-into-a-live-fleet, or re-runs a past run. Renaming the *live* run before it
ends: a run is labelled when it is archived, because that is when there is something
to name it after.

## Design sketch & open questions

**Rotation is B-with-C-fallback**, settled by `docs/notes/run-rotation-notes.md`:
checkpoint the old `state.db` and move that one file; if it cannot be opened, move all
three files untouched; if that fails, leave them and `Warn`. **Rotation may never fail
a boot** — a history feature that stops the app starting is worse than the bug it fixes.

**The index is a cache, not the truth.** `runs/index.json` supplies labels; the
directories supply existence. A read scans `runs/*/` and merges, so a lost or corrupt
index costs labels and nothing else. This is why the run id is a timestamp directory
name and the label is a separate mutable string — a rename is a JSON edit, never a
file move.

**Empty runs are archived like any other.** A start-then-quit leaves a run with only
boot notices; the suggested label says so and the counts make it obviously deletable.
Throwing it away instead would be the code deciding which of the operator's history is
worth keeping.

*Open question, decided by whoever implements:* opening an archived run through
`SqliteStore::open` runs migrations against it, which mutates a file this package
calls frozen. Recommended default: open past runs **read-only** with a separate
constructor that skips `migrate`, and let a run written by an older schema fail to
list with a reason rather than be silently upgraded. The alternative — migrating on
open — makes archives self-healing but means a bug in a future migration can damage
history that was already complete.

## Session prompt

```
Read docs/roadmap/11-run-history.md in full, plus building.md §1 (invariants) and §9
(escalation triggers). The "Current state" section is verified — do not re-explore it.

Implement WP-11. Land it in two commits so the storage half can be reviewed before the
UI half exists:

  1. src-tauri/src/runs.rs + the rotation call in fleet_bootstrap + the Tauri commands
     (runs_list, run_open, run_rename, run_delete) + tests.
  2. The History view: the Sidebar entry, the list, and read-only replay into
     MessageFeed / TaskBoard / EventFeed behind a banner.

docs/notes/run-rotation-notes.md is the measurement behind the rotation strategy and
it wins over any assumption in this doc (building.md §4). Do not add a Store method or
a third seam. Do not put anything on the message path.

Exit: the checklist at the bottom of this doc.
```

## Session exit checklist

- [ ] `decisions.md` entry for every Tier 2 default that moved (cite the D-number in the commit subject).
- [ ] If a verb was added or changed: N/A — this package adds no `fleet` verb.
- [ ] Spike notes committed to `docs/notes/`, version-stamped, with a `docs/README.md` index row.
- [ ] `docs/runtime-layout.md` updated — `~/.fleetor/runs/` is a new top-level directory.
- [ ] This doc: status → landed, "How it landed" appended.
- [ ] `00-index.md` status column updated — the last act of the session.
