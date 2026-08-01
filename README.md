# FLEETOR

*(working name)*

A local desktop app that runs a professional dev team made of Claude Code instances against a git repo you already have: one Opus orchestrator as tech lead in a real TUI, four DeepSeek V4 Flash workers implementing behind contracts, gates, and peer review. Plugs in on top of the repo — `rm -rf ~/.fleetor/<repo-key>` leaves it untouched.

Tauri 2 · Rust · macOS-first.

## Status

**Phase 0 — PASS (GO).** Flash tool-call fidelity through real Claude Code:
0 fidelity failures in 65 tool calls across 10 varied tickets, 10/10 acceptance
gates passed, ~$0.006/ticket. See `docs/phase0-report.md`. Run the probe with
`fleetor probe` (needs `DEEPSEEK_API_KEY` in `.env`).

**Phase 0.5 — PASS (GO).** Real `claude` TUI renders faithfully through
`portable-pty` + xterm.js in a bare Tauri 2 window: colors, alternate screen,
resize, scrollback, paste all verified. The WKWebView terminal risk is retired
and the embedded-orchestrator product shape holds. Run the spike with
`npm install && npm run tauri dev`.

**Phase 1 — PASS (GO).** The supervisor drives a full ticket lifecycle —
spawn → assign (over streaming stdin) → turn-end detection (`result`) → report
ingestion — persisted through SQLite and observable as an append-only event
log plus a raw per-worker transcript on disk. Verified against `fake-claude`
(all four scenarios: happy, no-report+reprompt, bad-report, watchdog kill) and
once against real Claude Code Flash (worker wrote + tested `greet.sh`, filed a
well-formed `fleet-report`, ticket → done). Run it with
`cargo run -p fleetor-cli -- supervise` (add `--real` for a live worker).

**Phase 2 — messaging model complete; exit test GREEN.** The fleet socket and
its routing hub: `ask_lead`/`reply` (the one worker→lead blocking call), async
`dm`/`broadcast` peer mail persisted to SQLite, the lead's `await_events`
long-poll, and mid-turn mail delivery via the worker's Stop hook. A new
`Transport` seam (`fleetor-ipc`, unix socket) and the `fleetor-shim` binary
(stdio MCP ↔ socket, one binary, slot via env) carry it. The MCP handshake and
Stop-hook re-injection were **spiked against real CC 2.1.220 first**
(`docs/phase2-spikes.md`, fixtures in `tests/fixtures/`) — both mechanisms pass;
the spike also found that mid-turn mail must be framed as coordination, not
commands (D-014). Verified by: the hub routing tests, a cross-process shim
bridge test (real binary over the socket), a Stop-hook drain test, and the
**BUILDING §6 exit test** — a scripted 3-agent conversation with a mid-turn
delivery, against fake-claude *processes* over the socket. 28 tests, 0 warnings.
The one remaining live-worker confirmation (shim + Stop hook wired into
`WorkerConfig`, driven by a real multi-worker runner) landed in **Phase 4a** —
see below.

**Phase 3 — quality loop complete; exit test GREEN.** The loop that closes
implement → gate → fix → review → done **without the lead** (handoff §8). A
`GateRunner` seam (the 4th and last) with a `ShellGateRunner` runs the exit gate
in the worktree; a failing gate auto-bounces the failing checks back to the
still-alive worker, capped at 3 retries then escalated. On green, a **fresh
reviewer** agent gets the diff + AC and approves or requests changes (also
bounced, also capped). `report()` is promoted onto the MCP surface (shim tool +
hub persistence), and the ownership tools `whos_working_on`/`claim_file`/
`backlog_add` land against the `leases`/`backlog` tables (D-016, D-017). Verified
by the **BUILDING §6 exit test** — a deliberately buggy ticket bounces, gets
fixed, and passes review against fake-claude + a real shell gate, no tokens —
plus review-changes→fix→approve, retry-cap escalation, and hub-level
report/ownership tests. Run it with `cargo run -p fleetor-cli -- quality`. 42
tests, 0 warnings.

