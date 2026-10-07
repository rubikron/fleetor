# One fleet launch: a new fleet and a reopen go through the same path

**Status:** ready-for-agent
**Decision:** D-099 (amends D-058, D-071, D-095)

## Problem Statement

I reopened a past run from History, then went back to Home, picked different harnesses on the start gate, and clicked **New fleet**. The view switched to the fleet, but nothing changed: the reopened run's panes kept running on its recorded seats (Claude Code, sonnet orch, haiku workers) instead of what I picked. Clicking New fleet while any fleet is running does nothing, and the app gives no sign of it.

The cause is structural. Starting a new fleet and reopening a run are two separate paths. Only reopen tears the running fleet down. New fleet assumes nothing is running, and it returns the live fleet unchanged if one is. Around that, several pieces of state outlive the transition they belong to:

- a reopen request file that survives a failed start and silently turns the next New fleet into a reopen
- UI feed, message and task lists that mix runs
- judges latched "started" after their panes were killed
- a failed start that leaves the UI believing the fleet is up

I also can't switch repos without closing the app. The target can't be changed while a fleet runs, and a reopen runs against whatever target the gate currently holds, not the repo the run belonged to.

## Solution

There is one way a fleet comes up: a **launch**, either **fresh** (from the gate) or a **reopen** (of a past run). Every launch does the same thing:

1. Check the seats can start.
2. Tear down whatever is running.
3. Set the old run aside for archiving.
4. Bring up the new fleet.

From my side:

- **New fleet always starts a new fleet** with exactly the seats and target on the gate, whether or not something is running. The running fleet goes to History.
- **Clicking a past run always reopens it** on the harnesses, models, credentials and target it recorded, whatever the gate holds.
- **No confirmation.** The view switches to the new fleet immediately. Archiving the old run finishes in the background.
- **The gate always describes the next fleet.** Seats and target stay editable while a fleet runs, and editing them never touches the running fleet.
- **History shows every run, including the live one.** Each run is one row that moves from **Running now** to **Archiving…** to **Archived**.
- **If a launch fails, I land back on Home with the reason.** The panes don't sit dead.

## User Stories

