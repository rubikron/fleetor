# FLEETOR — Scaffolding & Build Guide

**Audience:** Fable (detail mapping) and the Claude Code instance(s) that will build this.
**Companion:** `docs/handoff.md` — the *what and why*. This document is the *how to start without cornering yourself*. It lives at the repo root as `BUILDING.md`.

The guiding instruction from max: **give the builder a good framework, not handcuffs.** When the code fights an assumption in these docs, the assumption is allowed to lose — through the decision-tier process below, not by silently hacking around it.

---

## 1. Decision tiers

Every choice in both documents belongs to one of three tiers. When in doubt about which tier something is in, it's Tier 2.

### Tier 1 — Invariants. Ask max before violating.

1. **The repo-boundary test.** `rm -rf ~/.fleetor/<repo-key> && git worktree prune` leaves the user's repo untouched, minus kept feature branches. FLEETOR writes only application code, only on feature branches. Never CLAUDE.md, never trunk.
2. **Every agent is a real, unmodified Claude Code process.** The harness lives around CC — pty, stdin/stdout, hooks, MCP. Never fork or patch CC itself.
3. **The orchestrator is the real interactive TUI.** Chat, plan mode, skills, slash commands work because it genuinely is `claude`.
4. **Session lifetime = ticket lifetime.** Respawn is the context wipe.
5. **Blocking calls point worker→lead only.** No blocking worker↔worker primitive, ever — it deadlocks a 4-agent fleet.
6. **Merges land on an integration branch.** The user opens the real PR to trunk by hand.
7. **Shared knowledge merges only after review.** Unreviewed memory poisons all four agents.
8. **Auto-approve never exceeds the worker's worktree + declared files.** Widening this is a security decision — max's, not the builder's.

### Tier 2 — Strong defaults. Change freely, but log it in `DECISIONS.md`.

Worker count (4) · SQLite via rusqlite · report schema fields · gate retry cap (3) · profile file format · ticket/mail table shapes · the 60% context checkpoint · fluid roles with home areas · React for the UI · exact MCP tool names and signatures · directory layout under `~/.fleetor/` · macOS-first.

A `DECISIONS.md` entry is three lines: what changed, why the default lost, what would reverse it. Cheap enough that there is no excuse to skip it, substantial enough that the reasoning survives.

### Tier 3 — Free. No log needed.

Internal module structure, crate names, error-handling style, frontend state management, test organization, CSS architecture — anything invisible at the seams.

---

## 2. Stack

| Layer | Choice | Notes |
|---|---|---|
| Shell | **Tauri 2** | Small, local, Rust-native. |
| Async runtime | tokio | Process supervision, socket, channels. |
| Orchestrator pty | `portable-pty` | The wezterm crate. Spike it in Phase 0.5 before trusting it. |
| Worker processes | `tokio::process` + NDJSON framing | Plain pipes; no pty needed for headless workers. |
| State | `rusqlite`, bundled, WAL mode | Single writer task; everything else goes through a channel. |
| IPC | Unix domain socket in `~/.fleetor/<key>/` | Isolate in one module (Tier 2: macOS-first; named pipes can slot in later). |
| Worker MCP shim | Small Rust bin, stdio ↔ socket | One binary; slot identity via env var. |
| UI | Vite + React + TS, xterm.js | Tier 2. UI is thin; swapping frameworks later is cheap. React chosen because agents build it most reliably. |
| Frontend↔backend | Tauri events for streams, `invoke` for actions | pty chunks and worker activity are events; user actions are commands. |

---

## 3. Workspace scaffold

```
fleetor/
  BUILDING.md               ← this file
  DECISIONS.md              ← append-only; seeded with the Tier-2 list
  docs/handoff.md           ← the architecture handoff
  src-tauri/                ← THIN. Window, command registration, event wiring.
  crates/
    fleetor-core/             ← domain: tickets, envelopes, state machine. No I/O.
    fleetor-db/               ← rusqlite + migrations (from day one, even for table 1)
    fleetor-cc/               ← Claude Code adapter: spawn, NDJSON parse, pty, hooks config
    fleetor-server/           ← routing, supervision, gate runner, socket
    fleetor-shim/             ← bin: stdio MCP ↔ socket bridge
    fleetor-cli/              ← bin: headless driver for Phases 0–3
  ui/                       ← Vite app
  tests/
    fake-claude/            ← scripted stand-in for the real CLI (see §5)
    fixtures/ndjson/        ← golden transcripts captured from the real CLI
  examples/                 ← throwaway spikes; never imported by crates/
```

Two structural rules that keep the builder out of corners:

**`src-tauri` stays thin.** All logic lives in crates that compile and test headless. This is what makes Phases 0–3 buildable and provable via `fleetor-cli` before any window exists — and it means a UI-layer problem can never hold the core hostage.

**Traits at exactly four seams, no more:**

```rust
trait AgentProcess   // real claude vs fake-claude vs future model
trait Transport      // unix socket now, named pipe someday
trait Store          // sqlite behind it
trait GateRunner     // shell commands now, anything later
```