**Phase 4a — fleet runner GREEN; live-CC messaging confirmed.** The runner
that finally unifies the two channels Phases 1–3 kept apart: `run_fleet`
(`fleetor-server::runner`) boots the hub, connects a stand-in lead loop, and runs
each worker's **sync** supervisor on `tokio::task::spawn_blocking` — bridging the
std-process supervisor to the async hub without rewriting either (D-018). Worker
fleet-wiring (`--mcp-config`/`--add-dir`/`mcp__fleet`, the generated
`fleet-mcp.json` + Stop-hook `settings.json`, `FLEET_SOCKET`/`FLEETOR_SLOT`)
lives in `WorkerConfig::write_fleet_config()` in `fleetor-cc::spawn`. `WorkerSpec`
routes through the `AgentProcess` seam (`::fake`/`::real`) so one runner serves
both the free integration test and the live run. Verified by an end-to-end
fake-claude test (a dual-channel worker: stdin-driven **and** socket-speaking at
once — `ask_lead` + mid-turn mail) and by the **live confirmation gate**:
`fleetor run --real` drove one real Flash worker fully wired — event log showed
`ask_lead` → **blocked** → lead answered → `mail` → `Write`/`Bash`/`report` →
**done**; the worker named the file `hello.sh` (exactly the lead's reply, not the
decoy `greet.sh`), and the transcript shows it treating the Stop-hook mail as
coordination, not a command (D-014 holds against real CC 2.1.220). 43 tests,
0 warnings.

**Phase 4b — report-over-MCP is the supervisor's primary done-signal.** The
tight coupling 4a left decoupled (D-016 reversal). The worker's `fleet.report`
MCP call is now terminal: the hub persists it and appends `ReportFiled`, and the
**sync** supervisor reads that from the shared, persisted event log (a new
`Store::latest_seq` cursor + `events_since` at each turn boundary) — bridging the
`spawn_blocking` supervisor to the tokio hub with **no cross-runtime channel**.
The transcript `fleet-report` scrape drops to a backstop. This also removes the
supervisor's duplicate `ReportFiled` (the 4a double-log). The quality loop stays
transcript-scrape (it needs the full report body to bounce — a 4c/4d
companion), and the D-015 idle/opportunistic mail paths stay deferred to 4c/4d
where a real orchestrator exercises them (D-019). Verified deterministically: a
worker that files only over the socket closes `Done` with exactly one
`report-filed`. 44 tests, 0 warnings.

