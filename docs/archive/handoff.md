> ## ⚠ SUPERSEDED — historical record
>
> This is the **original architecture handoff**, describing the headless system: one TUI
> orchestrator, four `claude -p` workers under a sync supervisor, an MCP shim, a ticket board with
> gates and peer review. **All of it was deleted in the 5-TUI pivot (D-030).** Nothing here
> describes the code.
>
> It is kept because it is the *why* behind decisions that are still in force — the repo-boundary
> test, real-unmodified-CC-only, worker isolation, the four-seam discipline — and because
> `decisions.md` refers back to it. For how the system works now, read
> **`docs/fleet-comms-map.md`**.

# FLEETOR — Architecture Handoff

**For:** Fable, for detailed architectural mapping
**Supersedes:** the earlier brainstorm (several decisions changed after discussion — Flash instead of Pro, desktop shell, out-of-repo state)

A local desktop app that drives a team of Claude Code instances against a git repo the user already has. One Opus orchestrator acting as tech lead, four DeepSeek V4 Flash workers doing the implementation.

**What's decided** is in §1–§11. **What's still open** is §14 — those are the questions where a wrong guess is expensive, and they want real answers rather than defaults.

---

## 1. Decisions

> **Note for Fable:** these are *tiered*, not uniformly locked. `BUILDING.md` defines which are invariants, which are defaults that may be revisited with a logged decision, and which are free. Don't treat this table as handcuffs — the builder must be able to route around a bad assumption without asking permission for every swerve.

| Decision | Choice | Why |
|---|---|---|
| Orchestrator model | Claude Opus | Planning, contracts, review, taste |
| Worker model | DeepSeek V4 Flash | 284B MoE / 13B active, 1M ctx, τ²-Bench 95.0, ~$0.09–0.14 per M in |
| Every agent is | A real Claude Code process | No custom agent loop. The harness is *around* CC, never inside it |
| Orchestrator UX | Real `claude` TUI in a pty, embedded via xterm.js | Slash commands, plan mode, skills all work because it genuinely is Claude Code |
| Daemon | The desktop app's main process | Collapses the hub-vs-daemon question — workers survive orchestrator restarts for free |
| Fleet state | `~/.fleetor/<repo-key>/`, outside the repo | The tool plugs in on top of an existing project; zero footprint |
| Work isolation | One git worktree per worker slot, branch per ticket | Standard, cheap, revertable |
| Merge target | An integration branch, never trunk | User opens the real PR themselves, by hand |
| Session lifetime | One ticket (implement → review → fix → merge) | Replaces the process to wipe context; there is no "clear" command |
| Peer review | Different agent, fresh session, diff + AC only | Biggest quality lever, and nearly free at Flash prices |
| Worker↔worker messaging | Allowed, async only, fully logged | Real teams don't route everything through the lead |
| Blocking calls | Worker→lead only | Worker→worker blocking deadlocks a 4-agent fleet |
| CLAUDE.md | Read as guidance, never written to | Repo belongs to the user |

---

## 2. Runtime model

Three process types, one socket.

```
┌─ Desktop app (main process) ─────────────────────────────┐
│  FLEET SERVER — state.db, routing table, unix socket     │
│  owns every child process below                          │
└──┬──────────────────────────────────────┬────────────────┘
   │ pty                                  │ stdin/stdout (NDJSON)
   ▼                                      ▼
[ claude — Opus, interactive ]      [ claude -p ×4 — Flash, headless ]
   │ MCP shim → socket                    │ MCP shim → socket
   └──────────────┬───────────────────────┘
                  ▼
            fleet server
```

- **Orchestrator** — `claude` in a pty, cwd = repo root, rendered with xterm.js. Unmodified TUI.
- **Workers** — `claude -p --input-format stream-json --output-format stream-json`, cwd = own worktree, `--add-dir` pointed at the fleet directory so they can read tickets and knowledge.
- **Fleet server** — in-process with the app. Two client transports: stdio MCP for the orchestrator, unix socket for worker shims. Same tool surface, different faces.

Two channels per worker, deliberately separated:
**stdin/stdout = the lead speaks to the worker** (task injection, steering, queued mail).
**MCP = the worker speaks to the lead** (questions, reports, DMs).

---

