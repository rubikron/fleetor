# FLEETOR — Scaffolding & Build Guide

**Companion:** `docs/fleet-comms-map.md` — how a message actually travels. This document is the *how to work on this without cornering yourself*.

The guiding instruction: **give the builder a good framework, not handcuffs.** When the code fights an assumption in these docs, the assumption is allowed to lose — through the decision-tier process below, not by silently hacking around it. That is not theoretical: most of what this file described a version ago is gone, because it lost that argument (D-030).

---

## 1. Decision tiers

Every choice belongs to one of three tiers. When in doubt about which tier something is in, it's Tier 2.

### Tier 1 — Invariants. Ask before violating.

1. **The repo-boundary test.** `rm -rf ~/.fleetor && git worktree prune` leaves the user's repo untouched, minus kept feature branches. FLEETOR writes only application code, only on feature branches. Never `CLAUDE.md`, never trunk.
2. **Every agent is a real, unmodified Claude Code process.** The harness lives around CC — pty, environment, system prompt. Never fork or patch CC itself.
3. **Every pane is a real interactive TUI.** Not just the orchestrator. Chat, plan mode, skills and slash commands work because each pane genuinely is `claude`. This is what removed the supervisor, the mail queue, the turn-boundary machinery and the Stop hook in one go — a live TUI is always writable.
4. **Nothing may sit between `fleet send` and the target pty that can delay, refuse, reorder, drop or silently alter a message.** No queue, no idle guard, no retry, no backoff, no rate limiter, no deadline — including on the CLI's own call. A ceiling cannot cancel a delivery already in flight, so all it can do is report failure for a message that then arrives, which makes the model resend and makes the log lie. Waiting forever is the honest failure (D-034).
5. **`accepted` never renders as "delivered."** The bytes reached a live pty. Nothing on this side of it knows whether the model read them. A delivery model that quietly lies is the worst failure mode this product has (L3). Nor may `accepted` be stretched to cover a message that never had a pty to reach — that is `recorded`, and it is a different word on purpose (D-051).
6. **The log records outcomes, not intentions.** Persist after the ack, never before.
7. **Auto-approve never exceeds the worker's worktree.** Widening this is a security decision, not a builder's.
8. **Shared knowledge merges only after review.** Unreviewed memory poisons every pane. (Not built yet; the invariant predates the feature.)

### Tier 2 — Strong defaults. Change freely, but log it in `decisions.md`.

Worker count (4) · SQLite via rusqlite · the `fleet` verb list · the `fleet cmd` allowlist (`/clear`, `/compact`) · pane briefing text · the 30 ms submit gap · the ~16 ms / 64 KB coalescing window · React for the UI · directory layout under `~/.fleetor/` · worker model (DeepSeek V4 Flash) · macOS-first.

A `decisions.md` entry is three lines: what changed, why the default lost, what would reverse it. Cheap enough that there is no excuse to skip it, substantial enough that the reasoning survives.

### Tier 3 — Free. No log needed.

Internal module structure, crate names, error-handling style, frontend state management, test organization, CSS architecture — anything invisible at the seams.

---

## 2. Stack

| Layer | Choice | Notes |
|---|---|---|
| Shell | **Tauri 2** | Small, local, Rust-native. |
| Async runtime | tokio | Socket, hub, delivery tasks. |
| Panes | `portable-pty` | The wezterm crate. One pty per pane, all five identical. |
| State | `rusqlite`, bundled, WAL mode | One connection behind a mutex — exactly one writer. One table: `events`. |
| IPC | Unix domain socket at `~/.fleetor/_shell/fleet.sock` | Isolated in `fleetor-ipc`; a named-pipe impl can slot in behind `Transport`. |
| Agent surface | The **`fleet` CLI**, run through Bash | Deliberately not MCP. A model that can run `ls` can run `fleet send`, and its exit code and stderr are self-correcting in a way a tool result is not. |
| UI | Vite + React + TS, xterm.js | Tier 2. UI is thin; swapping frameworks later is cheap. |
| Frontend↔backend | Tauri events for streams, `invoke` for actions | pty chunks are events on **per-pane** channels; user actions are commands. |

---

## 3. Workspace scaffold

```
fleetor/
  CLAUDE.md                 ← session orientation + the documentation system
  building.md               ← this file
  decisions.md              ← append-only
  docs/                     ← indexed by docs/README.md; fleet-comms-map.md is the companion map
  prompts/                  ← every word a pane is told (D-042); prompts/README.md is the account
  src-tauri/                ← THIN. Window, ptys, delivery, wiring.
  crates/
    fleetor-core/           ← contracts: PaneId, Message, FleetEvent, wire, briefs. No I/O.
    fleetor-db/             ← rusqlite + migrations (appended, never edited)
    fleetor-ipc/            ← the Transport seam
    fleetor-server/         ← the hub (routing) and the bus (live event push)
    fleetor-cli/            ← bin: the `fleet` binary
  ui/                       ← Vite app
  tests/fake-pane/          ← a pane that is not claude (see §5)
  examples/                 ← throwaway spikes; never imported by crates/
```

