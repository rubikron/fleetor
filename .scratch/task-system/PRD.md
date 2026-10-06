# PRD — Goals and tasks that stay until someone deals with them

Status: draft, not published. Settled in the 2026-10-05 grill (Q1–Q25). Mockups: `docs/notes/fleetor-working-loop.html` (Plan and Mockups sections).

## Problem Statement

As the operator, I hand orch a goal, orch cuts it into tasks, and the workers build them. Today that record is thin and short-lived:

- **Tasks vanish with the session.** The task board lives in the run's database, which is archived when the app next starts. Work left open at the end of a session is gone from view, and the next session starts from nothing.
- **Unfinished work has no hand-off.** A worker that cannot finish has no structured way to say what it did, what is left, and where the partial work is. When a session ends or a pane dies mid-task, the next agent starts cold.
- **The history is a list of statuses and notes.** Receipts, review verdicts, merges and edits happen elsewhere (in messages) and never reach the task, so I cannot read one task and see what really happened to it.
- **Every task must name an owner when it is opened**, so nobody can record work for later, and a worker that spots a problem outside its own task has nowhere to put it.
- **I cannot act on the board.** The Tasks view is read-only; I can only message panes.
- **The drift from real runs is invisible.** Real runs recorded a task marked done 2.8 seconds before its check ran, workers that were never given a task, and reviewed work that was never merged. Nothing on screen shows these gaps.

## Solution

Tasks work like GitHub issues, kept per target repository.

- **Goals and tasks.** A goal is the vision the operator and orch agreed on. orch splits each goal into tasks for the workers. Both live in a task store that belongs to the target repo, outlive any one session, and stay until someone closes them.
- **The chain.** Every task carries one ordered, attributed timeline: opened, taken up, comments, receipts, releases, reviews, edits, flags, status changes and the closing handoff. Nothing in a chain is ever deleted.
- **Release.** A worker that cannot finish releases the task with four required fields (why, done, left, where), and the task goes back to unowned for the next agent.
- **Resume.** At the start of a session, orch goes through the in-progress tasks with the operator, or resumes them directly when the operator has turned on "finish remaining tasks upon startup".
- **Evidence on the task.** Receipts and review verdicts land in the chain, and each task names its reviewer. Flags on the task list and the worker cards show the gaps: assigned but never taken up, done with no receipt, done with no review, goal closed with tasks still open.
- **The operator writes.** From the Tasks view the operator can open goals and tasks, comment, edit, release, close, reopen, reassign, flag, remove and restore, each recorded as `operator`.
- **Curation.** Workers can open tasks they discover; anyone can flag a task as not worth doing; orch can remove destructive or counterproductive ones.

What does not change: the task store is still a record, not a dispatcher. Nothing reads tasks to route, assign, wake or block an agent, a claim is not a lock, and the message path between `fleet send` and a terminal is untouched.

## User Stories

### Goals

1. As the operator, I want orch to record the vision we agreed as a goal, so that the agreement lives somewhere other than orch's conversation.
2. As the operator, I want to open a goal myself from the Tasks view, so that I can set direction without typing into orch's terminal.
3. As orch, I want to open a goal with `fleet task post --goal`, so that the confirmed vision has one home I can point workers at.
4. As orch, I want goals to need only an outcome and vision criteria, so that I do not have to invent technical checks that belong to the tasks under them.
5. As a worker, I want to be refused when I try to open a goal, so that goals stay between the operator and orch.
6. As the operator, I want to see each goal with its tasks grouped under it, so that I can read the decomposition at a glance.
7. As the operator, I want a goal's task counts shown by status as text, so that I can see where things stand without a progress bar claiming completion nobody verified.
8. As orch, I want `fleet handoff --goal 11` to close the goal and add my handoff as its last chain entry, so that the vision and my report on it sit together.
9. As the operator, I want a handoff to list the goal's still-open tasks automatically, so that orch cannot report a goal met while leaving open work unmentioned.
10. As the operator, I want a closed goal with open tasks to carry a flag, so that loose ends stay visible after the handoff.

### Opening tasks