## 3. Directory layout and the repo boundary

```
~/.fleetor/<repo-key>/          keyed by remote URL, or path if no remote
  state.db                    tickets, claims, leases, mail, events
  tickets/T-041.md            human/agent-readable mirror of state
  knowledge/                  fleet-owned; mirrors the repo tree
    src/api/notes.md          → auto-injects when a worker works in src/api/
  logs/worker-2/T-041.jsonl   raw transcripts, never enter Opus context
  profiles/*.json
  wt/worker-1..4/             git worktrees
```

**Correctness test for the layering:** `rm -rf ~/.fleetor/<repo-key> && git worktree prune` must leave the repo exactly as it was, minus any feature branches the user chose to keep. If anything else needs undoing, the boundary is wrong.

The only thing FLEETOR ever writes into the repo is **application code on feature branches**. No `.gitignore` edit, no scaffolding commit, nothing in `git status`, no CLAUDE.md commits. The sole trace is `.git/worktrees/` entries, invisible to status and prunable.

**Knowledge is fleet-owned, not repo-owned.** CLAUDE.md is read for guidance like any Claude Code session would. What workers *learn* goes to `~/.fleetor/<key>/knowledge/`, mirroring the repo tree so path-scoped entries load automatically when a worker touches that area — the same ergonomic as nested CLAUDE.md, without writing to the user's project. Entries are proposed by workers and merged only after the lead or user accepts them (§9).

**Gate discovery, not gate configuration.** On first attach the orchestrator reads CLAUDE.md, `package.json` scripts, Makefile, and CI workflow, then proposes the exit-gate command set for confirmation. The repo already knows how it's built and tested; don't make the user restate it.

---

## 4. Fleet MCP surface

### Orchestrator-facing

| Tool | Behaviour |
|---|---|
| `fleet_start()` / `fleet_status()` / `worker_restart(slot)` | Lifecycle |
| `assign(worker, ticket, files_owned[], budget, profile?)` | Formats a ticket into a user message, writes to stdin |
| `send(worker, text)` | Mid-flight steering; queued, no interrupt |
| `interrupt(worker, reason)` | The only authority that may yank a busy worker out of its turn |
| `await_events(timeout_s)` | **Blocks.** Returns questions, reports, blocks, gate failures. This is how the lead listens without burning turns |
| `inbox(peek)` | Non-blocking drain |
| `reply(event_id, text)` | Unblocks a worker sitting in `ask_lead` |
| `report(task_id)` / `diff(worker)` / `traffic(since)` | Inspection |
| `transcript(worker, tail_n)` | Raw log — on explicit demand only |

### Worker-facing

| Tool | Behaviour |
|---|---|
| `ask_lead(question, options?)` | **Blocks** until answered or timeout |
| `notify_lead(text)` | Fire-and-forget progress |
| `report(status, summary, …)` | Structured completion |
| `dm(worker, text)` / `broadcast(text)` | Async, delivered at the peer's next turn boundary |
| `whos_working_on(path)` | Cheap conflict check |
| `claim_file(path)` | Ownership extension request; may be denied |
| `backlog_add(text)` | Where out-of-scope discoveries go instead of into the diff |

### Report schema

Keeps Opus context clean. Raw logs stay on disk, greppable via `transcript()`.

```
ticket
status      done | blocked | needs-decision | failed
summary     ≤200 words
branch, diffstat
gate        tests / typecheck / lint / build results
decisions   choices the lead might disagree with
questions   blocking
risks
followups   → backlog
```

### Message envelope

```
{ id, from, to, kind: dm|fyi|answer|ask, body, ref?: ticket|branch, ts }
```

---

## 5. Messaging mechanics

### Starting work

There is no "start" RPC — **in Claude Code a turn is triggered by a user message**. `assign()` is a thin wrapper that formats a ticket into one and writes a line to stdin.

1. Spawn the worker, cwd = its worktree, env pointing at Flash
2. Read stdout until the `system` init event → gives `session_id`
3. Write one NDJSON line (flush after every write):

```json
{"type":"user","message":{"role":"user","content":[{"type":"text","text":"<ticket + AC + owned files>"}]}}
```

4. Events stream back: `assistant`, `user` (tool results), then `result` — **`result` is the turn-end signal** and drives everything else.