1. As an operator, I want New fleet to start a fleet on my current gate picks even while another fleet is running, so that I can move to a new session in one click.
2. As an operator, I want the running fleet torn down and archived when I start a new one, so that I never have two fleets fighting over the same slot.
3. As an operator, I want no confirmation when I start a new fleet or reopen a run, so that switching sessions feels seamless.
4. As an operator, I want the view to switch to the new fleet immediately, so that I am not waiting on archiving.
5. As an operator, I want archiving of the previous run to finish in the background, so that its cost never delays the new fleet.
6. As an operator, I want a reopened run to come back on the harness, model and credential each seat recorded, so that every pane resumes its own conversation.
7. As an operator, I want a reopened run to ignore the gate's current seats, so that a resume can never put a codex session on Claude Code.
8. As an operator, I want a reopened run to use the target repo it recorded, so that I can reopen a session from repo A while the gate is set to repo B.
9. As an operator, I want a reopen refused before anything is torn down when its recorded target folder no longer exists, so that I don't lose my running fleet for nothing.
10. As an operator, I want a launch refused before teardown when the seats cannot start (for example a harness not logged in), so that my running fleet survives a launch that was never going to work.
11. As an operator, I want each fresh fleet's seats fixed at launch, so that editing the gate mid-run never changes the fleet already running.
12. As an operator, I want each fleet's target fixed at launch, so that a fleet never ends up split across two repos.
13. As an operator, I want the target input on Home to stay editable while a fleet runs, so that I can pick the next repo before clicking New fleet.
14. As an operator, I want a target edited while a fleet runs to apply only to the next launch, so that the running panes keep the repo they were placed in.
15. As an operator, I want the fleet setup panel on Home open by default, so that I can see which harnesses and models the next fleet will launch with.
16. As an operator, I want the gate to restore my last picks, so that a new session on a familiar repo starts on my usual setup without a prompt.
17. As an operator, I want every new session to get its own worktrees and branches, so that sessions on the same repo never collide.
18. As an operator, I want the live run to appear in History as **Running now**, so that History is a complete list of my sessions.
19. As an operator, I want clicking the **Running now** row to take me to the fleet view, so that I don't accidentally tear down and reopen the run I am in.
20. As an operator, I want the run I just left to show **Archiving…** until it is finished, so that I can see it is being saved.
21. As an operator, I want an **Archiving…** row to be unclickable, so that I can't reopen a run whose archive is incomplete.
22. As an operator, I want the row to become **Archived** and reopenable when the background step finishes, so that I can return to it seamlessly.
23. As an operator, I want each run to keep a single row that changes state, so that the list doesn't jump or briefly lose a run.
24. As an operator, I want the Home "Recent sessions" list and the History view to show the same rows and states, so that both agree.
25. As an operator, I want a notice on the live feed when the previous run finishes archiving, so that I know it is safely in History.
26. As an operator, I want a failed launch to return me to Home with the reason next to New fleet, so that I can fix the target, seats or login right there.
27. As an operator, I want a launch refused by the seat check to leave my old fleet running and visible, so that a refusal costs me nothing.
28. As an operator, I want a launch that fails after teardown to show "no fleet running" with the old run archiving into History, so that I know exactly where I stand.
29. As an operator, I want the feed and messages to reset on every launch, so that one run's events never mix with another's. Tasks are not cleared: they reload from the new fleet's target (see Coordination, A).
30. As an operator, I want pane statuses to reset on every launch, so that a dead marker from the old fleet never shows on a new pane.
31. As an operator, I want every pane, including the critic, to come back up after each launch, so that no pane is left dead after a switch.
32. As an operator, I want context gauges to work after a reopen exactly as after a fresh start, so that both kinds of launch behave the same.
33. As an operator, I want a start that fails to leave the UI knowing the fleet isn't up, so that retrying actually brings the panes up.
34. As an operator, I want a crash or quit during archiving to lose nothing, so that the next app launch finishes archiving the staged run.
35. As an operator, I want quitting the app to archive the live run, so that History on disk is complete when the app is closed.
36. As an operator, I want several quick New fleet clicks in a row to each archive cleanly, so that rapid switching never corrupts a run.
37. As an operator, I want New fleet disabled while a launch is in flight, so that a double click doesn't start two launches.
38. As an operator, I want an interrupted turn to be the only loss when I switch sessions, so that everything else (log, transcripts, session ids) is kept.
39. As an operator, I want resumed panes in a reopened run to never be mixed up with the archive of the run before it, so that each archive holds only its own transcripts.
40. As an agent reading `~/.fleetor/runs/` with no app running, I want every archived run to be one self-contained folder with its log, transcripts and manifest, so that History stays readable from the outside.

## Implementation Decisions

- **One backend launch command.** It replaces both the bootstrap command and the reopen command, and takes a launch source: `Fresh`, or `Reopen(run id)`. Its order is fixed:
  1. **Verdict.** Settle the seats and refuse if they cannot start. For a reopen, also refuse if the recorded target is missing or the run cannot be reopened. Nothing is touched before this step.
  2. **Teardown.** Kill every pane, judges included, and drop the live fleet, which closes its database.
  3. **Stage** the old run (see below).
  4. **Build** the new fleet.
  5. **Hand off** the background finish.