11. As orch, I want to open a task with an outcome, technical criteria, vision criteria, optional instructions, an optional owner and a goal, so that each task is a checkable slice of a goal.
12. As orch, I want to be refused when I open a task without naming a goal, so that I never create work that serves no confirmed vision.
13. As a worker, I want to open a task for a problem I found outside my own task, so that it is recorded for later instead of fixed on the side or forgotten.
14. As a worker, I want tasks I open to start unowned, so that I never hand work to a peer through the board.
15. As a worker, I want to message a peer about a task I opened, so that we can get it done without going through orch for every small thing.
16. As the operator, I want to open a task from a form with the same fields orch uses, so that my tasks read like any other.
17. As the operator, I want to leave a task unowned, so that it waits for whichever session or agent should take it.
18. As the operator, I want an optional "message the owner with this task" choice when I open an owned task, so that opening a task never silently assigns work and messaging stays an explicit act.
19. As anyone, I want a task without a goal to appear under "No goal", so that stray tasks are visible until someone attaches or drops them.
20. As orch, I want to attach a stand-alone task to a goal, so that worker discoveries join the decomposition.

### Identifying tasks

21. As anyone reading the UI, I want tasks shown as `#14`, so that they are short and familiar.
22. As an agent, I want `fleet task show 14` to accept the bare number, so that the shell never swallows my argument as a comment.
23. As an agent, I want a clear refusal when a subcommand that needs a number gets none, explaining that `#` starts a shell comment and to type `14`, so that I can correct myself from the error.
24. As anyone, I want task numbers to be per target repository and never reused, so that `#14` always means the same task in that repo.

### Taking up and finishing

25. As a worker, I want to take up a task with `fleet task update 14 --status in-progress`, so that the chain records "worker-3 took this up" and I become its owner.
26. As a worker, I want the status word to be `in-progress`, so that the CLI accepts the word I would naturally type.
27. As anyone, I want to see both the owner and who last touched a task, so that I can tell who is on it and whether anything is happening.
28. As the owner, I want to be the only one who can mark my task done, so that done remains my claim, made with my name on it.
29. As a reviewer who disagrees with a done claim, I want to record a review verdict rather than flip the status, so that the disagreement is attributed and specific.
30. As a worker, I want to comment on any task, so that findings and progress stay on the task they concern.
31. As anyone, I want a comment to leave the status and owner unchanged, so that activity is never mistaken for a claim of progress.

### Releasing

32. As a worker who cannot finish, I want `fleet task release 14 --why … --done … --left …`, so that the next agent starts from what I learned.
33. As a worker, I want "where" (branch and commit) filled automatically from my worktree, so that the most useful field is never typed wrong.
34. As orch releasing for a crashed worker, I want "where" filled from the task's latest receipt, and the entry marked "released on behalf of worker-2", so that I can hand on work its owner can no longer release.
35. As anyone releasing, I want to be refused when "where" cannot be worked out and I did not pass `--where`, so that no release goes out undocumented.
36. As the operator, I want every release refused unless why, done, left and where are all filled, so that releases are always complete.
37. As anyone, I want a release to appear as one chain entry that also returns the task to planned and clears the owner, so that the chain reads as one event.
38. As the operator, I want a release form in the UI with the same required fields, so that my releases follow the same rule.

### Resuming across sessions

39. As the operator, I want tasks kept per target repo across sessions, so that I can close the app and pick up later.
40. As anyone, I want the Tasks view to show only the current target's tasks, so that repos never mix.
41. As anyone, I want an owner from an earlier session shown as "worker-3, earlier run", and treated as available, so that a fresh pane is never mistaken for the one that did the work.
42. As anyone, I want the chain to show where one session ended and the next began, so that I can tell which work happened when.
43. As the operator, I want orch to go through in-progress tasks with me at the start of a session, so that nothing restarts without my say.
44. As the operator, I want a Settings toggle "finish remaining tasks upon startup", so that orch can resume in-progress work without asking me.
45. As the operator, I want that toggle to resume only in-progress tasks, so that tasks I parked as planned stay parked.
46. As the operator, I want resume to skip tasks whose goal is closed, so that orch asks me about those instead.
47. As orch with the toggle on, I want to announce what I am resuming rather than wait, so that the deviation is visible and not discovered later.
48. As a worker, I want to be told never to take up an in-progress task orch did not hand me, so that two agents do not restart the same work.
49. As a worker resuming a task, I want orch's message to point me at `fleet task show 14`, so that I rebuild my context from the chain and the release.

### Editing and status rules