> Verify the exact envelope and event names against the installed Claude Code version rather than trusting this doc.

### Mail delivery

| Recipient state | Mechanism |
|---|---|
| **Idle** | Fleet server writes the message straight to stdin. New turn. |
| **Mid-turn** | Queue it. The worker's `Stop` hook drains at turn end — returns `{"decision":"block","reason":"<mail>"}` so the worker continues with the mail in context. |
| **Opportunistic** | If a mid-turn worker calls any fleet tool, piggyback pending mail onto that tool's result. Free delivery, no waiting. |

The orchestrator uses the same `Stop`-hook trick — it can't be injected into because the human owns its stdin. `await_events` is the primary mechanism during active supervision; the Stop hook catches drains; `UserPromptSubmit` prepends anything that arrived while fully idle.

### Blocking rules

- `ask_lead` blocks — the lead is human-attended and responsive. Timeout returns *"no answer, use your judgment"* or *"park it"* so a worker never deadlocks on an AFK user.
- **No blocking worker→worker call exists.** If W2 can't proceed without W3, it escalates to the lead, who has `interrupt`. Human analogue: you don't yank a colleague out of flow, you go to the lead or route around.
- `dm` and `broadcast` return immediately.

### Three things that bite

1. **A headless worker with default permissions hangs on the first permission prompt.** No human on that TTY. Set permission mode / allowed tools at spawn, or auto-approve via a `PreToolUse` hook — which is exactly where the blast-radius rules live. Most likely thing to silently wedge the first prototype.
2. **You don't clear a worker's context, you replace the process.** No wipe command exists. "worker-2" is a logical slot; the OS process is per-ticket. Respawn *is* the wipe.
3. **Turns that end without a report.** Don't rely on the charter alone — when the server sees `result` with no report filed, it writes back *"You ended your turn without filing a report — file one now."* Cheap, self-healing, also catches a worker that thinks it's done before the gate ran.

---

## 6. Worker lifecycle and context policy

**Session lifetime = ticket lifetime.** Not per message (loses the thread mid-work), not forever (context rot). One session spans implement → review → fix → merge, so review feedback lands with the worker that still remembers why it wrote the code. Then the process dies.

Per ticket: spawn `claude` with `--fork-session --resume <warm_base_session_id>` so repo orientation is shared rather than re-derived. *(Whether this actually saves anything through the DeepSeek endpoint is an open question — §14.)*

**Three memory tiers:**

| Tier | Lifetime | Contents |
|---|---|---|
| Task context | Dies with the ticket | Conversation, tool calls, failed attempts |
| FLEETOR knowledge | Persistent, curated, reviewed | Gotchas, invariants, "we tried X, it failed because Y" |
| Charter | Stable | Slot identity, worktree, report contract |

**Never auto-compact mid-ticket.** Compaction silently drops the reasoning that got you here. Policy: at ~60% context, checkpoint → write handoff note → fresh session resumes from the note. If this fires often, tickets are too big.

**Workers explore.** Exploration is the strength of a normal Claude Code session — grep, read, notice conventions. A worker handed a pre-digested file list produces literal, brittle work. The ticket pins down *what done means*, what files it owns, and what contracts it must honor. Not how. SessionStart injects the ticket, which is just the message a human would have typed; everything else loads the way it always does.

**Guard the knowledge store.** Unreviewed shared memory poisons all four agents and every future session — one hallucinated gotcha will be believed indefinitely. Entries are proposed, queued, and accepted by the lead or user. Periodic consolidation merges duplicates and prunes stale entries.

Flash has a 1M window, so never wiping is *possible*. Don't confuse capacity with usefulness — attention degrades well before the window fills, and a stale failed approach sitting in context actively drags the model back toward it.

---

## 7. Worker profiles

A profile is a **thin diff over the orchestrator's config**, not a replacement load-out. Default a worker to exactly what the project's `.claude/` gives everyone, and diverge only where you've observed a reason to.

```
~/.fleetor/<key>/profiles/backend.json
  skills       add / remove relative to project default
  plugins      add / remove
  mcp          fleet shim always; others opt-in
  tools        allowed / disallowed
  permission   mode + auto-approve rules
  charter      --append-system-prompt (slot identity only)
  knowledge    which knowledge paths inject at SessionStart
  model        deepseek-v4-flash | deepseek-v4-pro
```