- **No reopen request file.** The run id travels in the launch source. Reopen state is never persisted between two commands.
- **The idempotent early return stays only as a guard.** A launch issued while one is already in flight is serialized behind it, not short-circuited into returning the live fleet.
- **Seats and target live on the fleet, fixed at launch, for both sources.** One `seats` value replaces the reopen-only seat field. A fresh launch takes the gate's settled seats and writes them back into the gate. A reopen takes the run's recorded seats (lineage-merged, as D-095 already does) and leaves the gate untouched. Placement reads the fleet's seats, never the gate.
- **Target fixed at launch.** A fresh launch reads the gate's target from config. A reopen uses the target recorded in the run's manifest and does not write config. The in-place target update on a live fleet is removed.
- **D-071 amended.** Editing the target while panes exist is no longer refused. It only changes config, which means the next fresh launch.
- **Archiving splits into two steps, used by all three callers:** launch, app open (crash net, D-085) and quit (WP-28). It stays one archive path, never a second one.
  - **Stage (foreground, fast).** Rename the live database and run identity into a staging folder under the shell slot, keyed by run id. Capture each seat's session id. Clone the transcripts into the staging folder.
  - **Finish (background on launch, synchronous on quit).** Freeze the database to a single file, move it into the runs directory, write the readable export and manifest, upsert the History index, and post the "archived as" notice to the live feed.
  - **Why session ids and transcripts are taken in the foreground.** A reopen shares its lineage's seat directories, so the resumed panes would otherwise be read into the previous run's archive.
- **App-launch sweep.** It finishes every staging folder left by a crash or quit, as well as the live slot it already handles.
- **Parallel kills.** Teardown signals all panes at once and then waits once for the grace period, not once per pane in turn.
- **History listing.** It includes the live run (from the live run identity) and every staged run, each with a state: `running`, `archiving` or `archived`. The row identity is the run id throughout.
- **UI: one `launch(source)`.** It replaces both the start handler and the reopen landing handler.
  - **On call:** switch to the fleet view, bump the generation (remounting the grid), reset statuses, feed and messages, and call the task-state reload hook (Coordination, A).
  - **On success:** mark the fleet started and ready.
  - **On failure:** return to Home and show the error beside New fleet.
  - New fleet is disabled while a launch is in flight.
- **History rows.**
  - **Running now:** navigates to the fleet view.
  - **Archiving…:** inert.
  - **Archived:** launches `Reopen(id)`.
- **Home.** The fleet setup panel is open by default. The target input stays enabled while a fleet runs. The dead target code in the unmounted start gate component is deleted.
- **Unchanged.**
  - The live event pipeline, which still appends and publishes each event immediately.
  - Worktree and branch layout, which is still per session.
  - One live fleet at a time.

## Coordination with the task PRD

`.scratch/task-system/PRD.md` runs in a parallel session. These rules are binding on both and are repeated there.

- **Base.** Both sessions branch from the same commit: the in-flight work on `redesign/ui-polish` committed and `remove/critic-evaluator` merged. Each session works in its own git worktree. On that base the critic and evaluator are gone, so story 31's critic, "judges included" in teardown and "re-arm the judges" no longer apply, and D-073/D-074 are already superseded.
- **Decision numbers.** This PRD owns D-099. The task PRD starts at D-100.
- **Order.** Slice 1 here merges first. Until it does, the task session touches only `crates/` and `prompts/`. It then rebases and wires into `src-tauri/` and `ui/`.
- **File ownership until slice 1 merges.** This PRD owns `src-tauri/src/fleet.rs`, `src-tauri/src/runs.rs`, `src-tauri/src/lib.rs`, `ui/src/fleet/useFleet.ts`, `ui/src/App.tsx`, `ui/src/fleet/api.ts`, `Homepage.tsx`, `RunHistory.tsx` and the History probe. It does not touch `crates/`, `prompts/`, `TaskBoard.tsx`, `SettingsPanel.tsx` or `MissionControl.tsx`.
- **Live checks take turns.** Both sessions share `~/.fleetor/_shell`, so only one runs the real app at a time.
- **A. Tasks are reloaded, not cleared.** `launch()` resets feed and messages itself and calls one `resetTasks()` hook for task state. Slice 1 ships that hook clearing the list, as today. The task PRD replaces its body with a replay of the fleet's task store. A failed launch calls the hook again on returning to Home, where the task PRD shows the gate target's tasks read-only.
- **B. The fleet's target is the task target.** The target fixed on the fleet at launch is the only one the task store may be keyed by, never the gate's. The Build step is where the task PRD opens the store, and Teardown (dropping the fleet) is what closes it. Slice 1 keeps both steps as single functions so that wiring is one line each.
- **C. `tasks.json` is written at stage.** Stage calls one hook, a no-op in this PRD, that the task PRD fills in. The sweep calls the same hook for a staged folder that lacks the file. The hook is given the run id and the target from the run's `run.json`, and a run with no recorded target is skipped with a notice. The History listing takes an archiving or archived run's task count from that file once the task PRD lands, and the Running now row's from the live task store; this PRD keeps the count field on all three row states and its row layout.
- **D. A reopen continues its lineage.** A reopen already records the lineage's `sessions` id. Slice 1 keeps it on the fleet beside the run id so the task PRD can read both. (**2026-10-07:** D-109 makes a reopen follow the startup setting; D-110 forbids anything starting an agent at launch.)