50. As the creator of a task, I want to edit its outcome and criteria, so that I can correct my own decomposition.
51. As orch, I want to edit tasks workers created, so that I can keep the decomposition coherent.
52. As the operator, I want to edit any task, so that my word is final.
53. As the operator, I want orch refused when it tries to edit my tasks, so that my wording changes only with my approval.
54. As a worker, I want to be refused when I edit a task I did not create, so that no worker quietly lowers a bar; I raise it in a comment instead.
55. As anyone, I want every edit recorded in the chain with the old text kept, so that a changed criterion is never silent.
56. As the creator, orch (for workers' tasks) or the operator (for any task), I want to drop or reopen a task, so that what gets built changes only with the same authority as editing.

### Review and evidence

57. As orch, I want to name a reviewer with `--reviewer 3` when I open or assign a task, so that the duty survives into later sessions.
58. As the operator, I want to see and set the reviewer in the task's side panel and form, so that I know who is on the hook.
59. As a reviewer, I want `fleet task review 14 --met` or `--not-met "…"`, so that my verdict lands in the chain, not only in a reply.
60. As the owner, I want to be refused when I review my own task, so that nobody reviews their own work.
61. As anyone, I want an unrequested verdict recorded and marked as unrequested, so that extra review is welcome but distinguishable.
62. As a worker, I want `fleet done 14 "<check>"` to keep sending orch the receipt, so that orch still gets the push it relies on.
63. As anyone, I want each receipt recorded in the task's chain with exit status, branch, commit, an uncommitted-changes flag and the check command, so that evidence sits on the task.
64. As a worker, I want `fleet done`'s exit code to still mean delivery only, so that a failed record never makes me resend a receipt that arrived.
65. As a worker, I want a warning when the receipt was sent but could not be recorded, so that I know the chain is missing it.
66. As anyone, I want each criterion to show the evidence recorded against it instead of a tick, so that the UI never asserts completion nobody verified.

### Monitoring and flags

67. As the operator, I want a flag on tasks assigned but never taken up, so that idle assignments are visible.
68. As the operator, I want a flag on tasks marked done with no receipt, so that the drift from real runs shows up.
69. As the operator, I want a flag on tasks marked done with no verdict from the named reviewer, so that skipped reviews show up.
70. As the operator, I want each worker card on Home to show its current task, how long it has held it, its latest entry and the same flags, so that I can spot idle workers and skipped steps without opening Tasks.
71. As the operator, I want flags worked out on screen from chain timestamps, so that nothing runs in the background reading the board.
72. As the operator, I want the Activity feed to show task entries from the current session only, including the runs it was reopened from, so that history from other sessions does not flood it.
73. As the operator, I want each task row to show its status, owner, latest entry, comment count, latest receipt and review state, so that the list answers most questions without opening a task.
74. As the operator, I want to filter the task list by owner (including "unowned") and by open, closed or all, so that I can find what I need.

### Curation

75. As anyone, I want to flag a task as not worth doing with a required reason, so that a doubtful task is challenged on the record.
76. As the operator, I want the task list to show how many have flagged a task, so that contested tasks stand out.
77. As orch, I want to remove a task I judge destructive or counterproductive, with a required reason, so that it leaves the working board.
78. As the operator, I want removed tasks hidden from default views but kept with their full chain, so that I can see who proposed what.
79. As the operator, I want to restore a removed task, so that a wrong removal is reversible.
80. As the operator, I want orch refused when it tries to remove my tasks, so that removal follows the same authority as editing.

### History and archive

81. As the operator, I want History to show the tasks a past session touched, so that a past session's view stays meaningful.
82. As an agent reading an archived run, I want a `tasks.json` of the tasks that session touched, so that the archive stays self-contained.

### Briefs

83. As a worker, I want my brief to teach only the task commands I use (show, comment, release, review, flag, open), so that my context is not spent on orch's rules.
84. As orch, I want my brief to add goals, opening tasks, editing, removing, reviewers and the startup triage, so that I know my part.
85. As any agent, I want flag details in `fleet task <sub> --help` and in refusal messages, so that I can learn them when I need them.
86. As the operator with a custom `orch.md`, I want the new startup placeholder to be optional, so that my custom brief keeps working.

### Added in coordination with the launch PRD

87. As the operator with no fleet running, I want the Tasks view to show the gate target's goals and tasks read-only, so that after a restart I can see what is open before I launch.
88. As the operator, I want a "Carried over" banner counting open tasks from other sessions, so that I know what orch will go through with me.
89. As a worker whose session was reopened, I want to be told to re-check my task before continuing, so that I do not keep building something another session took over or finished.
90. As the operator, I want the Running now row in History to show its task count, so that live and archived rows read the same.

## Implementation Decisions

### Model

- **Two kinds of record: goal and task.** A goal carries an outcome and vision criteria; technical criteria are optional. A task carries an outcome, at least one technical and one vision criterion, optional instructions, an optional owner, an optional reviewer, an optional goal (parent) and an optional "converges on" link.
- **Five statuses:** `planned`, `in-progress`, `done`, `dropped`, `removed`. `claimed` is gone with no alias, because nothing old is carried over.
- **Owner** is the pane that took the task up, together with the run and the lineage (`sessions` id) it took it up in. An owner from another lineage is shown as "earlier run" and treated as available. An owner from the same lineage is the same conversation resumed by a reopen, and stays the owner (Coordination, D). Taking a task up sets the owner; releasing clears it.
- **Creator** is recorded when a record is opened and never changes. The edit and status rules check identity against the creator and owner, which are fixed facts. No rule consults task state to permit or refuse an action, except that the task must exist.
- **Chain entries**, each attributed to a pane or `operator` and stamped with time and run id: opened, edited (old and new text), status changed, taken up, commented, released (why, done, left, where, on-behalf-of), receipt (exit status, branch, commit, uncommitted flag, check command, whether the message was accepted), review verdict (met or not met, reason, requested or unrequested), flagged (reason), removed or restored (reason), handoff (built, evidence, open lines, auto-listed open tasks). Run boundaries are not entries; the UI draws them wherever neighbouring entries carry different run ids.
- **Numbers** are per target, assigned inside the same write that records the task, and shown as `#n`.

### Permissions

| Action | Who may |
|---|---|
| Open a goal | orch, operator |
| Open a task | orch (must name a goal), workers (always unowned, goal optional), operator |
| Edit outcome or criteria | the creator; orch also on workers' tasks; operator on any; orch never on the operator's |
| Take up (`in-progress`) | anyone |
| Mark `done` | the owner only |
| Drop, reopen | same as edit |
| Release | anyone; all four fields required |
| Comment, flag | anyone; a flag needs a reason |
| Review | anyone except the owner |
| Remove, restore | orch (not the operator's tasks), operator |
| Set reviewer | orch, operator |
| Handoff | orch only, as today |

Refusals are written for a model to correct itself from, as every `fleet` refusal is today.

### Storage and transport

- **One task store per target**, a SQLite file at `~/.fleetor/targets/<target-slug>/tasks.db`. It is a sibling of the disposable shell directory and the runs archive, outside every pane's write-guardrail roots, so agents can change it only through `fleet task`.
- **Target identity** is the same path-based slug that names worktrees and branches. Moving a repo orphans its tasks; the store records the absolute path so a later re-link is possible.
- **The store uses the existing store interface unchanged** (append, read since, latest sequence), holding only task events.
- **Task events leave the run log.** The run-log task fold, its TypeScript mirror and its tests are removed. The archive digest counts tasks from the task store.
- **The hub holds a second store handle**, written only by the task operations. The task arm still never asks the app to do anything. The handle is opened in the launch's Build step for the target fixed on the fleet, never the gate's, and closed when Teardown drops the fleet (Coordination, B).
- **A second save-then-publish wrapper and follower** over the task store push events to the UI on a separate `fleet://task` event, with the same gap-free replay and lag recovery the run-log pump has.
- **The UI** replays the whole task store for the Tasks view. The Activity feed shows task entries from the current lineage (this run and the runs it was reopened from), matching the run log a reopen copies in. Every launch calls `resetTasks()`, which clears task state and replays the new fleet's store (Coordination, A).
- **No fleet live: read-only.** The Tasks view shows the gate target's store through a read-only backend command that opens the file read-only and never creates it. Every operator control is disabled with "start a fleet to change tasks", because writes still go only through the hub. The mockup's gold "Carried over" banner shows here and after launch, counting open tasks owned from another lineage. A failed launch returns to this state.
- **Operator actions** go through the hub as `operator`, the same path the operator's message composer uses. The UI never writes the store directly.

### CLI

- `fleet task` stays one command, so the verb list and the brief check do not change. Subcommands: `post` (with `--goal`, `--reviewer`, `--parent`, owner optional), `list` (`--open`, `--mine`), `show`, `update` (status, note), `comment`, `release`, `review`, `flag`, `edit`, `remove`, `restore`.
- Task arguments accept a bare number or a quoted `#n`. A missing number gets the shell-comment hint.
- `fleet done` sends the receipt message exactly as today, then records a receipt entry in a separate call after the send returns. The exit code still means delivery only; a failed record prints a warning.
- `fleet task release` reads branch and commit from the current worktree when the releaser is the owner standing in it; otherwise from the latest receipt; otherwise `--where` is required.
- `fleet handoff` gains `--goal`; it closes the goal, appends the handoff to its chain and lists the goal's open tasks into the entry.

### Briefs and settings

- **orch's brief.** The vision is written back as a goal task instead of a file or a message. A goal's vision counts as confirmed in the session that created it, so resuming its tasks needs no new confirmation; brand-new work still does. orch's brief also covers opening tasks under a goal, naming reviewers, editing, removing and the startup triage.
- **Worker brief.** Covers show, comment, take up, done, release, review, flag and opening unowned tasks, plus the rule never to take up an in-progress task orch did not hand over. It also carries the reopen rule: when your session is resumed, run `fleet task show` on the task you were on before continuing, and if you are no longer its owner, stop and ask orch who owns it.
- **Startup setting.** "Finish remaining tasks upon startup" is a key in the operator config, off by default, toggled in Settings. It reaches orch through a new optional `{startup_tasks}` placeholder that expands to "ask the operator first" or "resume in-progress tasks and announce them". It takes effect at the next session start.
- **Measurement.** Brief length is re-measured and restated against D-064's tripwire figures.

### Run archive

- Archiving a run writes a `tasks.json` of the tasks that session touched, beside `events.json`. It is written in the archive's stage step, after Teardown has closed the store, by filtering the old fleet's target store on the run id. The crash sweep writes it for any staged folder that lacks it, finding the store from the target in the run's `run.json` (verified: `runs::begin` records it, best-effort). A run with no recorded target gets no `tasks.json` and a feed notice saying so. History takes an archiving or archived run's task count from this file, and the Running now row's count from the live store filtered by run id (Coordination, C).

### Coordination with the launch PRD

`.scratch/one-fleet-launch/PRD.md` (D-099) runs in a parallel session. These rules are binding on both and are repeated there.

- **Base.** Both sessions branch from the same commit: the in-flight work on `redesign/ui-polish` committed and `remove/critic-evaluator` merged. Each session works in its own git worktree.
- **Decision numbers.** The launch PRD owns D-099. This PRD starts at D-100.
- **Order.** Launch slice 1 merges first. Until it does, this session touches only `crates/` and `prompts/`: the store, hub operations, CLI, briefs and their tests for S1. It then rebases and does S1's `src-tauri/` and `ui/` wiring, so S1 still ends in its end-to-end demo.
- **File ownership until launch slice 1 merges.** The launch PRD owns `src-tauri/src/fleet.rs`, `src-tauri/src/runs.rs`, `src-tauri/src/lib.rs`, `ui/src/fleet/useFleet.ts`, `ui/src/App.tsx`, `ui/src/fleet/api.ts`, `Homepage.tsx`, `RunHistory.tsx` and the History probe. This PRD owns `crates/`, `prompts/`, `TaskBoard.tsx`, `SettingsPanel.tsx` and `MissionControl.tsx`.
- **Live demos take turns.** Both sessions share `~/.fleetor/_shell`, so only one runs the real app at a time.
- **A. Tasks are reloaded, not cleared.** The launch PRD's `launch()` calls a `resetTasks()` hook. This PRD replaces its body with a clear plus a replay of the fleet's task store.
- **B. The fleet's target is the task target.** The gate's target can differ from the live fleet's and stays editable while a fleet runs. The store, the `fleet://task` follower and the Tasks view all follow the fleet's fixed target.
- **C. `tasks.json` at stage.** This PRD fills in the stage hook the launch PRD leaves as a no-op. The run-log digest stops counting tasks.
- **D. A reopen continues its lineage.** A reopen is a new run id with the same `sessions` id. Owners keep their tasks across it, the chain still draws a run boundary, and the startup triage and `{startup_tasks}` apply to fresh launches only. The store stays the truth when an older lineage is reopened after another session moved its tasks on: the owner-only rule refuses a stale `done`, and the worker brief's reopen rule sends the worker to check first.
- **Slice placement of the additions.** The read-only Tasks view and the Running now count ship with S1's wiring; the "Carried over" banner and the reopen rule ship with S2.

### Recorded decisions this amends

- **D-047:** an owner is optional; the operator writes; status words change; identity-based edit and status rules. The "diary, not a dispatcher" rule stands.
- **D-049:** the reviewer becomes a task field that is shown but never acted on.
- **D-058** is unchanged: run databases still rotate; tasks simply no longer live in them.

### Delivery: four vertical slices

Each slice ships its feature end to end: store, hub, CLI, brief lines, the agent-facing UI, and the operator's controls for that same feature.

| Slice | Contents | Demo |
|---|---|---|
| **S1. Goals and tasks** | Task store and `fleet://task`; goals and tasks opened, shown, taken up and done; comments; edit and status rules; the chain. UI: task list, task page with chain, operator new goal / new task form, comment, edit, close, reopen. | The operator opens a goal in the UI, orch splits it, a worker takes up a task, comments and marks it done. The operator edits a criterion, quits and restarts, and everything is still there. |
| **S2. Release and resume** | `release` with four fields; orch startup triage; startup setting; the worker rule. UI: release entry, operator release form, earlier-run owners, Settings toggle. | A worker releases mid-task; the operator restarts with the toggle on; orch resumes it from the release. |
| **S3. Review and evidence** | Receipts in the chain, `--reviewer`, `review`, handoff closing its goal, flags. UI: receipt and verdict entries, reviewer in panel and form, goal handoff entry, flags on rows and worker cards. | A full loop where a skipped review shows as a flag on the task and on the worker's card. |
| **S4. Curation** | Unowned worker tasks, attach to goal, flag, remove and restore. UI: "No goal" group, attach control, flag button and count, remove, restore, removed filter. | A worker opens a destructive task, two panes flag it, orch removes it, the operator restores it. |

## Testing Decisions

- **A good test drives the system the way an agent or the operator does** and asserts on what they would observe: the reply to an operation, the refusal text, the resulting chain, the rendered markup. It does not reach into internal structures.
- **Every test uses a dummy `tasks.db`** in a temporary directory, never the operator's real store.
- **Seam 1: the hub over a real Unix socket with a fake app.** Most behaviour is tested here:
  - each task operation and every permission refusal in the table;
  - required release fields and number assignment;
  - a restarted hub over the same dummy store producing the same board;
  - the separate pipe replaying without gaps.
  - Prior art: the existing task-board and handoff hub suites. They also carry two pins that must keep passing: task operations ask the app for nothing, and a `fleet send` is byte-identical whether the board is empty or full.
- **Seam 2: CLI parsing.** Bare numbers, the missing-number hint, required flags per subcommand, release's "where" resolution, and `fleet done` recording only after the send returns with its exit code unchanged. Prior art: the existing reply-recipient CLI suite.
- **Seam 3: rendering the real UI components under node.** A probe bundled with the repo's own esbuild covers the task list, the task page and its chain (release, receipt, verdict, edit, run boundary), flags, worker cards and the release and new-task forms. The frontend gains no test runner. Prior art: the pane-head, gauge and gate-seat render suites.
- **Briefs.** The existing brief tests keep checking every verb and required placeholder; they gain checks that each role's brief teaches its task subcommands and that `{startup_tasks}` renders both ways and is optional for custom briefs.
- **Each slice ends with its manual live demo**, recorded in the slice's notes.

## Out of Scope

- Syncing with real GitHub Issues.
- Viewing tasks across targets.
- Labels, milestones and per-criterion checkboxes.
- Anything that reads tasks to route, assign, wake, block or rate-limit an agent, including an app-driven wake at startup.
- Notifying the operator outside the app.
- Judging the quality of release, comment or review text beyond requiring it to be non-empty.
- Importing tasks from existing run logs.
- The Critic and the Evaluator, which `remove/critic-evaluator` deletes.
- Deleting old runs, worktrees and worker branches. That is a separate step at cutover, done by neither parallel session. The operator approved deleting old tasks and worktrees from previous projects on 2026-10-05, so task counts on pre-cutover runs need not survive; worker branches live in target repos.

## Further Notes

- **Prerequisite (Q11).** The in-flight codex and wake work is committed, `remove/critic-evaluator` is merged, and this work starts on a fresh `feat/tasks` branch in its own worktree, from the same commit as the launch PRD's branch.
- **Provisional (Q21).** A handoff closing a goal with open tasks is allowed and lists them; revisit after S3's live demo.
- **Decisions to record** as D-100 onward in `decisions/`, with D-047 and D-049 marked amended. D-099 is the launch PRD's.
- **Mockups** for the task list, task page, new-task form and worker cards are in the Mockups section of `docs/notes/fleetor-working-loop.html`. They predate a few decisions: they say `claimed` (now `in-progress`), and they do not yet show release's "why" field, flagging, removal or the reviewer form field.
