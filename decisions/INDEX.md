# Decision Index

> Standing defaults (Tier 2, as of project start) — from the top of the original `decisions.md`:
> Append-only. One entry per changed Tier-2 default (see building.md §1). Three lines each: what changed, why the default lost, what would reverse it. Tier-1 changes don't belong here — those go through max.
>
> *[historical — pre-pivot. Most of these died with D-030; the living Tier 2 list is building.md §1.]* Worker count 4 · rusqlite/WAL · report schema per handoff §4 · gate retry cap 3 · 60% context checkpoint · fluid roles with home areas · React + Vite UI · MCP tool names per handoff §4 · `~/.fleetor/<repo-key>/` layout · macOS-first · warm two-accent theme (coral attention / gold awareness, no blue).

Note: D-005 is referenced elsewhere in this file (e.g. D-009, D-011) but has no `## D-005` heading of its own in the source, so it has no entry here.

| ID | Phase | Status | Title |
|----|-------|--------|-------|
| [D-001](D-001.md) | meta | alive | Working name: FLEETOR |
| [D-002](D-002.md) | 0 | alive | DeepSeek reached via Anthropic-compatible endpoint, not LiteLLM |
| [D-003](D-003.md) | 0 | alive | Workers run under an isolated `CLAUDE_CONFIG_DIR` |
| [D-004](D-004.md) | meta | alive | `total_cost_usd` ignored; cost computed from token counts |
| [D-006](D-006.md) | 0.5 | alive | Phase 0.5 pty spike: PASS (GO); no companion-window fallback |
| [D-007](D-007.md) | 4 | alive | Spike shell: standalone `src-tauri` package, plain-TS frontend |
| [D-010](D-010.md) | meta | alive | Assign-first: don't gate the first stdin write on the `init` event |
| [D-008](D-008.md) | 1 | alive | Phase 1 report ingested from a transcript block, not MCP |
| [D-009](D-009.md) | 1 | alive | Phase 1 crates: added `fleetor-core`, `fleetor-db`, `fleetor-server` |
| [D-011](D-011.md) | 2 | alive | Phase 2 adopts tokio for the socket + hub (D-005/D-009 deferral resolved) |
| [D-012](D-012.md) | meta | alive | New `Transport` seam lives in its own crate `fleetor-ipc`; wire types in `fleetor-core` |
| [D-013](D-013.md) | meta | alive | One socket, two faces; worker/lead op split enforced structurally |
| [D-014](D-014.md) | 2 | alive | Mid-turn mail framing: coordination, not commands (spike-derived) |
| [D-015](D-015.md) | 2 | reversed | Report-over-MCP and the idle/opportunistic delivery paths deferred within Phase 2 |
| [D-016](D-016.md) | 3 | reversed | Phase 3 quality loop is supervisor-level; report-over-MCP wired at the hub, primary in Phase 4 |
| [D-017](D-017.md) | 4 | alive | GateRunner seam takes config, not discovery; two retry caps default 3; `claim_file` carries the ticket |
| [D-018](D-018.md) | 4a | alive | Phase 4a fleet runner lives in `fleetor-server::runner`; sync supervisor bridged to the async hub via `spawn_blocking`; `WorkerSpec` carries the `AgentProcess` seam so fake and real share one path |
| [D-019](D-019.md) | 4b | alive | Phase 4b: report-over-MCP is the supervisor's primary done-signal; the sync↔async bridge is the shared event log; quality loop stays transcript-scrape; D-015 idle/piggyback deferred to 4c/4d |
| [D-020](D-020.md) | 4c | alive | Phase 4c: the live event bus is a `Store` decorator (`BroadcastStore`) in `fleetor-server`, not a new emit path; persist-then-publish; lag recovers from the DB |
| [D-021](D-021.md) | 4d | alive | Phase 4d: orchestrator-as-lead drives a *dynamic* fleet over the lead MCP face; the lead seat becomes external; `run_fleet` stays for the static path |
| [D-022](D-022.md) | 4e-1 | alive | Phase 4e-1: the fleet server is embedded in the Tauri backend and pushed to a React shell; a scripted `demo` stands in for the live fleet (no tokens) |
| [D-023](D-023.md) | 4e-1 | alive | Phase 4e-1 redesign pass: impeccable-critiqued de-slop, a live Fleet topology view, and resizable/collapsible panels |
| [D-024](D-024.md) | 4e-2 | alive | Phase 4e-2: the real `claude` lead drives a *dynamic* fleet from the pty; the worker factory is backend-selectable (fake→real); the `demo` stand-in is deleted; live spend stays behind a start gate |
| [D-025](D-025.md) | 4f | alive | Phase 4f: the lead can interrupt / restart a worker; the async→sync yank is a kill-by-pid over one `RunnerCommand` channel; a deliberate kill is `Interrupted`, not `Crashed` |
| [D-026](D-026.md) | 4g | alive | Phase 4g (D-015 reversal): the last two mail-delivery paths land — idle→stdin (supervisor) and opportunistic piggyback (shim); mail framing single-sourced in `fleetor-core` |
| [D-027](D-027.md) | 4h | alive | Phase 4h: the shell types worker→lead traffic into the orchestrator TUI (a free pump owns lead ingestion; the app owns the pty, so the "human owns stdin" limit doesn't bind) |
| [D-028](D-028.md) | 4i | reversed | Phase 4i: worker transcripts in the shell; worker stderr finally captured (was dropped), surfacing the reason a headless worker dies |
| [D-029](D-029.md) | 4i | reversed | Phase 4i: a persistent worker pool — 4 headless workers spawned idle at startup, messaged/broadcast by the orch; `session = ticket` relaxed to `session = worker` |
| [D-030](D-030.md) | meta | superseded | The 5-TUI messaging pivot: MCP and the headless supervision model are deleted; all five agents become live `claude` TUIs |
| [D-031](D-031.md) | 1 | alive | Phase 1: the pane contract lands additively alongside the headless one; delivery becomes request/response so the log can never claim a send that didn't happen |
| [D-032](D-032.md) | 1 | alive | Delivery-path audit: only a dead pane may refuse a message, and membership has exactly one source of truth |
| [D-033](D-033.md) | 1 | alive | A message body can escape the bracketed paste that carries it; framing now sanitizes at the pty boundary |
| [D-034](D-034.md) | meta | alive | Delivery is unbounded: every timeout, ceiling and success-heuristic removed from the message path |
| [D-035](D-035.md) | 2 | alive | Phase 2: the headless fleet is unwired from the app, and the socket half of messaging is proven |
| [D-036](D-036.md) | 3 | alive | Phase 3: the pane registry and the `fleet` CLI |
| [D-037](D-037.md) | 4 | alive | Phase 4: the shell becomes five terminals and a message record |
| [D-038](D-038.md) | 5 | alive | Phase 5: the deletion |
| [D-039](D-039.md) | meta | alive | concurrent messages to one pane: a serial writer that drains |
| [D-040](D-040.md) | 6 | alive | Phase 6 (part): the docs stop describing a system that does not exist |
| [D-041](D-041.md) | meta | alive | startup orphan sweep: the teardown that runs when teardown can't |
| [D-042](D-042.md) | meta | alive | the briefs stop being Rust and become `prompts/*.md` |
| [D-043](D-043.md) | meta | alive | the panes stop appending to Claude Code's system prompt and replace it |
| [D-044](D-044.md) | meta | alive | the orchestrator becomes a vision partner, and the workers get a culture |
| [D-045](D-045.md) | meta | alive | `fleet cmd`: a command channel that is not the message path |
| [D-046](D-046.md) | WP-04 | alive | WP-04: the context gauge is `AppCommand::Roster`'s job, not a new channel; the window is our own 128k, not CC's 200k guess; the live gauge never estimates |
| [D-047](D-047.md) | WP-05 | alive | WP-05: the task board is a fold over the event log, and the one arm that never reaches a terminal |
| [D-048](D-048.md) | WP-06 | alive | WP-06: the worktrees stay, because the object database was already shared |
| [D-049](D-049.md) | WP-06 | alive | WP-06: the reviewer is named in the assignment message, not in a field on the block |
| [D-050](D-050.md) | WP-06 | alive | WP-06: `fleet done` runs the check locally, and its exit code still means delivery |
| [D-051](D-051.md) | WP-07 | alive | WP-07: the operator is a name in the record, and `recorded` is one word rather than two |
| [D-052](D-052.md) | WP-08 | superseded | WP-08: the fence is `HOME`, seeded with one file, and the PATH leak it would have left open |
| [D-053](D-053.md) | WP-09 | reversed | WP-09 (budget half): the ledger closes, a Tier 2 cap is set slightly above measured reality, and a conservative prune recovers what little was left |
| [D-054](D-054.md) | meta | alive | the worker window is 500k, stated once, and Claude Code is told rather than left to guess |
| [D-055](D-055.md) | meta | alive | shakedown finding 1: purpose before implementation, said outright |
| [D-056](D-056.md) | meta | alive | the tenets go in as the operator wrote them, and orch finally gets the three attitudes |
| [D-057](D-057.md) | meta | alive | the docs get four homes, an index, and a root CLAUDE.md |
| [D-058](D-058.md) | WP-11 | alive | WP-11: a run is a database, rotated at start, and history is a directory of frozen files |
| [D-059](D-059.md) | meta | alive | the archive is written for an agent to read, and it takes the workers' transcripts with it |
| [D-060](D-060.md) | WP-12 | alive | the self-improving loop: an answer key the orchestrator cannot see, and a grader it cannot change |
| [D-061](D-061.md) | WP-16 | alive | WP-16: dev mode is a key in the operator's config, not a webview preference |
| [D-062](D-062.md) | WP-14 | alive | WP-14: `orch` gets a config dir of its own, and D-059's named gap closes |
| [D-063](D-063.md) | WP-12 | alive | the answer key is absent, not unobtainable, and a frozen grader is not enough |
| [D-064](D-064.md) | WP-13 | alive | WP-13: the done verb is spelled `handoff`, it answers `recorded`, and the orch cap moves to 3,800 |
| [D-065](D-065.md) | WP-17 | alive | WP-17: the write guardrail is a `PreToolUse` hook, writes only, and its Bash half enforces stated intent |
| [D-066](D-066.md) | WP-15 | superseded | WP-15: the evaluator is a `PaneId` in no enumeration, its wake is a bus subscriber, and it reads the live run in place |
| [D-067](D-067.md) | meta | alive | worktrees namespaced by target repo |
| [D-068](D-068.md) | meta | alive | orch answers with `fleet reply`, including to names it does not know |
| [D-069](D-069.md) | meta | alive | the Fence learns about rustup: both toolchain homes are the fleet's own |
| [D-070](D-070.md) | meta | alive | an improve run targets a clone, never the live checkout |
| [D-071](D-071.md) | meta | alive | the target is fixed once a pane exists, and the backend is what says so |
| [D-072](D-072.md) | meta | alive | the layout and the host: a pane's bring-up is a function of values, not of the operator's machine |
| [D-073](D-073.md) | meta | alive | the evaluator is a view in the rail, not a second window (supersedes the window half of D-066) |
| [D-074](D-074.md) | meta | alive | the evaluator is placed, dev mode is read through the layout, and the target is one cell |
| [D-075](D-075.md) | meta | alive | the process-shaping module becomes an internal of placement, and the layout goes on the fleet |
| [D-076](D-076.md) | meta | alive | the Critic: a fourth pane identity, a fifth brief, and a sixth finding category the spike earned |
| [D-077](D-077.md) | meta | alive | `fleet reply` refuses a recipient at parse, and the discriminator is the argv element rather than the first word |
| [D-078](D-078.md) | meta | alive | `fleet broadcast` refuses a recipient too, and the two message verbs move together |
| [D-079](D-079.md) | meta | alive | the Critic gets a socket and a switch, and the switch is in the hub |
| [D-080](D-080.md) | meta | alive | the Critic's brief learns the interview: a remit line, a third citation form, and testimony that can corroborate but never anchor |
| [D-081](D-081.md) | meta | alive | the vendor-binary tier is wired into the conformance suite, runs by default, and shouts when it skips |
| [D-082](D-082.md) | meta | alive | a fenced worker is handed `DISABLE_AUTOUPDATER=1`, because its private HOME had become four Claude Code installations nobody runs |
| [D-083](D-083.md) | meta | alive | the light/dark switch morphs every colour together, the way Mintaka's page turns dark, instead of snapping |
| [D-084](D-084.md) | meta | alive | a target that is not a git repository is `git init`-ed when its first worker starts, instead of sharing one checkout |
| [D-085](D-085.md) | meta | alive | the last session is archived when the app opens, not only when the next fleet starts |
| [D-086](D-086.md) | WP-28 | alive | quitting the app archives the session; launch keeps archiving as the crash net |
| [D-087](D-087.md) | meta | alive | Claude Code first, codex after: the suite is finished on one harness before the second is made to match |
| [D-088](D-088.md) | meta | alive | pane terminals run at line-height 1.0, because a pane holds a TUI and a TUI draws boxes |
| [D-089](D-089.md) | WP-29 | alive | a reopened pane keeps the posture a fresh one gets, and checkpoint 15 now asserts it |
| [D-090](D-090.md) | WP-30 | alive | harness.rs becomes interface-only, and machine-probing joins the trait |
| [D-091](D-091.md) | meta | alive | the orchestrator seat spawns `--permission-mode auto` |