## Testing Decisions

- **What counts as a good test here.** Assert what the operator or an outside reader observes: which seats and target the fleet placed, whether the old panes are gone, and what a run folder in History contains. Don't assert which helper ran in what order, except where an order is the contract. "Refuse before teardown" is such an order, and a test already holds it textually.
- **Seams (confirmed with the operator).** There are three: the launch function, the archive stage and finish steps, and the History render probe. The launch function carries most of the behaviour, and the other two cover what it cannot reach.
- **Seam 1: a launch function that takes no Tauri runtime state.** It runs over a scratch layout, a real pane registry, and the gate hold. The Tauri command is a thin wrapper around it.
  - It follows the existing pattern of the archive-at-launch function, which is already split into a command and a scratch-layout version.
  - Most behaviour is driven through this seam:
    - New-after-reopen places the gate's seats.
    - Reopen places the recorded seats and recorded target.
    - A refusal kills nothing.
    - The old run lands staged, then archived.
    - A missing recorded target refuses.
    - Rapid relaunches each produce one archive.
  - The webview emitter has to be passed in, so this seam can run without an app handle.
- **Seam 2: archive steps.** Stage and finish are tested in the runs module, in the style of its existing rotation tests:
  - a staged-then-crashed run is finished by the launch sweep
  - finish produces a single-file database with no WAL beside it
  - transcripts written after staging are not in the archive
- **Seam 3: History row states** are tested through the existing History render probe: Running now navigates, Archiving… is inert, and Archived reopens.
- **Prior art:**
  - the recorded-seats test for D-095
  - the target-settable test for D-071, which drives a real registry over a real pty (it is rewritten to the new rule)
  - the quit archive test
  - the launch-sweep archive test
  - the lineage tests in the runs module
  - the gate-refusal tripwire, retargeted from the old bootstrap function to the launch function
- **Live check per slice.** In the real app:
  1. Reopen a run, change the gate, click New fleet, and see the new seats spawn.
  2. Run a fleet on repo A, set the target to repo B, click New fleet, then reopen A's session.
  3. Time the switch, and watch the old row go from Archiving… to Archived.

## Out of Scope

- Running more than one fleet at a time.
- Restart not respawning a pane, which is a separate spawn-trigger bug.
- Cleaning up old worktrees and branches.
- Remembering gate picks per target repo, and any "use previous setup?" prompt.
- Batching event appends, which was rejected: about 200 events per run, and batching would delay delivery and break crash safety.
- Evaluator behaviour, including the wake replaying old handoffs on reopen. The operator reports the evaluator is deleted, so it is left untouched here.
- Saving terminal scrollback into the archive.

## Further Notes

- **Slices, in order:**
  1. One launch and UI `launch()`, with synchronous archiving. This alone fixes the reported bug.
  2. Switching repos while running: target editable, setup panel open, dead code removed.
  3. An instant switch: parallel kills, background finish, the staging sweep, and the History row states.
- **Not verified:**
  - That cloning a transcript on APFS is instant through the standard copy call.
  - How long a background finish takes on a large run (today's runs archive to about 10 MB).
- **Evaluator.** Resolved by the shared base in Coordination: work starts after `remove/critic-evaluator` is merged.