These four are where known change lives (model swaps, platforms, storage, gate types). Do not add speculative traits beyond them — over-abstraction is its own corner, and it's the one agents fall into most.

---

## 4. Contracts first

Before any supervision logic, define in `fleetor-core` and freeze behind serde:

1. **The message envelope** — `{id, from, to, kind, body, ref?, ts, v}`. Versioned from day one; `v:1` costs nothing now and saves a migration later.
2. **The event stream types** — what the server emits to the UI and CLI (worker state changes, board changes, mail, gate results).
3. **The report schema** — as in handoff §4.
4. **The MCP tool schemas** — both faces, generated or hand-written but single-sourced; the shim and server must not drift apart.
5. **The DB schema sketch** — `tickets`, `leases`, `mail`, `events` (append-only log), `knowledge_proposals`. Sketch-level; migrations carry it forward.

The MCP tool surface is the *stable* contract — internals may churn freely behind it. This is deliberate: it's the same surface regardless of whether the fleet server lives in a Tauri app, a daemon, or something not yet imagined.

**Capture before coding:** run the installed `claude` CLI, dump real `system`/`assistant`/`result` NDJSON events into `tests/fixtures/`, and build the parser against those — not against the field names in the handoff, which are from memory. Same for hook payloads. `claude --version` goes in the fixture directory name; when CC updates, re-capture and diff.

---

## 5. fake-claude

A small script in `tests/fake-claude/` that speaks the stream-json protocol and plays scripted scenarios:

- happy path: init → work → `report` → `result`
- ends turn without filing a report
- hangs (simulated permission wedge) — tests the budget watchdog
- calls `ask_lead` and waits
- receives mid-turn mail via Stop-hook injection
- gate-fail → bounce → fix → pass

Every supervision test runs against fake-claude: fast, free, deterministic. Real-CC integration tests exist but run behind a flag, because they cost real tokens. This one artifact is what makes the fleet server testable at all — build it in Phase 1, extend it every time a new failure mode is discovered in the wild.

---

## 6. Build phases

Each phase has an exit test. Don't start the next phase until it passes; *do* revisit earlier phases freely.

