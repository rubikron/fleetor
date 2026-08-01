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

## D-005 — Phase 0 scaffold: only `fleetor-cc` + `fleetor-cli`, sync I/O

- **What:** Created just the two crates Phase 0 needs (not the full 6-crate layout), and the spawn/stream path uses `std::process` + threads, not tokio.
- **Why:** YAGNI + reversible. `fleetor-cc`'s spawn builder returns a `Command` whose env/arg logic ports to `tokio::process` unchanged; remaining crates (core/db/server/shim) get added when their phase needs them.
- **Reverses if:** Phase 1 supervision needs async multiplexing — promote spawn to tokio and add crates then.
