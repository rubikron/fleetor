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
Next: Phase 2 (messaging — shim, `ask_lead`, Stop-hook delivery).

## Reading order

1. `docs/handoff.md` — architecture: what and why
2. `BUILDING.md` — decision tiers, scaffold, phases, risks: how to build without cornering yourself
3. `DECISIONS.md` — running log of where defaults lost

## Non-negotiables (full list in BUILDING.md §1)

Real unmodified Claude Code processes only. The user's repo is never written to except code on feature branches. Session = ticket. Blocking calls point worker→lead only. Merges land on an integration branch, never trunk.
