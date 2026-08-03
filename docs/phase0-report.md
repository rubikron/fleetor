> ## ⚠ Historical mechanism, live finding
>
> This measured DeepSeek V4 Flash's tool-call fidelity through **headless** Claude Code, driven by
> a `fleetor probe` command that no longer exists. The *mechanism* is gone with the pivot (D-030).
>
> The *finding* is still load-bearing: it is why the worker panes run Flash. 0 fidelity failures in
> 65 tool calls across 10 varied tickets, 10/10 acceptance gates, ~$0.006/ticket. Nothing has
> re-measured Flash through an interactive TUI, which is an open Phase 6 item.

# FLEETOR — Phase 0 Probe Report

Worker model: `deepseek-v4-flash` via `https://api.deepseek.com/anthropic` · Claude Code `2.1.220 (Claude Code)` · isolated `CLAUDE_CONFIG_DIR`.

## Headline

- Tickets: **10** · verify-passed: **10/10** · worker-claimed success: **10/10**
- **False successes** (claimed done, AC failed): **0**
- Tool calls: **65** · fidelity failures: **0** · **fidelity failure rate: 0.0%**
- Task-level errors (legit non-zero, not fidelity): 1 · orphaned tool calls: 0
- Real cost (token-based estimate): **$0.0639** total · $0.0064/ticket avg · 307592 in / 12619 out tokens

## Per-ticket

| Ticket | Category | Verify | Claimed | Turns | Calls | Fidelity fails | Task errs | Cost |
|---|---|---|---|---|---|---|---|---|
| `T-01-bugfix-average` | bugfix | ✅ pass | yes | 4 | 3 | 0 | 0 | $0.0056 |
| `T-02-add-sub` | add-feature | ✅ pass | yes | 11 | 10 | 0 | 1 | $0.0074 |
| `T-03-grep-slugify` | grep-driven | ✅ pass | yes | 7 | 6 | 0 | 0 | $0.0066 |
| `T-04-rename-mul` | refactor-rename | ✅ pass | yes | 15 | 14 | 0 | 0 | $0.0076 |
| `T-05-new-module` | new-module | ✅ pass | yes | 8 | 7 | 0 | 0 | $0.0062 |
| `T-06-json-edit` | json-edit | ✅ pass | yes | 4 | 3 | 0 | 0 | $0.0055 |
| `T-07-glob-count` | glob-count | ✅ pass | yes | 5 | 4 | 0 | 0 | $0.0056 |
| `T-08-bugfix-clamp` | bugfix | ✅ pass | yes | 5 | 4 | 0 | 0 | $0.0062 |
| `T-09-write-tests` | test-authoring | ✅ pass | yes | 9 | 8 | 0 | 0 | $0.0064 |
| `T-10-refactor-validate` | refactor-multi | ✅ pass | yes | 7 | 6 | 0 | 0 | $0.0069 |

## Fidelity failures by tool

| Tool | Calls | Fidelity failures |
|---|---|---|
| `Bash` | 14 | 0 |
| `Edit` | 16 | 0 |
| `Glob` | 5 | 0 |
| `Grep` | 7 | 0 |
| `Read` | 21 | 0 |
| `Write` | 2 | 0 |

## Verdict: GO

Flash's tool-call fidelity through Claude Code's tool surface is clean on this
suite — **0 malformed/rejected/mismatched calls in 65**, and every worker's
self-reported success matched the independent acceptance gate (0 false
successes). Real cost is negligible (~$0.006/ticket). Per the BUILDING Phase 0
exit test, this is a pass; no need to escalate model choice to max, and the
architecture proceeds on Flash as the worker model.

## Scope & limitations (why 0% is a signal, not a guarantee)

This suite is deliberately *easy*, so read the result as "no fidelity floor
problems," not "Flash is flawless":

- **Toy repo, small tickets, crisp AC.** Real codebases have ambiguity, larger
  files, and longer tool chains where Flash may drift. The 60%-context
  checkpoint and ticket-sizing questions (handoff §14) are untested here.
- **`MultiEdit` was never exercised** — Flash did the T-04 rename with sequential
  `Edit`s instead. It's *untested*, not passed. Same for `Task`, `WebFetch`,
  `NotebookEdit`. Coverage is Read/Edit/Bash/Grep/Glob/Write only.
- **One-shot `-p` runs, not the real messaging loop.** `ask_lead`, mid-turn mail,
  Stop-hook delivery, and multi-session tickets are Phase 1–2 surface.
- **Permission posture was `acceptEdits` on a throwaway repo**, not the
  PreToolUse path-guard the real worker needs (Tier 1.8). The wedge-on-permission
  failure mode (handoff §5) is therefore *not* exercised by this probe.
- **`n=1` per ticket.** No variance measurement; a flaky fidelity failure at low
  rate wouldn't show. Re-run with repeats before trusting a hard rate.

Extend `tests/fake-claude` and this probe with harder tickets as real failure
shapes surface in later phases (BUILDING §5).

## Notes

- `total_cost_usd` from Claude Code is Anthropic-priced and **fabricated** against
  DeepSeek — ignored. Cost above is token-based at Flash rates ($0.14/$0.28 per M,
  cache-read at 0.1×). Observed per-turn system-prompt floor: ~24–30k input tokens.
- A `tool_result` with `is_error:true` from Bash is a task-level error (e.g. a
  failing test), not a fidelity failure — classified separately. The one task
  error (T-02, `Exit code 5`) was a transient test run the worker recovered from.
- **Config isolation is load-bearing.** A non-isolated worker inherited the
  operator's `~/.claude` (28 tools, personal MCP servers, global CLAUDE.md) at
  ~36k input tokens/call; `CLAUDE_CONFIG_DIR` cut that to vanilla CC (~26k, 25
  tools, 0 MCP). Workers must always run isolated — for measurement validity,
  cost, and the Tier-1 zero-footprint boundary.