Two structural rules that keep the builder out of corners:

**`src-tauri` stays thin — but it is no longer trivial.** It owns the ptys, and the ptys are the product. What stays out of it is *routing*: the hub decides where a message goes, `src-tauri` only types it. Keep that split. The registry is deliberately behind an `Emit` callback rather than an `AppHandle` so `src-tauri/tests/panes.rs` can drive five real ptys with no window — a registry only exercisable through a GUI is a registry nothing tests.

**Traits at exactly two seams, no more:**

```rust
trait Transport      // unix socket now, named pipe someday
trait Store          // sqlite behind it
```

There were four. `AgentProcess` (real vs fake `claude`) and `GateRunner` (shell commands vs anything later) both died with the headless fleet — the first because there is no process to abstract when the agent is a terminal, the second because there is no gate. Do not add speculative traits beyond these two; over-abstraction is its own corner, and it is the one agents fall into most.

The seam that replaced them is not a trait but a channel: `AppCommand`, request/response between the hub and whatever owns the terminals. Request/response rather than fire-and-forget specifically because `fleet send 3` needs a real yes/no, and only the pty registry knows whether pane 3 is alive.

---

## 4. Contracts first

The first four are defined in `fleetor-core` and frozen behind serde. The fifth is a contract of a different kind — prose, frozen by tests rather than by a wire format:

1. **`PaneId`** — serializes as a bare string (`"orch"`, `"worker-2"`), so the CLI argument, the DB payload, the event field and the TypeScript type are all the same thing with no second spelling to keep in sync. **`"operator"` is the one name with no pty behind it** (WP-07, D-051): the human is addressable, never spawnable, and `PaneId::has_pty()` is where every consequence of that is derived rather than flagged.
2. **`Message`** — the record, **body included**, plus the exact framing typed into the receiving TUI. Single-sourced, so the one delivery path can never invent a second spelling. Its `sanitize` is a security boundary: a body containing `\x1b[201~` would otherwise end the bracketed paste that carries it and turn its own tail into live keystrokes at a `claude` prompt. **`Command` is its deliberate sibling, not a variant of it** (D-045): a `fleet cmd` is delivered *unframed* so its `/` reaches column 0, may target the sender, and is refused at accept time against an allowlist — three things a message must never do. Keeping them apart is what stops the "chop the prefix off inside the router" design Tier 1.4 has rejected twice; `docs/notes/command-channel-notes.md` is the measurement behind it.
3. **`FleetEvent`** — five variants. What the backend appends and the UI replays. The frontend renders an unknown `type` as *nothing*, so `ui/src/fleet/types.ts` moves in the same commit as a new variant or the event is invisible rather than broken. **`Task` (WP-05) is the one to read before adding a sixth**: it is the nearest neighbour of the `TicketMoved` Phase 5 deleted, and the only reason it is not that is that nothing reads it back to permit, order or refuse anything. `crates/fleetor-core/src/task.rs` carries the tripwire list; `crates/fleetor-server/tests/task_board.rs` checks the claim instead of asserting it.
4. **The wire** (`Op` / `OpResult` / `Hello`) — six ops, and the wire tag *is* the CLI verb. `Task` is the only *op* that never reaches the app: a board that reached a terminal would be a dispatcher. On the result side there are **three outcome words and they never blur** (Tier 1.5): `accepted` (bytes reached a live pty), `recorded` (entered the log; no pty exists — a task claim, or a message to `operator`), and `delivered`, which nothing ever renders. `OpResult::Recorded` is deliberately *one* variant for both callers, because they are one event class differing only in what the id names (D-051).
5. **The briefs** — what each pane is told about the fleet at spawn, via `--system-prompt`. Never a `CLAUDE.md` in the pane's cwd: that would show up in `git status` and the worker could delete it. The flag *replaces* Claude Code's own system prompt rather than appending to it (D-043), so `prompts/` is the whole of what a pane is told; `docs/notes/system-prompt-notes.md` is the measurement that made that safe.