Build cost is near zero — these are just different `--settings` / `--mcp-config` / `--append-system-prompt` values at spawn. The user edits them by talking to the orchestrator or by editing the file.

**Sell profiles on behaviour, not tokens.** Skill descriptions are a line each; the savings are modest. The real point is that a worker with thirty skills available *will reach for them*, and that's where erratic behaviour comes from. Narrower action space, more predictable agent — a mechanical constraint rather than an instructional one.

**Model is a profile field.** Most tickets run Flash; a genuinely hard one gets a profile pinned to V4 Pro. Heterogeneous workers with no new architecture.

**Slash commands become worker prompt templates.** A headless worker can't type `/review`, but the fleet server can expand a slash command file into the message it writes to stdin. Review, gate, retro, and spec-review prompts all become user-editable markdown — behaviour tuning with no code change.

**Load-out is fixed at session start.** A worker discovering it needs a missing skill must `ask_lead`, and granting means respawn plus re-injecting ticket and handoff note. Default profiles slightly generous and prune from observed usage — over-pruning is worse than over-granting, because a worker missing a tool won't say so, it'll improvise something worse.

---

## 8. Process discipline

The practices that earn their keep, and specifically why for agents:

| Practice | Rationale |
|---|---|
| **Acceptance criteria before work starts** | The single biggest lever. Vague ticket → improvisation → erratic output. Flash improvises more than Pro, so this matters more than it would have. |
| **Interface-first** | Lead defines contracts and signatures; workers fill in behind them. Load-bearing with Flash — it's much better at filling in behind a defined contract than inventing one. |
| **Declared file ownership per ticket** | Two agents in one file is the top concurrency failure. Declared at assignment, enforced by hook. |
| **Peer review by a different instance** | Fresh eyes catch what self-review structurally cannot. At Flash prices, run two reviewers on anything risky. |
| **Machine-checkable exit gate** | Tests/typecheck/lint/build pass *before* a report reaches the lead. Failures auto-bounce, capped at ~3 retries then escalate — cheap tokens make grinding tempting and it still wastes wall-clock. |
| **Trunk-based, short-lived branches** | Long-lived agent branches produce unresolvable merge hell. Rebase before every handoff. |
| **WIP limit of 1** | Context stays on one problem. |
| **"Blocked" as a first-class outcome** | Must be cheaper to raise a hand than to guess. If escalation feels like failure, agents invent requirements instead. |
| **Spec review before dispatch** | One worker checks the breakdown. Catches a bad plan at its cheapest moment. |
| **Retro → knowledge** | What makes the fleet improve rather than repeat last week's mistake. |

**Deliberately skipped:** story points, sprint ceremony, elaborate branching models, sign-off chains. They manage human coordination costs that don't apply.

**Role model:** fluid roles with stable home areas. Role is a property of the ticket assignment, not the agent identity — keeps utilization high, and shared knowledge replaces most of what fixed specialization would buy.

---

## 9. Guardrails

Constrain interfaces and verification. Leave method free.

**Mechanical — costs zero context, zero attention:**

- PreToolUse path guard: writes only inside the worker's worktree and declared files
- No force-push, no direct trunk writes, no writes into `~/.fleetor/` internals
- Per-ticket wall-clock and token budget; server kills and escalates on breach
- Exit gate must pass before a report is accepted
- One branch per ticket, squashable, trivially revertable
- Profile narrows the available action space

**Instructional — keep to three:**

1. Out-of-scope discovery goes in the backlog, not in your diff.
2. If the ticket is wrong or ambiguous, say so. That's a success.
3. Report in the structured schema.

**Not in the prompt:** coding style essays (linters carry that), long always/never lists, defensive boilerplate. Every rule is attention spent, and rule-dense prompts produce timid literal agents — the opposite of what's wanted.

**Why this contains erratic behaviour without a rulebook:** variance is dangerous in proportion to blast radius and detection latency. Small tickets, crisp AC, an automated gate, a peer reviewer, and one-command revert mean a strange idea is caught in minutes and costs nothing.

---

## 10. Desktop app

### Shell

