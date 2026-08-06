# FLEETOR

*(working name)*

A local desktop app that runs five Claude Code terminals against a git repo you already have, and lets them talk to each other. One is your own Opus, as orchestrator. Four are DeepSeek V4 Flash workers, each in its own git worktree. All five are **real, interactive `claude` TUIs** — chat, plan mode, skills and slash commands all work, because each one genuinely is `claude`.

They coordinate through one command:

```
fleet send 2 "take the parser, I have the CLI"
fleet broadcast "rebasing onto master"
fleet reply "on it"
fleet cmd self "/compact keep the parser design" --why "task block done; the rest is stale"
fleet roster
```

That's the whole agent-facing surface. A message is typed straight into the target terminal — you watch it land.

`fleet cmd` is the one verb that is not a message: it runs `/clear` or `/compact` — and nothing else — in a pane's own terminal, so a worker can prune its own context between tasks. `--why` is required, because the record of *why* the fleet cleared or compacted is the point of the verb (D-045).

Tauri 2 · Rust · React · macOS-first. Plugs in on top of your repo: `rm -rf ~/.fleetor` leaves it untouched.

## How it works

```
  ┌──────────────┬──────────────┐
  │              │  [1][2][3][4]│      fleet send 2 "…"
  │ orchestrator │              │             │
  │  your Opus   │  worker-2    │             ▼
  │              │  Flash       │      unix socket ──▶ Hub
  │              │              │                       │
  └──────────────┴──────────────┘      AppCommand::Deliver
         ▲                                              │
         └────── pty://output/orch ◀── PaneRegistry ◀────┘
                                            │
                              bracketed paste + CR into the pty
```

Five ptys, one per pane. The `fleet` CLI runs inside a pane's Bash tool and dials a unix socket. The hub decides where the message goes and asks the app to type it. The app writes it into the target pty as a bracketed paste, then answers with whether the bytes were queued. Only *then* does the hub write the message to the log — so the feed records what actually happened, never an intention.

**`accepted`, never `delivered`.** A zero exit means the bytes reached a live terminal. It is not a claim that the model there read them, and nothing in the UI renders it as one.

**Nothing may delay, refuse, reorder, drop or throttle a message.** There is no timeout anywhere between `fleet send` and the pty, no queue, no idle guard, no rate limiter. A ceiling could only ever report failure for a message that then arrives — which makes the model resend and makes the log lie. See D-034.

## Running it

Needs `claude` on your PATH and a `DEEPSEEK_API_KEY` in a `.env` at the repo root (workers only — the orchestrator uses your own login).

```bash
npm install
npm run tauri dev
```

Then **Start fleet**. Five processes spawn: your Opus in the target repo, four Flash workers in per-slot worktrees under `~/.fleetor/_shell/worktrees/`. Ask the orchestrator to run `fleet send 2 "say hello back with fleet reply"` and watch worker-2's tab.

Without a configured target it works in a seeded testbed at `~/.fleetor/testbed` — a small Python project with a passing test suite, embedded in the binary so it can't fail to materialize. Point it at your own repo with **Choose folder…** on the start gate, or by setting `target` in `~/.fleetor/config.json`.

### A run that costs nothing

```bash
FLEETOR_PANE_CMD=$PWD/tests/fake-pane/fake-pane.sh npm run tauri dev
```

Every pane runs a five-line echo script instead of `claude`. The registry, socket, hub, delivery path and UI are all real; only the agents are not. Drive it from another terminal:

```bash
export FLEET_SOCKET=~/.fleetor/_shell/fleet.sock FLEETOR_PANE=orch
./target/debug/fleet send 2 "hello worker two"
```

## Layout

```
crates/
  fleetor-core/     contracts: PaneId, Message, FleetEvent, the wire, the briefs
  fleetor-db/       SQLite behind the Store seam — one table, `events`
  fleetor-ipc/      the Transport seam: unix socket, newline-delimited JSON
  fleetor-server/   the hub (routing) and the bus (live event push)
  fleetor-cli/      the `fleet` binary
src-tauri/
  pty.rs            PaneRegistry: N ptys, per-pane channels, coalesced reads
  deliver.rs        one serial writer per pane; the AppCommand receiver
  spawn.rs          how a pane's claude is launched, and what it is told
  fleet.rs          store, bus, hub, target resolution, pane spawning
ui/                 Vite + React + xterm.js
tests/fake-pane/    a pane that is not claude, for zero-token tests
```

State lives in `~/.fleetor/`: `config.json` (your target), `testbed/` (the fallback project), `_shell/` (the socket, the event DB, worker config dirs and worktrees).

## Status

**The pivot is complete and merged.** FLEETOR previously ran one TUI orchestrator plus four *headless* `claude -p` workers under a sync supervisor, reached through an MCP shim, wrapped in a ticket board with gates and peer review. All of it is gone — see `docs/tui-pivot-plan.md` for why, and D-030 through D-039 in `decisions.md` for each decision.

Verified: 55 workspace tests and 34 shell tests, seven of them driving five **real ptys**; `tsc --noEmit && vite build` clean. And the thing no test can prove — five live `claude` TUIs messaging each other through the socket — has been run by hand.

Known gaps, in rough priority order:

- **Broadcast amplification (L5) is mitigated by prompt text, not by a limiter.** Five peers that all answer broadcasts is a token fire. The worker brief says *never reply to a broadcast unless it names you*; there is deliberately no hub-side token bucket, because that is the one mitigation that would sit in the delivery path and refuse sends. It returns only if a real fountain is observed, sized against that measurement.
- **A pane mid-turn still queues messages inside Claude Code**, where we have no visibility. The serial per-pane writer (D-039) fixed the part that was ours; this residual is why `accepted` is the weaker claim.
- Briefings have not been adversarially tested against a live Flash worker.
- The coalescing window (~16 ms / 64 KB) is a reasoned guess, not a measurement. No chunks/sec instrument yet.
- `kill_all`'s process-group teardown has not been checked with `ps` after a window close.

## Reading order

1. **`docs/fleet-comms-map.md`** — how a message actually gets from `fleet send` to a terminal. Start here.
2. **`docs/tui-spawn-notes.md`** — what was measured against real Claude Code 2.1.220 about spawning an interactive pane. Several of these findings are load-bearing and non-obvious.
3. **`building.md`** — decision tiers, the seams, the risk register.
4. **`decisions.md`** — the running log of where defaults lost, and why.
5. **`docs/tui-pivot-plan.md`** — the plan this branch executed, with each phase's deviations recorded.

Documents under `docs/` marked **superseded** describe the pre-pivot system. They are kept as the record behind decisions that are still in force, not as a description of the code.

## Non-negotiables

Real unmodified Claude Code processes only — the harness lives around `claude`, never inside it. The orchestrator is the operator's genuine interactive TUI. Your repo is never written to except code on feature branches. Nothing may sit between `fleet send` and the target pty that can delay, refuse, reorder, drop or silently alter a message.
