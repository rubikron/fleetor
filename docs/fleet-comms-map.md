# How a message travels — as-built map

The one document to read before changing anything in the delivery path. It follows a single `fleet send` from the model that typed it to the terminal it lands in, and names what can go wrong at each step.

*Written against the post-pivot system (D-030). The previous version of this file mapped six channels, a sync/async supervisor split, an MCP shim, a mail queue and a ticket board — it was the input to the pivot, and all of that is deleted. This documents what the code **does**.*

---

## 1. The path, end to end

```mermaid
graph TB
    subgraph pane["worker-1 — a real claude TUI in a pty"]
        AGENT["the model, in its Bash tool<br/>fleet send orch 'parser is green'"]
    end

    subgraph cli["fleet — crates/fleetor-cli"]
        HELLO["FLEETOR_PANE=worker-1 → Hello { pane }<br/>FLEET_SOCKET → ~/.fleetor/_shell/fleet.sock<br/>Op::Send { to: orch, text }"]
    end

    subgraph app["Tauri app process (src-tauri)"]
        HUB["Hub — fleetor-server::hub<br/>Message::direct(..).framed()<br/>AppCommand::Deliver { to, text, ack }"]
        DELIVER["deliver.rs<br/>orch's serial writer, draining"]
        REG["PaneRegistry — pty.rs<br/>ESC[200~ body ESC[201~ · 30ms · CR<br/>one write, one lock hold"]
        STORE["BroadcastStore → EventFollower"]
        UI["React shell<br/>Terminals · Messages · Topology · Activity"]
    end

    ORCH["orch's pty — the operator's own claude"]

    AGENT --> HELLO
    HELLO -->|"unix socket, NDJSON"| HUB
    HUB --> DELIVER
    DELIVER --> REG
    REG -->|"bytes"| ORCH
    REG -.->|"DeliveryResult { accepted }"| HUB
    HUB -->|"FleetEvent::Message — after the ack"| STORE
    STORE -->|"fleet://event"| UI
    ORCH -->|"pty://output/orch"| UI
```

Two things about that diagram are load-bearing.

**The ack comes back before the log is written.** `Hub::deliver` asks the app, waits, and *then* appends. That ordering is the entire reason the app seam is request/response rather than fire-and-forget: the feed records the real outcome, never an intention. Reverse it and the Messages view starts claiming deliveries that did not happen.

**There is no timeout on that wait.** Not in the CLI, not in the hub, not in the delivery loop. A ceiling cannot cancel a delivery already sitting in the channel, so all it could do is report failure for a message that then arrives — the model resends, and the log lies. Waiting forever is the honest failure (D-034).

---

## 2. `fleet` — the agent surface

Seven verbs: `send`, `broadcast`, `reply`, `cmd`, `task`, `roster`, `whoami`. It is a Bash command, not an MCP server, and that is deliberate — a model that can run `ls` can run `fleet send`, and it reads its own exit code and stderr, so a refusal is self-correcting in a way a tool result is not.

Three of them carry a message. **`cmd` reaches a terminal without being one** (§3a), and **`task` never reaches a terminal at all** (§3b).

Identity comes from `FLEETOR_PANE`, set by the spawn path. There is no anonymous connection: `Hello` carries a `PaneId`, not an `Option<PaneId>`, so a message from nobody cannot be constructed. `fleet reply` routes on who last got through, which is only meaningful because of that.

Exit codes are a contract both briefs state verbatim: **non-zero means it did not arrive.** Zero means the bytes reached a live terminal, and explicitly not that anyone read them.

The wire tag *is* the verb — `Op::Send` serializes as `"send"` — so there is no second spelling to keep in sync. A test pins the clap subcommand list to `brief::VERBS`, because a rename landing in only one place teaches every pane a command that exits 2.

---

## 3. The hub — routing, and nothing else

It decides where a message goes and writes down what happened. It holds nothing. There is no queue, no mail table, no turn boundary, no ask/reply waiter, no long-poll — all of that existed to compensate for headless workers having no writable stdin between turns, and a live TUI is always writable. That single fact is what deleted roughly seven thousand lines.

`broadcast` fans out to the **app's** roster, not to a configured slot list: a fan-out should reach the panes that exist, not the panes someone once declared. It reports `accepted` only if *every* leg landed — the brief taught the model that zero means the bytes arrived, so a partial fan-out reading as a full one means the panes that missed it are never followed up.

