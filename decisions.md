# DECISIONS.md

Append-only. One entry per changed Tier-2 default (see BUILDING.md §1). Three lines each: what changed, why the default lost, what would reverse it. Tier-1 changes don't belong here — those go through max.

## Standing defaults (Tier 2, as of project start)

Worker count 4 · rusqlite/WAL · report schema per handoff §4 · gate retry cap 3 · 60% context checkpoint · fluid roles with home areas · React + Vite UI · MCP tool names per handoff §4 · `~/.fleetor/<repo-key>/` layout · macOS-first · warm two-accent theme (coral attention / gold awareness, no blue).

---

## D-001 — Working name: FLEETOR

- **What:** Project branded FLEETOR (was: Fleet).
- **Why:** max's call; "for now" — expect a possible rename before any public artifact.
- **Reverses if:** a real name is chosen; grep-friendly, so a rename is one pass.

## D-002 — DeepSeek reached via Anthropic-compatible endpoint, not LiteLLM

- **What:** Workers point at `ANTHROPIC_BASE_URL=https://api.deepseek.com/anthropic` with `ANTHROPIC_MODEL=deepseek-v4-flash`, auth via `ANTHROPIC_AUTH_TOKEN`. No LiteLLM/OpenRouter proxy (handoff cited those from memory).
- **Why:** DeepSeek ships a native Anthropic-compatible endpoint; removes a proxy hop and a dependency. Verified working in Phase 0.
- **Reverses if:** the endpoint drops tool-use fidelity or a model not on it is needed — slot a proxy behind the same `AgentProcess` seam.

## D-003 — Workers run under an isolated `CLAUDE_CONFIG_DIR`

- **What:** Every worker spawns with `CLAUDE_CONFIG_DIR` pointed at a fleet-owned dir, never the operator's `~/.claude`.
- **Why:** Phase 0 measured the operator's global config leaking in (28 tools, personal MCP servers, global CLAUDE.md) at ~36k input tokens/call and a polluted tool surface; isolation drops it to vanilla CC (~26k, 25 tools, 0 MCP). Also upholds the Tier-1 zero-footprint boundary. Profiles (handoff §7) layer on top of this clean base.
- **Reverses if:** we deliberately want the operator's project `.claude/` to flow through — then inherit that dir explicitly rather than the personal global one.

## D-004 — `total_cost_usd` ignored; cost computed from token counts

- **What:** Real cost = tokens × DeepSeek Flash rates ($0.14 in / $0.28 out per M, cache-read ~0.1×), computed in `fleetor-cli`. The `result.total_cost_usd` field is kept for reference only.
- **Why:** Claude Code computes `total_cost_usd` from its built-in Anthropic price table via model-name mapping — off by ~30× against DeepSeek ($0.13 reported vs ~$0.004 real).
- **Reverses if:** DeepSeek's endpoint starts returning a trustworthy cost field, or published rates change (update the constants in `report.rs`).

## D-006 — Phase 0.5 pty spike: PASS (GO); no companion-window fallback

- **What:** Real `claude` TUI renders faithfully through `portable-pty` + `@xterm/xterm` in a bare Tauri 2 window. Operator-verified exit test: colors, alternate screen (slash-command menus), resize/reflow, scrollback, paste all behave. The WKWebView terminal-fidelity risk (BUILDING §8, risk row 2) is retired; the embedded-orchestrator product shape holds — the companion-window fallback is **not** needed.
- **Why:** This was the one Tauri-specific platform risk (handoff §12). De-risked early, before the shell phase, exactly as the risk register prescribes.
- **Reverses if:** a later CC version or macOS webview regression degrades the TUI unfixably — then escalate to max (product-shape change), not a silent workaround.

## D-007 — Spike shell: standalone `src-tauri` package, plain-TS frontend

- **What:** `src-tauri/` is its own cargo workspace (empty `[workspace]`), deliberately **not** a member of `crates/*` — keeps tauri's dep tree and build profile out of the clean fleetor-cc/cli lockfile. The spike frontend is plain TypeScript + xterm.js (no React), single Vite host at repo root with source under `ui/`.
- **Why:** YAGNI + isolation. A one-terminal spike needs no framework; a framework would be dead weight. Isolating the tauri workspace means a UI-layer build problem can never hold the headless core hostage (BUILDING §3).
- **Reverses if:** Phase 4 (the real shell) folds `src-tauri` into the main workspace and adopts React per BUILDING §2 — expected, and cheap since the spike is throwaway-grade.

## D-010 — Assign-first: don't gate the first stdin write on the `init` event