**Phase 4c — live event bus for the UI.** The push side of the event log. Every
state change already funnels through the one chokepoint `Store::append_event`;
`BroadcastStore` (`fleetor-server::bus`) is a **decorator** over the `Store` seam
that delegates all persistence and, right after a successful append, publishes
the row's real `seq` on a `tokio::sync::broadcast` bus — so both the sync
supervisor and the async hub feed subscribers live with **zero change to the
proven Phase 1–4b loops** (unwrap it and you're back to a plain store). The DB
stays the source of truth: **persist-then-publish**, best-effort send, and an
`EventFollower` (`follow(after)`) that streams the DB snapshot then live events
gap-free and dup-free — a follower that lags the bounded ring recovers by
re-reading `events_since` (D-020). `fleetor run` now prints events live through a
follower instead of dumping the log afterward. Verified by `event_bus.rs`: a live
subscriber receives *exactly* the persisted log (same seqs, no gaps/dupes), a
late subscriber gets full history then live across a seamless boundary, and a
follower flooded past `BUS_CAPACITY` recovers every event from the DB. 48 tests,
0 warnings.

**Phase 4d — orchestrator-as-lead over a dynamic fleet.** The scripted
`LeadPolicy` stand-in is replaced by a real lead seat that drives the fleet
through **MCP tools**. New lead-facing ops `assign` and `fleet_status` join
`await_events`/`reply`/`send`; the shim gains a **lead role**
(`FLEETOR_ROLE=lead` → `Party::Lead`) with its own tool face. The heart is
**dynamic assign**: workers are spawned *on demand* as the orchestrator calls
`assign` over the hub — `run_dynamic_fleet` (runner) bridges the hub→runner
command over a plain mpsc channel, each assign becoming a supervised worker on
`spawn_blocking` (the 4a seam, now fed dynamically). The lead seat is external —
a `driver` future runs the session; a static hub with no runner answers `assign`
with a clean error. `run_fleet` (the static path) is untouched, so 4a/4b/4c hold.
Verified by `orchestrator.rs`: a fake orchestrator assigns a ticket over the hub
(the worker exists only because of the assign), answers its `ask_lead`, steers it
with mail, and polls `fleet_status` until the board shows done — plus a
cross-process test that the real shim binary in lead role bridges `assign` MCP →
the hub. **Deferred:** the live Opus-in-the-seat run (a `--real` gate),
`interrupt`, and the pty/xterm rendering of the TUI (4e). 55 tests, 0 warnings.

**Phase 4e-1 — the observable shell (no tokens).** The fleet server, embedded in
the Tauri backend and pushed to a **React** window. `src-tauri` takes plain path
deps on the proven crates and a `fleet` module that opens the store, wraps it in
the 4c `BroadcastStore`, binds the hub on the fleet socket (idle until 4e-2), and
runs an `EventFollower` that streams every appended event to the webview as the
`fleet://event` Tauri event — the same 4c seam, now terminating in React instead
of `println!`. The UI (migrated off the 0.5 plain-TS spike) renders the top bar,
navigation spine, always-visible **dashboard band** (orchestrator + 4 workers +
queue), **ticket board**, and the live **event feed**; a `useFleet` hook seeds
the board from a snapshot and reduces the stream. Invoke actions (`fleet_assign`,
`fleet_demo`) write through the same store, so they stream back for free. The 0.5
pty terminal pane is carried over unchanged (a plain idle `claude`, **not yet**
the wired lead). A scripted `fleet::demo` (a pure `plan()` of ticket/worker/gate/
review/report steps applied on a timer) animates the whole shell with **zero
tokens and no child processes** — the 4e-1 stand-in for a live fleet. Verified by
`cargo check` + a `demo`-plan unit test (one event per step; board ends T-101
done, T-102 in review) and `tsc --noEmit && vite build` (43 modules); the 55
crate tests are untouched. The live visual check (`npm run tauri dev`) is the
operator-run gate. **Deferred to 4e-2:** embedding `claude` as `Party::Lead`
driving `run_dynamic_fleet` over the hub bound here, the live Opus confirmation,
and real repo/gate/cost values in the top bar. See D-022.

**Phase 4e-1 redesign pass — de-slop + Fleet view + resizable panels.** A
presentation-only refinement (no backend change), driven by an `impeccable`
design critique: the side-tab accent bars (the top AI-slop tell) are gone,
the **blocked** signal is unified to coral end-to-end, motion is reserved for
that one signal, and the a11y floor is raised (AA contrast, keyboard focus, tab
roles). A **new Fleet topology view** (`ui/src/views/FleetGraph.tsx`) — a
hand-rolled SVG with the lead centred, workers in an asymmetric signal-driven
layout, and a message dot travelling each live edge (`assign`/`ask_lead`/ticket)
— reads off the same 4c bus. Terminal ↔ board is now **resizable + collapsible**
(`react-resizable-panels`, persisted sizes). A faithful sign-off mock is at
`docs/ui-redesign-preview.html`. Verified: `tsc --noEmit && vite build` clean
(45 modules); crate tests untouched. See D-023.

**Phase 4e-2 — the live lead over a dynamic fleet.** The stand-in is gone: the
hub is now bound **dynamic** (via `run_dynamic_fleet`), so a supervised worker is
spawned the moment the lead calls `assign` over the socket. The lead seat is the
operator's real `claude` TUI in the pty — it spawns with `FLEETOR_ROLE=lead`, a
generated `fleet` MCP config (the shim), `--add-dir`, and `mcp__fleet` on the
allow-list, running the operator's own Opus in a throwaway git **scratch repo**
under `~/.fleetor/_shell` (Tier-1 blast radius). The worker `factory` is testable
and backend-selectable: **fake-claude by default** (a free proof path that dials
the hub) and real DeepSeek Flash behind `FLEETOR_WORKER_BACKEND=real` — the choice
is announced on the feed, never silent. The `demo` command/module is deleted; the
top bar shows the **real** target/branch/worker-backend/gate (`fleet_config`); and
spawning the lead (which spends tokens) sits behind an explicit **start-session
gate** in the terminal pane. Token posture this phase: **wired, zero live spend** —
the factory is proven by unit tests, the whole thing compiles and the 55 crate
tests hold; the first live Opus+worker run is the operator's to trigger. Verified:
`cargo test` (2 new factory tests + 55 crate tests green), `tsc --noEmit && vite
build` clean. See D-024.
Next: the first live run (operator-triggered) — start the session, let the lead
assign a scratch ticket, watch the band/board/graph animate; then real Flash
workers, then top-bar cost/token metering.

## Reading order

1. `docs/handoff.md` — architecture: what and why
2. `BUILDING.md` — decision tiers, scaffold, phases, risks: how to build without cornering yourself
3. `DECISIONS.md` — running log of where defaults lost

## Non-negotiables (full list in BUILDING.md §1)

Real unmodified Claude Code processes only. The user's repo is never written to except code on feature branches. Session = ticket. Blocking calls point worker→lead only. Merges land on an integration branch, never trunk.