**The brief prose is `prompts/*.md`, not a Rust literal (D-042).** `fleetor-core::brief` bakes those files in with `include_str!` — so the binary always has a working brief and the tests still pin it — and renders `{me}` `{peers}` `{workers}` into them. The operator's own copies in `~/.fleetor/prompts/` are read once at bootstrap and override the built-ins, announced on the Activity feed: Info when one loads, Warn naming the fix when one is refused, silence when there is none. `prompts/launch.conf` does the same for a worker's model, endpoint and permission mode.

Two fragments — `delivery-contract.md` and `broadcast-rule.md` — are *composed into* the briefs at a placeholder rather than written out in them, and a template that dropped its placeholder is refused rather than rendered. They are the two clauses the fleet cannot run without: the first is the only reason a model can tell a failed send from a good one, the second is the only mitigation left for broadcast amplification after the rate limiter was removed (D-031, and Tier 1.4 forbids putting it back). Everything around them is the operator's to rewrite. `prompts/README.md` is the full account.

The `fleet` CLI surface is the *stable* contract — internals may churn freely behind it. A test pins its clap subcommands to `brief::VERBS`, because a rename that lands in only one place teaches every pane in the fleet a command that exits 2.

**Measure before coding.** Anything touching an unknown gets a throwaway in `examples/` first, and the measurement goes in `docs/`. `docs/notes/tui-spawn-notes.md` is the model: it bisected exactly which config keys an interactive `claude` needs before it will reach a prompt, and three of its findings were load-bearing surprises. Where a spike and a plan disagree, the spike wins.

---

## 5. fake-pane

`tests/fake-pane/fake-pane.sh` — five lines. It announces itself, then echoes every submitted line back with a marker. Selected by setting `FLEETOR_PANE_CMD`, which the spawn path honors in place of `claude`.

That is enough to make the whole delivery path testable for free: `src-tauri/tests/panes.rs` runs five **real ptys** and proves channel isolation both ways, a message landing in its target and nowhere else, keystrokes and deliveries sharing a writer without interleaving, a dead pane refusing by name, a killed pane respawning, `kill_all` reaping the fleet, and a four-way burst arriving whole, in order, in fewer writes than there were messages.

What a fake pane deliberately **cannot** prove is whether a live `claude` reaches its prompt. That is L1/L2, and it was settled by measurement against the real binary — a fake would answer it wrong in the reassuring direction. Extend the script when a new failure mode is found in the wild; do not extend it into pretending to be Claude Code.

---

## 6. Where things stand