A broadcast leg deliberately does **not** become the recipient's reply target. It was not addressed to them, the brief tells them not to answer it, and letting it overwrite `last_inbound_from` would silently redirect their next `fleet reply` to a pane that never spoke to them. That is the one failure where a message arrives somewhere it was never meant to go.

---

## 3a. `fleet cmd` — the one arm that is not a message (D-045)

`fleet cmd <pane|self> "/compact keep the parser" --why "…"` runs a slash command in a pane's terminal. It travels the same socket, the same hub, the same per-pane serial writer and the same `write_paste`. **Four things differ, and each of them is something a message must never do:**

| | a message | a command |
|---|---|---|
| framing | `[fleet · orch] …` | **none** — the `/` must be the first character in the input box |
| target | never itself (`hub.rs` self-send guard) | **may be itself** — `fleet cmd self` is the ordinary use |
| accept-time check | none; the body is whatever was typed | **allowlisted** against `command::ALLOWED_COMMANDS` |
| batching | concurrent messages join into one paste (D-039) | **always a write of its own** |

The last two rows are the same fact from two directions. `docs/command-channel-notes.md` measured what happens when a command is *not* alone at column 0: the input box concatenates, and `another unfinished thought/clear` is submitted as prose. So the writer splits a drained batch into runs — consecutive messages join exactly as they did before, a command never joins anything.

**The refusal is at accept time only.** `Command::new` checks the allowlist, the leading `/`, the absence of control characters and a non-empty `--why`, and answers with a sentence for the sender's stderr. Nothing is written and nothing is logged — the same shape as the self-send refusal, which is the class D-034 keeps. After acceptance nothing may delay, drop or alter it: there is no queue, no retry and no deadline on this arm either.

**This is deliberately not "the router chops the prefix off for slash commands."** That design — inspecting message bodies for a leading `/` — has been argued and lost twice (Tier 1.4). Commands are not messages, and nothing is chopped: `Message::framed`, `sanitize`, `write_paste` and `Op::Send`/`Broadcast`/`Reply` are byte-identical to what they were before this arm existed.

`accepted` means what it always means, and here it is weaker than usual: the bytes reached a live pty. A command submitted mid-turn is queued by Claude Code and runs when the turn ends; a command that landed after unsubmitted text never runs at all. Neither is observable from this side, so nothing renders `accepted` as "executed".

---

## 3b. `fleet task` — the arm that never leaves the log (WP-05, D-047)

`fleet task post|update|list` maintains the shared board: the decomposition the fleet agreed on, one block at a time, each with an outcome, technical criteria, a semantic link back to the vision, an owner and optional tree links.

**It is the only op that does not reach the app.** `Hub::task` is not even `async`: it appends to the store, or folds the store back into a board, and stops. Everything else on this map ends at a pty; this ends at the log.

```
fleet task ──▶ hub ──▶ Store::append_event ──▶ (bus) ──▶ Tasks view
                 └──▶ Store::events_since(0) ──▶ task::board ──▶ stdout
```

That absence is the whole design. **The board is a diary, not a dispatcher:**

