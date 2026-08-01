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