**The live roadmap is `docs/roadmap/00-index.md`** — the Blackboard work packages (WP-01..09, all landed except the shakedown's live half) and whatever gets filed after them from `docs/roadmap/TEMPLATE.md`. The pivot that preceded them is a closed chapter: its plan is `docs/archive/tui-pivot-plan.md`, Phases 0–6 with each phase's exit test and recorded deviations. `docs/README.md` indexes everything else.

**Working agreements:**

- **Spike-then-commit.** Spikes are allowed to be ugly; crates are not allowed to import them.
- **Prefer reversible implementations.** When two designs are close, take the one that's cheaper to back out of.
- **When an assumption from the docs fights back, it loses** — via a `decisions.md` entry (Tier 2) or a question (Tier 1). Never via a workaround that preserves the letter of the doc while burying the problem.
- **Delete rather than deprecate.** Dead code that implements a pattern the design now bans is a trap, not a leftover: the obvious next move is always to re-point it at something live. Phase 2 deleted the injection queue and its producer together for exactly this reason.

---

## 7. UI theme

Tokens:

```css
:root {
  --surface-0: #1f1e1b;      /* app background */
  --surface-1: #262421;      /* cards, pane cells */
  --surface-2: #2d2b27;      /* top bar, section headers */
  --border:        #383632;
  --border-strong: #45423d;
  --text-primary:   #e8e6e1;
  --text-secondary: #b8b4ab;
  --text-muted:     #8a867c;
  --accent:      #d97757;    /* warm coral-orange — attention */
  --gold:        #c69a4e;    /* dark golden — secondary signal */
  --gold-dim:    #8a6a33;    /* gold borders, quiet badges */
  --ok:          #7fa36a;    /* warm green */
  --err:         #c25d4f;    /* warm red */
  --font-sans: system-ui, -apple-system, "Segoe UI", sans-serif;
  --font-mono: ui-monospace, "SF Mono", Menlo, monospace;
}
```

**The palette is entirely warm — there is no blue anywhere in this app.** Focus rings, selection, links, info states: all places where UI kits default to blue get coral or gold instead.

- **Coral (`--accent`)** = *needs or has attention*: active nav rail, live pane dots, message rails.
- **Gold (`--gold`)** = *noteworthy but not urgent*: broadcast tags, undelivered markers, unread badges. Gold never blinks or demands — it labels.

**xterm palette:** the 16 ANSI colors are tuned warm so a pane's TUI output sits on-theme, remapping ANSI blue/cyan to gold/tan (`#c69a4e` / `#a98f5f`). If CC's own color semantics suffer, favor legibility over theme.

**Layout skeleton:**

1. **Sidebar** — a flat registry of views (Terminals, Messages, Topology, Activity) above a CONFIGURE section. Future options are an entry, not a redesign.
2. **Dashboard band** — one cell per pane, orchestrator anchored wider, visible in every view. Cells are clickable shortcuts to their pane.
3. **Workspace** — the terminal grid, or the message record, or the topology, or the activity log.

Rules, in priority order:

1. Flat. No gradients, shadows, or textures. Depth comes from the three-surface ramp and hairline borders.
2. Monospace for everything the *system* owns — pane names, message bodies, paths. Sans for prose.
3. Status is never color alone — always dot + text label.
4. Dense but quiet: small type, generous line-height, pill chips for ambient status.
5. **Every terminal stays mounted.** There is no screen-replay in the backend: unmount an xterm and its buffer is destroyed, and the pane comes back blank. Hidden panes use `.is-hidden`, never conditional rendering. This is a correctness rule wearing a layout rule's clothes (L7).

---

## 8. Risk register

| Risk | Detection | Mitigation | Fallback |
|---|---|---|---|
| **Broadcast amplification** — five peers that all answer broadcasts burn the budget talking to each other | Watch the Messages view during a live broadcast | Brief clause: *never reply to a broadcast unless it names you*. Deliberately **not** a limiter — that is the one mitigation that would refuse a send | A hub-side token bucket, sized against the observed fountain, not a guess |
| A pane wedges on onboarding or a trust dialog while every send reports success | Silent: it looks exactly like a working pane | `seed_config_dir` at the spawn site, keyed by absolute cwd; `ANTHROPIC_API_KEY` unset | Per-tab restart; re-run the seed |
| A pane mid-turn queues messages inside CC, where we have no visibility | The message is `accepted` in the log and the model never acts on it | One serial writer per pane, draining what is queued into a single paste (D-039) | None available from this side of the pty — hence `accepted` |
| Injected bytes become menu navigation at a `/` prompt | A pane doing something nobody asked for | Bracketed paste; `sanitize` strips every escape from the body; workers spawn `--permission-mode auto` so prompts don't appear | Per-tab restart |
| The `fleet` binary is not where the app looks | Panes spawn fine and cannot message | Explicit resolution ladder with an existence check at every rung, and a loud `Notice` when empty | `FLEETOR_FLEET_BIN` |
| Tauri event firehose from five repainting TUIs | Visible stutter; a chunks/sec instrument would settle it | Per-pane channels + ~16 ms / 64 KB coalescing | `addon-canvas` on the two visible panes only. **Not** `addon-webgl` — context limits and context-loss-on-hide in WKWebView are worse than what they fix |
| CC's flags or config keys drift across versions | A pane that no longer reaches its prompt | `docs/notes/tui-spawn-notes.md` is version-stamped; re-measure on a CC update | Adapter in `spawn.rs` absorbs it |
| Leaked `claude` processes after window close | `ps` after quitting | `kill_all` SIGTERMs each pane's **process group**, then SIGKILLs; `RunEvent::Exit` backstop for Cmd+Q (D-041) | Startup sweep in `orphans.rs` — reaps leftover pids on next launch (D-041) |
| SQLite write contention | Busy errors under load | One connection behind a mutex; WAL | — |
| Token burn in tests | CI cost | `fake-pane` for everything; the one live gate is operator-triggered | — |
| Over-abstraction | Traits nobody implements twice | The two-seam rule (§3) | Collapse the trait |

---

## 9. Escalation triggers — ask, don't decide

Interrupt and ask when, and only when:

1. Anything would require writing into the user's repo beyond feature branches (Tier 1.1).
2. Anything would widen auto-approve beyond a worker's worktree (Tier 1.7).
3. Anything would put a delay, a refusal, a retry or a ceiling between `fleet send` and a pty (Tier 1.4). This one has been argued and lost twice; bring a measurement, not an intuition.
4. A Tier 1 invariant genuinely conflicts with reality — bring the conflict, not a workaround.
5. Real-money surprises: endpoint pricing differs materially, or a test strategy implies non-trivial recurring spend.
6. Cross-platform or distribution/signing questions — scope decisions, not engineering ones.

Everything else: decide, log if Tier 2, keep moving. Frequent small questions are a worse failure mode than an occasional logged wrong default — the whole design exists so wrong defaults are cheap.