- posting a block assigns nobody — assignment is an ordinary `fleet send`, and the brief says so in those words;
- nothing schedules, routes, orders or refuses anything by task state;
- `done` is a claim its author made, not a verdict — WP-06's review is what verifies one;
- any status may follow any other, and anyone may update any block (the event's `from` is the accountability);
- cycles and dangling links in the tree are tolerated and rendered as written, never validated into a graph.

The old system's `Assign` op returned `Ack` while nothing ever ran, and `wire.rs` still carries the warning that `Assign` and `FleetStatus` "are the seed of the ticket system growing back". So the claim is checked rather than asserted: `crates/fleetor-server/tests/task_board.rs` pins that ten posted blocks send the app **zero** commands, and that a `fleet send` to a worker is byte-identical at the pty and in the log whether the board is empty or holds ten blocks including that worker's own, marked `done`.

There is no `tasks` table. The board is `task::board(events_since(0))` — the same replay the message feed does — so a restarted hub computes the same board and the UI's Tasks view mirrors the fold in `ui/src/fleet/board.ts`.

---

## 3c. `operator` — the addressee with no terminal (WP-07, D-051)

The human is a `PaneId` like any other name (`"operator"`), and exactly one thing is true of them that is not true of a pane: **there is no pty.** Every asymmetry falls out of that one fact via `PaneId::has_pty()`, not out of a flag.

```
worker-2 ──▶ fleet send operator ──▶ hub ──▶ Store::append_event ──▶ (bus) ──▶ inbox
                                       └──▶ "recorded msg-…"  (exit 0)

operator ──▶ composer ──▶ Hub::handle ──▶ (the path in §1, unmodified) ──▶ pty
                                       └──▶ "accepted msg-…"
```

**Inbound is the only new arm on the message path.** `Hub::deliver` opens with `if !msg.to.has_pty() { return self.record(msg) }`; everything below it — framing, ack, ordering, reply target, log entry — is what it was for pane↔pane traffic. `Hub::record` is not `async` and never touches `self.app`, the same signature-level tell `Hub::task` carries.

**Outbound is not a new arm at all.** The UI's composer calls `Hub::handle(PaneId::Operator, op)` in-process — the identical function `serve_conn` calls after reading a `Hello`, with only the socket missing — so the operator's `send`/`broadcast`/`reply` are the fleet's own three verbs rather than a fourth thing.

Three words now exist and they never blur (Tier 1.5):

| word | means | who says it |
|---|---|---|
| `accepted` | the bytes reached a live pty — **not** that the agent read them (L3) | any send to a pane |
| `recorded` | it entered the log; no pty exists to have taken it | a message to `operator`, and a `fleet task` claim |
| `delivered` | — | nothing. It is never rendered |

`recorded` is **derived, never stored**: the event carries `accepted: false, detail: None`, which is the literal truth of that field when there is no pty, and every renderer computes the word from the addressee. One store failure asymmetry comes with it — a failed append *fails* the op here, because for the human the log **is** the delivery.

What the operator is not: spawnable (`spawn_pane` refuses by name), killable, a `fleet cmd` target (refused at accept time — a slash command needs an input box), or a broadcast leg. `Hub::roster` puts them at the top of the *listing* with the state `present`; `Hub::app_roster`, which is what a fan-out targets, does not know they exist. A leg for a pty that is not there would make every `fleet broadcast` report a partial failure.

---

## 4. The registry — five ptys

One pty per pane, keyed by `PaneId`. Three properties matter.

**Per-pane event channels** (`pty://output/orch`, `pty://output/2`). Not one channel with an id in the payload. `AppHandle::emit` wakes *every* listener registered on a name, so a shared channel means all five panes' callbacks fire for every chunk from every pane and four of them discard it. Per-id names make cross-talk impossible rather than merely unlikely, and remove 5× the JS wakeups as a side effect.

**Coalesced reads.** `read()` returns as soon as any bytes are available, so a TUI's 200-byte spinner frame would otherwise make a complete trip across the IPC bridge — the event rate tracks the TUI's *repaint* rate, not its data rate. The pump merges ~16 ms or 64 KB before emitting. It is two threads, not one: a single thread could only check its window *after* the next blocking read returned, so the tail of a burst would sit unflushed until the pane happened to print again.

**One write, one lock.** The paste-open, body, paste-close and `\r` reach the pty under a single hold of that pane's writer mutex. Tauri dispatches sync commands on a thread pool, so an operator keystroke racing a delivery could otherwise land between a message's body and its submit — corrupting it or submitting it early.

---

## 5. The writer — one per pane, draining

Each pane has a single task that owns its terminal. When free it takes **everything** currently queued for that pane (`try_recv`, never `recv`) and sends it as one bracketed paste, each message still framed with its sender, separated by a blank line.

This exists because of something observed on a live fleet: several workers messaging the orchestrator at once, some getting lost. Two causes.

*Ours:* every `Deliver` used to get its own `spawn_blocking` task, and those raced for the writer lock in whatever order the blocking pool scheduled them. Nothing was dropped — the mutex and the kernel buffer see to that — but a pane could see messages in an order the log disagreed with.

*Not ours:* a `claude` input box is one text field. Message A submits and starts a turn; B's paste lands mid-turn and CC queues it; C lands on the same buffer. What the model reads is a blend.

The serial drain fixes the first outright and takes the second away from the TUI, doing the merge ourselves where the framing still says who sent what. It is **not** the batching this design bans: zero delay for a message arriving at a free pane, and it groups only what was already concurrent — the set the TUI was going to merge anyway. Every message keeps its own log row, its own `accepted` and its own ack. The batch groups the write, never the accounting (D-039).

---

## 6. What each agent actually receives

Two things, and nothing else.

**At spawn, a briefing** via `--system-prompt` (`fleetor-core::brief`) — never a `CLAUDE.md` in the pane's cwd, which would show up in `git status` and could be deleted by the worker itself. It names the peers, teaches the five verbs, states the exit-code contract verbatim, and shows the framing the pane will actually see. The worker brief additionally carries the L5 clause: *never reply to a broadcast unless it names you.*

**At runtime, framed messages** typed into its input:

```
[fleet · orch] take the parser, I have the CLI
[fleet · worker-1 → all] rebasing onto master
```

The two framings differ on purpose. A worker can only obey the do-not-answer-a-broadcast rule if it can see which kind arrived.

`message::sanitize` is a security boundary, not tidying. Delivery wraps the body in a bracketed paste, which makes it *data* only for as long as it contains no escape of its own — a body carrying `ESC[201~` would end the paste early and every byte after it would arrive as live keystrokes at a `claude` prompt, where `ESC` cancels and `/` opens a menu. CRLF and lone CR become newlines, tabs and newlines survive, every other control character is dropped. Dropped rather than rejected: a mangled message that lands beats a clean one that doesn't. The log keeps the raw body — sanitizing is a property of the pty boundary, not of the message.

---

## 7. What can go wrong, and how it shows

| Failure | How it presents | Where it is handled |
|---|---|---|
| Pane never spawned | `fleet send 2` exits 1, `worker-2 is not running` | `PaneRegistry::writable` |
| Pane has exited | exits 1, `worker-2 has exited` | same. The predicate is `accepts_input()`, i.e. **not dead** — never `is_live()`, because nothing tells us when `claude` reaches its prompt, and refusing on a guess means a healthy pane silently drops everything |
| App window closed | exits 1, `the fleet app is not accepting commands` | `Hub::ask_app` |
| App took it and never answered | exits 1, `treat it as not delivered` | same. A dropped ack would otherwise park the caller forever |
| **Pane is mid-turn** | **exits 0, `accepted`, and the model may never act on it** | **nothing on this side of the pty can see it.** The residual, and the reason `accepted` is the weaker claim (L3) |
| `fleet` not on PATH | panes spawn fine and cannot message; `Notice` on the Activity view at spawn | `spawn::fleet_bin_path` |
| Pane wedged on onboarding | looks exactly like a working pane; every send reports success | `spawn::seed_config_dir`, called at the spawn site so it is keyed by the cwd actually used |
| Body contains `ESC[201~` | would end the paste early and turn its tail into live keystrokes | `message::sanitize` |
| Same, in a `fleet cmd` | exits 1, `refused rather than repaired` | `command::check_command`. **Refused, not stripped** — a mangled message is still the message, a mangled command is a *different command* |
| **`fleet cmd` lands on a non-empty input box** | **exits 0, `accepted`, and the command runs as prose instead** | **nothing on this side can see it.** Measured in `docs/command-channel-notes.md` §3; the second residual, and the second reason `accepted` never means "executed" |
| Worktree creation failed | `Warn` on Activity; that worker shares the target checkout | `fleet::worker_cwd` |
| Message to `operator`, log write failed | exits 1, `nothing was recorded` | `Hub::record`. The one place a store error *fails* a send — for the human the log is the delivery, not a record of one |
| **Operator never reads their inbox** | **exits 0, `recorded`, and nobody answers** | **nothing on this side can see it** — the third residual, and the reason `recorded` promises the log and not a person |

---

## 8. Reading the code

Follow it in this order; it is roughly the order the bytes travel.

1. `crates/fleetor-core/src/pane.rs` — identity, why `accepts_input()` is not `is_live()`, and the one name with no pty behind it
2. `crates/fleetor-core/src/message.rs` — the record, the framing, `sanitize`
3. `crates/fleetor-core/src/command.rs` — the other kind of thing a pane can send, and why it is not a message
4. `crates/fleetor-core/src/brief.rs` — what each pane is told
5. `crates/fleetor-cli/src/main.rs` — the six verbs and the exit-code contract
6. `crates/fleetor-server/src/hub.rs` — routing, and persist-after-ack
7. `src-tauri/src/deliver.rs` — the serial drain, and the run split that keeps a command alone
8. `src-tauri/src/pty.rs` — the registry, the channels, the read pump
9. `src-tauri/src/spawn.rs` — how a pane is launched

The two test files worth reading as documentation: `crates/fleetor-server/tests/pane_messaging.rs` (every routing decision, against a fake app) and `src-tauri/tests/panes.rs` (five real ptys, no tokens).