- **What:** The supervisor writes the ticket to the worker's stdin *immediately* after spawn, then reads the event stream (consuming `init` inline). It does **not** "read stdout until init, then send" as handoff §5 step 2–3 prescribes.
- **Why:** Verified against real Claude Code: in `-p --input-format stream-json` mode, `claude` emits `system/init` only *after* it begins processing the first stdin message. Gating the first write on `init` deadlocks (server waits for init; CC waits for input) — the symptom was a real-CC run that produced zero events until the wall-clock kill. The handoff explicitly said to verify event ordering against the installed CC rather than trust the doc; this is that correction. Multi-turn streaming confirmed in the same probe (the process stays alive after `result`, awaiting more input), which the no-report reprompt relies on. `session_id` is still captured from `init` when it arrives; it just isn't needed to send the first message.
- **Reverses if:** a future CC emits `init` eagerly at startup — the assign-first path still works (init consumed earlier), so no revert needed; only revisit if streaming multi-turn semantics change.

## D-008 — Phase 1 report ingested from a transcript block, not MCP

- **What:** In Phase 1 the worker files its report by ending its final message with a fenced ```` ```fleet-report ```` JSON block; the supervisor extracts and parses it from the transcript (`Report::from_transcript_text`). The assignment message carries the schema and an example. The real `report()` MCP tool (handoff §4) is **not** built yet.
- **Why:** The MCP `report()` channel needs the stdio-shim + unix socket, which is Phase 2's deliverable. Transcript-block ingestion makes the full ticket lifecycle testable end-to-end *now* (against fake-claude and one real-CC run) without pulling Phase 2 forward. Reversible: when the shim lands, `report()` becomes primary and this parser stays as the "no report filed" backstop (handoff §5).
- **Reverses if:** Phase 2 wires the MCP surface — then reports arrive as structured tool calls and the transcript scrape demotes to a fallback (or is dropped if the tool proves reliable).

## D-009 — Phase 1 crates: added `fleetor-core`, `fleetor-db`, `fleetor-server`

- **What:** Grew the workspace from 2 crates to 5, per BUILDING §3's layout: `fleetor-core` (frozen contracts — envelope, events, report, ticket, `Store`/`AgentProcess` seams), `fleetor-db` (rusqlite + migrations, the 5-table sketch), `fleetor-server` (the supervisor loop). `fleetor-cc` gained the streaming `supervised_command`, the `AgentProcess` seam, and the persistent stdin/stdout `Session` driver.
- **Why:** Phase 1 is when supervision, persistence, and the contract layer are first needed (D-005 said add crates when their phase needs them). Only two of the four sanctioned seams are introduced — `AgentProcess` and `Store`; `Transport` and `GateRunner` are deferred to their phases (2, 3) to avoid speculative abstraction (BUILDING §8).
- **Reverses if:** a crate proves to carry no weight — collapse it back. `fleetor-server` in particular is a lib so Phase 4's Tauri app can consume it directly.

- **What:** Created just the two crates Phase 0 needs (not the full 6-crate layout), and the spawn/stream path uses `std::process` + threads, not tokio.
- **Why:** YAGNI + reversible. `fleetor-cc`'s spawn builder returns a `Command` whose env/arg logic ports to `tokio::process` unchanged; remaining crates (core/db/server/shim) get added when their phase needs them.
- **Reverses if:** Phase 1 supervision needs async multiplexing — promote spawn to tokio and add crates then.

## D-011 — Phase 2 adopts tokio for the socket + hub (D-005/D-009 deferral resolved)

- **What:** `fleetor-ipc` (the new `Transport` crate) and `fleetor-server`'s hub run on **tokio**. The Phase 1 sync supervisor (`std::process` + threads + mpsc) is untouched and still passes; only the new Phase 2 surface is async.
- **Why:** Phase 2 multiplexes a socket accept loop + N worker connections + a blocking lead long-poll (`await_events`) + blocking `ask_lead` waiters — the exact "async multiplexing" D-009 said would trigger the promotion, and the runtime BUILDING §2 already names. `tokio::sync::{oneshot, Notify}` model the ask/reply and long-poll cleanly; hand-rolling them on `Condvar` would diverge from §2 for no gain.
- **Reverses if:** the async surface proves not to earn its keep — unlikely now that the hub, shim, and CLI all consume it.

## D-012 — New `Transport` seam lives in its own crate `fleetor-ipc`; wire types in `fleetor-core`

- **What:** The socket **wire contract** (`Request`/`Response`/`Op`/`OpResult`/`LeadEvent`/`Hello`, versioned `WIRE_VERSION=1`) is pure serde in `fleetor-core::wire`. The **transport** (the `Transport` trait — the 3rd sanctioned seam — plus `UnixTransport`, framed `Conn`, and a `Client` helper) is a new crate `fleetor-ipc`, depended on by both `fleetor-server` and `fleetor-shim`.
- **Why:** Contracts belong in core (BUILDING §4) and must stay tokio-free; the I/O belongs behind the seam. A shared crate keeps the shim from depending on the whole server, and isolates the socket in one module (BUILDING §2 IPC row) so a Windows named-pipe impl slots in behind the same trait. Framing is newline-delimited JSON over any async duplex, so the transport can change without touching the framing.
- **Reverses if:** a second transport never materializes and the trait carries one impl forever — collapse `Transport` to the concrete `UnixTransport` (BUILDING §8 over-abstraction guard).

## D-013 — One socket, two faces; worker/lead op split enforced structurally

- **What:** The hub serves workers and the lead on one socket; a connection declares its `Party` via `Hello`. The wire `Op` enum holds both faces, and the hub **rejects** a lead op from a worker connection and vice-versa. `ask_lead` is the *only* worker blocking op; there is no worker↔worker blocking op in the enum at all.
- **Why:** Makes Tier-1.5 (blocking is worker→lead only; no worker↔worker deadlock primitive) a property of the type surface, not a convention. Mail (`dm`/`broadcast`/`send`) is always async and persisted to the `mail` table (source of truth, survives a crash — handoff §11); questions/notices and `ask_lead` reply-waiters are in-memory and transient (a crash drops an in-flight ask, which times out to the park answer).
- **Reverses if:** a future need for lead↔lead or a second blocking worker op appears — revisit the split, but never add a worker→worker blocking op (Tier-1.5 is max's).

## D-014 — Mid-turn mail framing: coordination, not commands (spike-derived)

- **What:** The Stop-hook injects queued mail as a `block` decision whose `reason` is explicitly framed as *in-band teammate coordination that augments the current task*, never as a new instruction. Delivery is via the shim binary's `stop-hook` mode; an empty queue emits nothing (turn ends), and the queue emptying is the loop terminator (no `stop_hook_active` bookkeeping needed).
- **Why:** The Phase 2 spike (`docs/phase2-spikes.md`) showed real Flash **refuses** injected text that reads as an override of its task ("treating it as data, not a command") — correct model behavior. The delivery *channel* is proven; only the *framing* needed care. The MCP + Stop-hook mechanisms themselves passed against installed CC 2.1.220 (fixtures in `tests/fixtures/`), so no escalation to max was needed.
- **Reverses if:** a later CC changes Stop-hook re-injection semantics — the fixtures are version-stamped; re-capture and diff.

## D-015 — Report-over-MCP and the idle/opportunistic delivery paths deferred within Phase 2

- **What:** Phase 2 builds the messaging *core* — `ask_lead`/`reply`, `notify_lead`, `dm`/`broadcast`, `await_events`/`inbox`, and turn-boundary (`drain`) delivery. Deferred: (a) `report()` as an MCP tool (still transcript-scraped per D-008); (b) the "idle → server writes to stdin" and "opportunistic piggyback on a tool result" delivery paths (handoff §5) — only turn-boundary drain is wired; (c) `whos_working_on`/`claim_file`/`backlog_add` (handoff §4), which serve the Phase 3 ownership/quality loop.
- **Why:** YAGNI against the exit test (BUILDING §6): the scripted 3-agent conversation with a mid-turn delivery needs exactly the core above. The deferred items are additive behind the same wire surface.
- **Reverses if:** their phase arrives — report-over-MCP and ownership tools are natural Phase 3 companions; idle/opportunistic delivery lands when the multi-worker supervisor (Phase 4 shell) drives real turn boundaries.

## D-016 — Phase 3 quality loop is supervisor-level; report-over-MCP wired at the hub, primary in Phase 4

- **What:** The quality loop (gate → auto-bounce → peer review → done) lives in `fleetor-server::quality`, driving one worker session over stdin/stdout (reusing the Phase 1 `assign`/`drive_to_report` primitives, now kept alive across bounces) plus a **fresh reviewer session** per review round. Report/verdict are transcript-scraped (`fleet-report` / new `fleet-review` fenced blocks). Separately, `report()` is promoted onto the MCP surface (`Op::Report`, shim `report` tool, hub persists + emits `report-filed` + notifies the lead), proven at the hub layer — but the *supervisor's* ingestion stays transcript-scrape.
- **Why:** The supervisor (stdin/stdout) and the hub (socket) are deliberately unwired until Phase 4's multi-worker runner (BUILDING §6 table; D-015). Making report-over-MCP the supervisor's primary path now would require the supervisor to consume hub events — pulling Phase 4 forward. Keeping the loop supervisor-driven makes the exit test ("buggy ticket bounces, gets fixed, passes review") fully testable against fake-claude + a real shell gate with zero tokens, exactly as Phases 1–2 were.
- **Reverses if:** Phase 4 unifies the runner with the hub — then `report()`-over-MCP becomes the supervisor's primary ingestion and the transcript scrape demotes to the "no report filed" backstop (handoff §5); the `fleet-review` verdict can move onto an MCP tool the same way.

## D-017 — GateRunner seam takes config, not discovery; two retry caps default 3; `claim_file` carries the ticket

- **What:** `GateRunner` (the 4th sanctioned seam) is `run(cwd) -> GateReport`; *which* commands is `GateSpec` config on the concrete `ShellGateRunner` (`sh -c` per check, exit 0 = pass, combined output tailed to 4 KB for the bounce). Gate *discovery* (reading CLAUDE.md/package.json/Makefile — handoff §3) is left to Phase 4. The loop uses two retry caps — `gate_retry_cap` and `review_retry_cap`, both default 3 — escalating to the lead (ticket → `Blocked`) after the cap rather than grinding. Added a `ReviewResult`/`ReviewOutcome` event variant and a `backlog` table (migration 0002); `whos_working_on`/`claim_file`/`backlog_add` route through the hub against the existing `leases` table + the new `backlog` table. `claim_file(path, ticket)` carries the worker's ticket id (a Tier-2 tweak over handoff §4's `claim_file(path)`) so the hub records the lease without tracking slot→ticket state it doesn't yet own.
- **Why:** The seam stays model-agnostic (a future non-shell runner keeps the same face); config-not-discovery is YAGNI for the exit test. Separate caps because gate and review failures are independent loops. Escalate-to-Blocked matches handoff §8 ("capped at ~3 retries then escalate"). Carrying the ticket on `claim_file` avoids inventing hub-side assignment state before Phase 4 wires it.
- **Reverses if:** Phase 4 gives the hub real slot→ticket assignment (drop the `ticket` arg, infer it) or discovery lands (the spec is populated by a scan instead of a literal).

## D-018 — Phase 4a fleet runner lives in `fleetor-server::runner`; sync supervisor bridged to the async hub via `spawn_blocking`; `WorkerSpec` carries the `AgentProcess` seam so fake and real share one path

- **What:** The multi-worker fleet runner is a module in `fleetor-server` (`run_fleet`), not a new crate. It boots the hub on a tokio runtime, connects a stand-in lead `Client`, spawns a `run_lead_loop` (long-polls `await_events`, answers `ask_lead`, sends mid-turn mail), and runs each worker's **sync** supervisor (`run_ticket`, the Phase 1 std-process/threads loop) on `tokio::task::spawn_blocking`. Worker fleet-wiring (the `--mcp-config`/`--add-dir`/`mcp__fleet` allow-list, the `fleet-mcp.json` + `settings.json` Stop hook, and `FLEET_SOCKET`/`FLEETOR_SLOT` env) is written by `WorkerConfig::write_fleet_config()` and lives entirely in `fleetor-cc::spawn` — the one place that knows how a fleet worker differs. `WorkerSpec` holds a `Box<dyn AgentProcess + Send>` with `::real(config,…)` / `::fake(agent,…)` constructors, so the fake-claude integration test and the live `fleetor run --real` drive the identical runner.
- **Why:** The sync↔async seam is the core Phase 4 problem (BUILDING §6 table: supervisor and hub were deliberately unwired through Phase 3). `spawn_blocking` bridges them without rewriting the proven sync supervisor to async or hand-rolling a second event loop — the two channels stay decoupled for 4a (report-over-MCP as the supervisor's *primary* signal, and idle/piggyback delivery, are still deferred; see below). Routing `WorkerSpec` through the `AgentProcess` seam (rather than hardcoding `RealClaude`) is what lets the whole runner be exercised for free against fake-claude's dual-personality worker (stdin-driven **and** socket-speaking at once), keeping the live-CC run a confirmation gate rather than the only test. Runner-in-server (not a 7th crate) because Phase 4's Tauri app consumes `fleetor-server` directly (D-009); a crate would carry no weight (BUILDING §8).
- **Live confirmation (the 4a gate, cost ~cents):** `fleetor run --real` drove one real Flash worker fully wired. Event log showed the full loop: `mcp__fleet__ask_lead` → worker **blocked** → lead answered → worker **working** → `mail` (lead→worker-1) → `Write`/`Bash`/`mcp__fleet__report` → ticket **done**. Two independent proofs the messaging landed in-model: (1) the worker named the file `hello.sh` — exactly the lead's reply, not the decoy `greet.sh` — so the blocking `ask_lead` answer was consumed, not ignored; (2) the transcript shows the Stop-hook mail delivered and the model reasoning *"coordination only, keep to my ticket… my ticket is already done"* — the D-014 framing worked against real CC 2.1.220, not just fixtures.
- **Followups (not bugs):** (a) the worker called `mcp__fleet__report` **twice** (once before, once after acknowledging the mail), yielding two `report-filed` events; ticket state transitions once (idempotent), so it's benign — but mail-arriving-after-a-report can prompt a redundant report, worth smoothing when 4b makes report-over-MCP primary. (b) `fleetor run` has no free fake path (it `bail!`s without `--real`); the fake path is the `fleet_runner` integration test, by design.
- **Reverses if:** 4b lands the D-015/D-016 reversals — report-over-MCP becomes the supervisor's primary ingestion (transcript scrape demotes to the "no report filed" backstop) and idle→stdin / opportunistic-piggyback delivery join turn-boundary drain. If the sync supervisor is ever rewritten to async, `spawn_blocking` drops out; until then it is the seam.

## D-019 — Phase 4b: report-over-MCP is the supervisor's primary done-signal; the sync↔async bridge is the shared event log; quality loop stays transcript-scrape; D-015 idle/piggyback deferred to 4c/4d

- **What:** The base supervisor (`run_ticket`) now treats the worker's `fleet.report` MCP call as its **primary** terminal signal (the D-016 reversal). The hub already persists the report and appends `ReportFiled`; the supervisor watches the shared event log — a new `Store::latest_seq()` cursor captured at assign, scanned via `events_since` at each turn boundary (`mcp_report_step`, with a bounded ~500ms `grace_poll_mcp_report` for the hub task's commit lag) — and finishes on that event *without re-saving or re-emitting*. It's a plain `ReportStep::Terminal { Reported }`, so no new outcome variant. The transcript `fleet-report` scrape (D-008) is demoted to the backstop for a turn that ends with a block instead. Gated behind a `prefer_mcp` flag on `drive_to_report`: `run_ticket` passes `true`; the **quality loop passes `false`** and is unchanged.
- **Why:** This is the tight coupling Phases 1–3 deliberately left open (the runner doc's "4b"). The key design move: the sync supervisor (`spawn_blocking`) and the async hub (tokio) are bridged through the **shared, persisted event log they both already hold** — no cross-runtime channel, no rewrite of either loop (KISS). Report-over-MCP being primary also removes the supervisor's transcript-scrape *duplicate* — note the 4a live double-`ReportFiled` was actually the model invoking `fleet.report` **twice** (two hub emits), a separate worker-behavior/hub-idempotency concern the supervisor can't and shouldn't police; 4b guarantees only that the *supervisor* adds no duplicate (deterministically proven by the `mcp-report` fake: exactly one `report-filed`, no scrape).
- **Quality loop stays transcript-scrape:** it needs the full `Report` **body** to attach gate results and drive bounces; the event-only primary path carries just `status`. Unifying it needs a `Store` report-load read + a `fleet-review` MCP tool — a natural 4c/4d companion (D-016's own reversal note). `prefer_mcp = false` keeps it byte-for-byte as Phase 3 shipped.
- **D-015 idle/opportunistic delivery deferred (again, deliberately):** idle→stdin and piggyback-on-tool-result have no live consumer yet — no idle detection and no real orchestrator — and turn-boundary Stop-hook drain already covers the headless worker (proven in 4a). YAGNI: they land in 4c/4d where live turn boundaries and the orchestrator TUI make them meaningful. Only the D-016 half of the "4b" plan is delivered here.
- **Verified:** `runner_terminates_on_report_over_mcp_no_double_log` — a worker that files only over the socket (no transcript block) closes `Done` via the hub event with exactly one `report-filed`; the existing `fleet-ask` test now exercises the backstop (transcript block) path. Full suite 44 tests, 0 warnings. No live-CC spend: the fix is deterministic at the supervisor layer; a real re-run is confounded by model nondeterminism (a worker may still call the report tool twice).
- **Reverses if:** the quality loop is unified onto report-over-MCP (drop `prefer_mcp`, add the report-load read) — then transcript-scrape becomes a pure backstop everywhere; or if `latest_seq`/`events_since` polling proves too coarse under a busy multi-worker log (promote to a per-slot notify channel from the hub).
