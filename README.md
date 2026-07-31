# FLEETOR

*(working name)*

A local desktop app that runs a professional dev team made of Claude Code instances against a git repo you already have: one Opus orchestrator as tech lead in a real TUI, four DeepSeek V4 Flash workers implementing behind contracts, gates, and peer review. Plugs in on top of the repo — `rm -rf ~/.fleetor/<repo-key>` leaves it untouched.

Tauri 2 · Rust · macOS-first.

## Status

**Phase 0 — PASS (GO).** Flash tool-call fidelity through real Claude Code:
0 fidelity failures in 65 tool calls across 10 varied tickets, 10/10 acceptance
gates passed, ~$0.006/ticket. See `docs/phase0-report.md`. Next: Phase 0.5 (pty
spike). Run the probe with `fleetor probe` (needs `DEEPSEEK_API_KEY` in `.env`).

## Reading order

1. `docs/handoff.md` — architecture: what and why
2. `BUILDING.md` — decision tiers, scaffold, phases, risks: how to build without cornering yourself
3. `DECISIONS.md` — running log of where defaults lost

## Non-negotiables (full list in BUILDING.md §1)

Real unmodified Claude Code processes only. The user's repo is never written to except code on feature branches. Session = ticket. Blocking calls point worker→lead only. Merges land on an integration branch, never trunk.
