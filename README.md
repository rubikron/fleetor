# FLEETOR

*(working name)*

A local desktop app that runs five Claude Code terminals against a git repo you already have, and lets them talk to each other. One is your own Opus, as orchestrator. Four are DeepSeek V4 Flash workers, each in its own git worktree. All five are **real, interactive `claude` TUIs** — chat, plan mode, skills and slash commands all work, because each one genuinely is `claude`.

They coordinate through one command:

```
fleet send 2 "take the parser, I have the CLI"
fleet broadcast "rebasing onto master"
fleet reply "on it"
fleet cmd self "/compact keep the parser design" --why "task block done; the rest is stale"
fleet task post --to 2 --outcome "the parser accepts nested groups" --crit-t "cargo test -p parser" --crit-s "one grammar, end to end"
fleet done task-3 "cargo test -p parser"
fleet roster
fleet whoami
```

That's the whole agent-facing surface — eight verbs. A message is typed straight into the target terminal — you watch it land.

`fleet cmd` is the one verb that is not a message: it runs `/clear` or `/compact` — and nothing else — in a pane's own terminal, so a worker can prune its own context between tasks. `--why` is required, because the record of *why* the fleet cleared or compacted is the point of the verb (D-045).

`fleet task` is the shared board: the decomposition the fleet agreed on, with each block's outcome and its performance criteria, visible to every pane and to you. It is **a diary, not a dispatcher** — posting a block assigns nobody, nothing is scheduled from it, and a `done` on it is a claim its author made. Assignment is still an ordinary `fleet send` (D-047).

`fleet done` is the receipt: it runs the block's check in the worker's own worktree and sends the orchestrator the evidence — exit code, branch @ commit, output tail — as an ordinary message. A failing check is still a receipt; the judgement belongs to the peer who reviews the named commit, and reviewed work merges to `fleet/integration`, never your trunk (D-048..D-050).

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
  fleetor-cli/      the `fleet` binary; done.rs runs a receipt's check locally
src-tauri/
  pty.rs            PaneRegistry: N ptys, per-pane channels, coalesced reads
  deliver.rs        one serial writer per pane; the AppCommand receiver
  spawn.rs          how a pane's claude is launched, and what it is told
  fleet.rs          store, bus, hub, target resolution, pane spawning
  prompts.rs        loads ~/.fleetor/prompts/ overrides at bootstrap
  context_gauge.rs  per-worker context estimates from its own transcript
  orphans.rs        startup sweep for panes a crash left behind
  testbed.rs        materializes the embedded fallback project
prompts/            every word a pane is told, as files (D-042) — see prompts/README.md
ui/                 Vite + React + xterm.js
tests/fake-pane/    a pane that is not claude, for zero-token tests
docs/               indexed by docs/README.md
```

State lives in `~/.fleetor/`: `config.json` (your target), `testbed/` (the fallback project), `_shell/` (the socket, the event DB, worker config dirs and worktrees).

## Status

**The pivot is complete and merged, and the Blackboard packages landed on top of it.** FLEETOR previously ran one TUI orchestrator plus four *headless* `claude -p` workers under a sync supervisor, reached through an MCP shim, wrapped in a ticket board with gates and peer review. All of it is gone — see `docs/archive/tui-pivot-plan.md` for why, and D-030 through D-039 in `decisions.md` for each decision. What came after — prompts as files, the command channel, the context gauge, task blocks, receipts and review, the operator as participant, the Fence — is WP-01..09 in `docs/roadmap/00-index.md`, with D-042 through D-056 behind them.

Verified: 187 workspace tests and 93 shell tests, eleven of them driving five **real ptys**; `tsc --noEmit && vite build` clean. And the thing no test can prove — five live `claude` TUIs messaging each other through the socket — has been run by hand.

Known gaps, in rough priority order:

- **Broadcast amplification (L5) is mitigated by prompt text, not by a limiter.** Five peers that all answer broadcasts is a token fire. The worker brief says *never reply to a broadcast unless it names you*; there is deliberately no hub-side token bucket, because that is the one mitigation that would sit in the delivery path and refuse sends. It returns only if a real fountain is observed, sized against that measurement.
- **A pane mid-turn still queues messages inside Claude Code**, where we have no visibility. The serial per-pane writer (D-039) fixed the part that was ours; this residual is why `accepted` is the weaker claim.
- The live shakedown (WP-09's second half) is under way but unfinished — one finding closed (D-055); the full loop (vision → task blocks → `/compact` with a logged why → receipt → review → merge) has not yet run end to end. `docs/notes/blackboard-shakedown.md` is the log.
- The coalescing window (~16 ms / 64 KB) is a reasoned guess, not a measurement. No chunks/sec instrument yet.
- `kill_all`'s process-group teardown has not been checked with `ps` after a window close.

## Reading order

1. **`CLAUDE.md`** — orientation, the commands, the conventions, and the documentation system. Auto-loaded if you are a Claude Code session; read it anyway if you are not.
2. **`building.md`** — decision tiers, the seams, the risk register. The invariants in §1 come before everything else.
3. **`docs/fleet-comms-map.md`** — how a message actually gets from `fleet send` to a terminal; §8 is the walk through the code.
4. **`docs/README.md`** — the index of everything else under `docs/`: the as-built maps, the measurement notes, the roadmap, the archive.
5. **`docs/roadmap/00-index.md`** — the live roadmap: what landed, what's pending, how new work gets filed.
6. **`decisions.md`** — the running log of where defaults lost, and why. Navigate it with `grep "^## D-"`.

Documents under `docs/archive/` describe superseded systems or moments that have passed. They are kept as the record behind decisions that are still in force, not as a description of the code.

## Non-negotiables

Real unmodified Claude Code processes only — the harness lives around `claude`, never inside it. The orchestrator is the operator's genuine interactive TUI. Your repo is never written to except code on feature branches. Nothing may sit between `fleet send` and the target pty that can delay, refuse, reorder, drop or silently alter a message.