**Tauri 2, Rust backend.** Small, lightweight, local. The UI layer is thin enough that webview terminal fidelity is the one real platform risk — de-risked by a dedicated pty spike before the shell phase (§12).

- **Rust core** = fleet server. Owns `state.db`, the socket, every child process, and the orchestrator's pty (`portable-pty`).
- **Webview** = UI. Terminal pane (xterm.js fed by pty output events), worker strip, ticket board, tabs.
- Workers reach the fleet server through a tiny Rust shim binary bridging stdio MCP ↔ unix socket.
- macOS-first; socket code isolated in one module so named pipes can slot in later if Windows ever matters.

### Layout

```
┌─ repo · branch · gate · session cost ──────────────────────────────────┐
├──────────┬─────────────────────────────────────────────────────────────┤
│ SIDEBAR  │ DASHBOARD BAND — always visible                             │
│          │  orchestrator · worker-1..4 · queue (backlog/active/rev/done)│
│ Workspace│  each cell: status dot + label · activity · elapsed · ctx   │
│ Tickets  ├────────────────────────────────────────┬────────────────────┤
│ Event log│ ORCHESTRATOR — real claude TUI in pty  │  TICKET BOARD      │
│ Diffs    │  (chat, plan mode, skills, slash       │   in review        │
│ Knowledge│   commands — unmodified)               │   in progress      │
│ ────────  │                                        │   backlog          │
│ Profiles │                                        │   done             │
│ Gate     │                                        │                    │
│ Models   │                                        │                    │
│ Settings │                                        │                    │
└──────────┴────────────────────────────────────────┴────────────────────┘
```

- **Sidebar** is the app's navigation spine: workspace views up top (Tickets, Event log incl. DMs, Diffs, Knowledge queue), a CONFIGURE section below (Profiles, Gate, Models & keys, Settings). Deliberately roomy — future panels (retro, metrics, multi-repo) slot in without redesign. Unread counts as small mono badges.
- **Dashboard band** sits above the workspace and is *always visible* regardless of the selected view: one cell for the orchestrator (state, model, context, unread), one per worker slot (status dot + label, current ticket + activity, elapsed, context), one for the queue (backlog / active / review / done counts). Clicking a cell jumps to the relevant view. The band replaces the earlier separate worker strip.

### Visual language

Approved from the mockup — carry it forward:

- Flat surfaces, no gradients or shadows. Three-step surface ramp for depth.
- Monospace for anything the system owns: slot names, ticket IDs, tool calls, paths, metrics. Sans for prose.
- Warm palette throughout — **no blue anywhere**. Coral-orange accent for attention (active state, tool-call markers, blocked-question rail); dark gold for awareness (review/pending states, unread badges, cost). Everything else neutral warm gray. Exact tokens in the scaffolding guide §7.
- **Status is never colour alone** — dot plus a text label, always.
- Pill chips for ambient status in the top bar. Dense but quiet: small type, generous line-height, hairline borders.
- The blocked question gets a left accent rail inline in the terminal stream. It's the one thing that should catch the eye.

### End-to-end user flow

**First run.** Point at an existing local repo. The app scans — CLAUDE.md, package.json scripts, Makefile, CI — and proposes the exit gate for confirmation; detects trunk; checks the tree is clean. Creates `~/.fleetor/<repo-key>/` and four worktrees. User supplies the DeepSeek base URL and key. Everything else defaults; profiles start identical to the orchestrator.

**Priming.** One `claude` does a repo orientation pass; capture its `session_id` as the warm fork base.

**Session.**

1. User types the goal into the terminal pane. Plan mode if it's large. The orchestrator explores the repo like any Claude Code session.
2. Orchestrator produces contracts and tickets with AC and file ownership. Board fills in live.
3. One worker spec-reviews the breakdown. Lead revises. User edits tickets on the board or argues with the lead in chat.
4. On approval, the app spawns a worker process per ticket in its worktree.
5. User watches the board and worker strip. They don't read worker output; it's a tab away.
6. **The core loop:** a worker hits ambiguity → `ask_lead` → surfaces inline in the terminal pane with an accent rail → user answers in chat → `reply()` → worker unblocks with context intact. OS notification if the app isn't focused.
7. On completion the gate runs. Failures bounce silently and never reach the user. Passes go to a peer reviewer.
8. Structured report lands in the terminal pane; diff viewer tab shows the actual change.
9. User approves merge to the integration branch — **never trunk**. They open the real PR by hand.
10. Worker writes its handoff note and proposes knowledge entries → review queue tab → accept or reject.
11. Process killed, slot idle, next ticket dispatched.

