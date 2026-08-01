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
`report-filed`. 44 tests, 0 warnings. Next: 4c (a live event bus for the UI),
then 4d (the orchestrator-as-lead TUI) and the Tauri React shell (4e).

## Reading order

1. `docs/handoff.md` — architecture: what and why
2. `BUILDING.md` — decision tiers, scaffold, phases, risks: how to build without cornering yourself
3. `DECISIONS.md` — running log of where defaults lost

## Non-negotiables (full list in BUILDING.md §1)

Real unmodified Claude Code processes only. The user's repo is never written to except code on feature branches. Session = ticket. Blocking calls point worker→lead only. Merges land on an integration branch, never trunk.