| Phase | Deliverable | Exit test |
|---|---|---|
| **0 — Probe** | `fleetor-cli probe`: one real CC worker against the Flash endpoint, driven by hand | 10 varied tickets on a toy repo; tool-call failure rate and failure *shapes* written up. **If fidelity is bad, stop and ask max** — model choice is his. |
| **0.5 — pty spike** | Bare Tauri window running the real `claude` TUI via portable-pty + xterm.js | Resize, colors, alternate screen, scrollback, paste all behave. **If not fixable, stop and ask max** — the fallback (orchestrator lives in the user's own terminal, app becomes a companion window) changes the product. |
| **1 — Supervisor** | spawn / assign / turn-end detection / report ingestion, via `fleetor-cli` | Full ticket lifecycle against fake-claude, then against real CC once. |
| **2 — Messaging** | shim, `ask_lead`/`reply`, `dm`, Stop-hook delivery, `await_events` | Scripted 3-agent conversation with a mid-turn delivery, against fake-claude. |
| **3 — Quality loop** | Gate runner, auto-bounce with retry cap, peer-review dispatch | A deliberately buggy ticket bounces, gets fixed, passes review — no human input. |
| **4 — Shell** | Tauri app: terminal pane, worker strip, board, ask-lead rail | The §7 theme; a full session run end-to-end in the UI. |
| **5 — Memory** | Profiles, knowledge store + review queue, retro/consolidation | A knowledge entry proposed, reviewed, and visible to the next session. |

**Working agreements for the builder:**

- **Spike-then-commit.** Anything touching an unknown (pty quirks, hook behavior, DeepSeek endpoint quirks) gets a throwaway in `examples/` first. Spikes are allowed to be ugly; crates are not allowed to import them.
- **Prefer reversible implementations.** When two designs are close, take the one that's cheaper to back out of.
- **When an assumption from the docs fights back, it loses** — via a `DECISIONS.md` entry (Tier 2) or a question to max (Tier 1). Never via a workaround that preserves the letter of the doc while burying the problem.

---

## 7. UI theme

Carry the approved mockup. Tokens:

```css
:root {
  --surface-0: #1f1e1b;      /* app background */
  --surface-1: #262421;      /* cards, worker cells, tickets */
  --surface-2: #2d2b27;      /* top bar, section headers */
  --border:        #383632;
  --border-strong: #45423d;
  --text-primary:   #e8e6e1;
  --text-secondary: #b8b4ab;
  --text-muted:     #8a867c;
  --accent:      #d97757;    /* warm coral-orange — attention */
  --gold:        #c69a4e;    /* dark golden — secondary signal */
  --gold-dim:    #8a6a33;    /* gold borders, quiet badges */
  --ok:          #7fa36a;    /* warm green — gate pass, diff added */
  --err:         #c25d4f;    /* warm red — gate fail, diff removed */
  --font-sans: system-ui, -apple-system, "Segoe UI", sans-serif;
  --font-mono: ui-monospace, "SF Mono", Menlo, monospace;
}
```

**The palette is entirely warm — there is no blue anywhere in this app.** Focus rings, selection, links, info states: all places where UI kits default to blue get coral or gold instead. The two-accent split:

- **Coral (`--accent`)** = *needs or has attention*: active nav rail, working-state dots, tool-call markers, the blocked-question rail. Unchanged three-use budget.
- **Gold (`--gold`)** = *noteworthy but not urgent*: review/pending states on the board and band, unread badges, session cost, gate-running indicator, knowledge-proposal markers. Gold never blinks or demands — it labels.

If something warrants attention → coral. If it warrants awareness → gold. If neither → neutral.

**xterm palette:** tune the 16 ANSI colors warm so the orchestrator's TUI output sits on-theme — in particular remap ANSI blue/cyan to gold/tan variants (`#c69a4e` / `#a98f5f`). Verify readability against real CC output in the Phase 0.5 spike; if CC's own color semantics suffer, favor legibility over theme.

**Layout skeleton (Tier 2 in detail, Tier 1 in spirit):**

1. **Sidebar** — the navigation spine. Workspace views (Workspace, Tickets, Event log, Diffs, Knowledge) above a CONFIGURE section (Profiles, Gate, Models & keys, Settings). Built as a flat registry of panels so future options (retro, metrics, multi-repo) are an entry, not a redesign. Active item gets the accent left-rail; unread counts as small mono badges.
2. **Dashboard band** — a persistent status strip above the workspace, visible in *every* view: one cell for the orchestrator (state, model, ctx, unread), one per worker slot (dot + label, ticket + activity, elapsed, ctx), one for the queue (backlog / active / review / done). Cells are clickable shortcuts to their views. This band absorbs the earlier standalone worker strip.
3. **Workspace** — terminal pane + ticket board, as before.

Rules, in priority order:

1. Flat. No gradients, shadows, or textures. Depth comes from the three-surface ramp and hairline borders.
2. Monospace for everything the *system* owns — slot names, ticket IDs, tool calls, paths, metrics. Sans for prose.
3. One accent, spent in exactly three places: active-state dots, tool-call markers, and the blocked-question rail. If a fourth use appears, one of the existing three is probably wrong.
4. Status is never color alone — always dot + text label.
5. Dense but quiet: small type, generous line-height, pill chips for ambient status.
6. The `ask_lead` question is the only element designed to catch the eye: left accent rail, inline in the terminal stream.

---

## 8. Risk register

| Risk | Detection | Mitigation | Fallback |
|---|---|---|---|
| Flash tool-call fidelity through CC's full tool surface | Phase 0, measured | — | Pin workers to V4 Pro or any Anthropic-compatible model; model-as-profile-field means architecture is untouched. **Ask max** — cost is his call. |
| TUI degrades through portable-pty + xterm.js | Phase 0.5 | Spike before shell | Companion-window mode (orchestrator in user's own terminal). **Ask max** — changes the product. |
| Headless worker wedges on a permission prompt | Looks identical to a slow worker | Permission mode set at spawn + PreToolUse auto-approve within owned paths; budget watchdog catches the residue | Watchdog kill + escalate |
| CC hook/NDJSON API drift across versions | Fixture re-capture diff on CC update | Version-stamped fixtures; parser built from captures, not docs | Adapter layer in `fleetor-cc` absorbs it |
| Stop-hook mail injection behaves differently than assumed | Phase 2 exit test | Verify against installed CC before building on it | Fall back to next-turn-boundary delivery only |
| SQLite write contention | Busy errors under load | Single writer task; WAL; everything through one channel | — |
| Zombie processes after app crash | Startup sweep | Pidfile per child; sweep and reap on launch | `git worktree prune` + lease expiry recovers state |
| Token burn in tests | CI cost | fake-claude for everything; real-CC tests behind a flag | — |
| Over-abstraction by the building agent | PR review smell: traits nobody implements twice | The four-seam rule (§3) | Collapse the trait |

---

## 9. Escalation triggers — ask max, don't decide

The builder should interrupt and ask when, and only when:

1. Phase 0 or 0.5 fails its exit test — model choice and product shape are max's.
2. Anything would require writing into the user's repo beyond feature branches (Tier 1.1).
3. Anything would widen auto-approve beyond worktree + declared files (Tier 1.8).
4. A Tier 1 invariant genuinely conflicts with reality — bring the conflict, not a workaround.
5. Real-money surprises: endpoint pricing differs materially from the handoff's numbers, or a test strategy implies non-trivial recurring spend.
6. Cross-platform or distribution/signing questions — scope decisions, not engineering ones.

Everything else: decide, log if Tier 2, keep moving. Frequent small questions are a worse failure mode than an occasional logged wrong default — the whole design (small tickets, gates, revertability) exists so wrong defaults are cheap.