**Close and resume.** Closing SIGTERMs the workers; state persists, worktrees stay. Reopening restores the board exactly; in-flight tickets resume from their handoff note or return to backlog.

---

## 11. Failure modes

| Failure | Mitigation |
|---|---|
| Two workers edit one file | Declared ownership + PreToolUse enforcement |
| Worker loops on a failing test | Token/time budget → kill → escalate with transcript tail |
| Declares done, gate fails | Auto-bounce, capped retries, then escalate |
| Permission prompt with no human | Permission mode set at spawn; auto-approve hook |
| Turn ends with no report | Server writes back demanding one |
| Bad plan cascades to four agents | Spec review before dispatch |
| Zombie claims | Leases + heartbeats; expired claims return to backlog |
| Knowledge poisoning | Reviewed merges + periodic consolidation |
| Opus context blown by chatter | Structured reports only; raw logs on disk |
| DM storms | Rate-limit `broadcast`; DMs carry facts that change someone's work, not status narration |
| App crash mid-flight | State in SQLite; worktrees survive; resume from handoff notes |

---

## 12. Suggested build order

| Phase | Deliverable | Proves |
|---|---|---|
| 0 | One worker driven by hand through the DeepSeek endpoint | Flash's tool-call fidelity inside real Claude Code — the whole thing rests on this |
| 0.5 | pty spike: real `claude` TUI through portable-pty + xterm.js in a bare Tauri window | Resize, colors, alternate screen, mouse — the one Tauri-specific risk, retired early |
| 1 | Fleet server, spawn/assign/report, CLI only | The stdin/stdout loop and turn-end detection |
| 2 | `ask_lead`, `dm`, Stop-hook delivery, `await_events` | The messaging model |
| 3 | Exit gate, auto-bounce, peer review | Quality loop closes without the lead |
| 4 | Tauri shell — terminal pane, board, worker strip | The product |
| 5 | Profiles, knowledge store, retro/consolidation | Improvement over time |

Phases 0 and 0.5 are non-negotiable and should be boring — if Flash misbehaves through Claude Code's tool surface, or the TUI degrades through the pty pipeline, everything downstream changes.

---

## 13. Open questions for Fable

1. **Does `--fork-session` from a warm base actually save anything through the DeepSeek endpoint?** Large cost implication. Needs measurement, not assumption.
2. **Tool-call fidelity across Claude Code's full tool surface under Flash.** V4's XML tool format is claimed to remove JSON escaping failures, but CC's surface is wide. Which tools misbehave, and how often?
3. **`await_events` vs Stop hook** as the primary orchestrator wake mechanism — both, and how do they interact when mail arrives during a long-poll?
4. **Worktree granularity** — persistent per slot (warm build caches, but state leaks between tickets) vs ephemeral per ticket (clean, but cold builds)?
5. **Ticket sizing heuristic** — what target keeps the 60% context checkpoint from firing? Probably needs calibrating against Flash specifically.
6. **Is four the right number** now that workers are ~10× cheaper? Utilization data from phase 3 should answer this.
7. **Gate command discovery** — how far can auto-detection go before it needs to ask?
8. **Interrupt semantics** — what happens to a worker's in-flight tool call when the lead interrupts, and can the session recover cleanly or must it respawn?

---

## Sources

- [DeepSeek V4 Flash — OpenRouter](https://openrouter.ai/deepseek/deepseek-v4-flash)
- [DeepSeek V4 Flash: 284B MoE, 1M Context, Benchmarks, Pricing](https://www.morphllm.com/deepseek-v4-flash)
- [DeepSeek V4: V4-Pro and V4-Flash — Complete Guide](https://deepseek.ai/deepseek-v4)
- [DeepSeek V4 Preview Release — DeepSeek API Docs](https://api-docs.deepseek.com/news/news260424/)
- [Use Claude Code with Non-Anthropic Models — LiteLLM](https://docs.litellm.ai/docs/tutorials/claude_non_anthropic_models)
