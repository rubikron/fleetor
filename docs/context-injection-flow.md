# When each piece of context is injected

The companion to [`prompts/README.md`](../prompts/README.md), which says *what* the files are. This one says **when each one arrives, and where it lands inside the pane's actual context window.**

The thing to hold onto: a pane is a real interactive `claude`, so everything here has to land in one of the four places `claude` accepts context — its system prompt, its memory files, its environment, or a user turn. There is no fifth door.

---

## 1. The timeline

Four moments. Only the last one repeats.

```mermaid
sequenceDiagram
    autonumber
    participant OP as Operator
    participant APP as FLEETOR app
    participant FS as prompts/ + ~/.fleetor/prompts/
    participant PANE as the pane's claude

    rect rgba(120,140,255,0.10)
    note over APP,FS: T0 — bootstrap, once per app launch
    APP->>FS: PaneContext::resolve()
    FS-->>APP: orch.md · worker.md · fragments · launch.conf
    APP->>OP: Activity notice — "using your ~/.fleetor/prompts/worker.md"<br/>or "ignoring it — {delivery_contract} is missing"
    end

    rect rgba(120,200,150,0.10)
    note over OP,APP: T1 — spawn, once per pane
    OP->>APP: start the fleet
    APP->>APP: render_worker(template, me, roster, cwd)<br/>fragments composed in at their placeholders
    APP->>APP: build argv + env from launch.conf
    end

    rect rgba(255,180,120,0.12)
    note over APP,PANE: T2 — exec, once per pane
    APP->>PANE: claude --permission-mode auto --system-prompt "<brief>"<br/>cwd = worktree · CLAUDE_CONFIG_DIR · ANTHROPIC_MODEL · FLEETOR_PANE
    PANE->>PANE: assembles its own context window (§2)
    end

    rect rgba(255,140,180,0.12)
    note over APP,PANE: T3 — runtime, unbounded
    loop every fleet send / broadcast / reply
        APP->>PANE: ESC[200~ [fleet · orch] take the parser ESC[201~ CR
        PANE->>PANE: arrives as a user turn
    end
    end
```

**Everything before T3 is frozen at `exec`.** There is no way to change a running pane's system prompt, so an edited `worker.md` reaches a pane on its *next* launch. That is the same rule `fleet_pick_target` follows for the target repo, and for the same reason.

---

## 2. Where it lands in the pane's context window

This is the part that matters when you are writing a brief: your text is not the only thing in there, and it does not arrive first.

```mermaid
graph TB
    subgraph win["the pane's context window, in assembly order"]
        direction TB
        S1["<b>1 · what CC still supplies</b><br/>“You are Claude Code…” + git status<br/><i>its 6.5 KB of guidance is gone (D-043)</i>"]
        S2["<b>2 · our brief</b> ← --system-prompt<br/>orch.md / worker.md, rendered<br/>+ delivery-contract.md<br/>+ broadcast-rule.md<br/>+ scaffolding.md<br/>+ vision-tenets.md (orch)"]
        S3["<b>3 · tool definitions</b><br/>Bash · Read · Edit · …<br/><i>`fleet` is a Bash command, not a tool</i>"]
        S4["<b>4 · CC's environment block</b><br/>cwd · platform · shell · model<br/><i>gone under --system-prompt —<br/>cwd is restated in our brief</i>"]
        S5["<b>5 · memory files</b><br/>project CLAUDE.md — from the cwd<br/>user CLAUDE.md, skills, agents, MCP —<br/>from CLAUDE_CONFIG_DIR"]
        S6["<b>6 · the conversation</b><br/>operator keystrokes, and every<br/>[fleet · …] message as a user turn"]
        S1 --> S2 --> S3 --> S4 --> S5 --> S6
    end
    T2A["T2 · exec"] -.->|"argv"| S2
    T2B["T2 · exec"] -.->|"cwd + CLAUDE_CONFIG_DIR"| S5
    T2C["T2 · exec"] -.->|"cwd"| S2
    T3["T3 · runtime"] -.->|"bracketed paste"| S6
```

Three consequences worth designing around:

**Our brief is a *replacement*, but a narrower one than that sounds (D-043).** `--system-prompt` empties Claude Code's guidance out of the system prompt — 6,866 chars down to 364 in the measured run — and leaves everything else exactly where it was. Tools, memory files, skills, agents and the git-status section are byte-identical under either flag; `docs/notes/system-prompt-notes.md` has the diff. Two consequences for anyone writing a brief: the line "You are Claude Code, Anthropic's official CLI for Claude." is its own system block and still arrives, so a brief saying "you are not Claude Code" contradicts the text above it; and the working posture that *did* leave has to come from `scaffolding.md`, which is why that fragment is composed into both briefs and cannot be dropped. A brief still cannot beat `--permission-mode` — that is a flag, not prose.

**Layer 5 stopped being where orch and worker diverge (D-062).** Until WP-14 the orchestrator ran on the operator's own `CLAUDE_CONFIG_DIR` and got their user `CLAUDE.md`, skills, subagents and MCP servers. It now runs on a fleet-owned config dir holding the same two onboarding keys a worker's does — the price of having its transcript archived with the run — so **neither class** gets any of that. Both still get the repo's own committed `CLAUDE.md`, because both have a checkout as their cwd. So a pane that needs to know something must be told it in its brief or in a file committed to the repo. There is no third place, for either of them now.

**Messages arrive as user turns, mid-conversation** — not as system context, and not at a turn boundary. That is why `broadcast-rule.md` exists: a user turn is exactly the thing a helpful model answers.

---

## 3. Every piece, and who owns it

| What | File / source | Lands in | When | Editable without a rebuild |
|---|---|---|---|---|
| Orchestrator brief | `prompts/orch.md` | system prompt (replace) | T2, once | ✅ `~/.fleetor/prompts/orch.md` |
| Worker brief (all 4 slots) | `prompts/worker.md` | system prompt (replace) | T2, once | ✅ `~/.fleetor/prompts/worker.md` |
| Exit-code contract | `prompts/delivery-contract.md` | inside both briefs | T2, once | ✅ — but the placeholder is required |
| Anti-amplification clause | `prompts/broadcast-rule.md` | inside the worker brief | T2, once | ✅ — but the placeholder is required |
| Working posture CC no longer supplies | `prompts/scaffolding.md` | inside both briefs | T2, once | ✅ — but the placeholder is required |
| The vision tenets | `prompts/vision-tenets.md` | inside the orchestrator brief | T2, once | ✅ — but the placeholder is required |
| The pane's working directory | the cwd `spawn.rs` sets | `{cwd}` in both briefs | T2, once | ❌ — CC's `# Environment` section used to carry it |
| Peer roster | computed — `PaneId::roster` | `{peers}` / `{workers}` | T2, once | ❌ `WORKER_SLOTS` |
| Worker model | `prompts/launch.conf` | `ANTHROPIC_MODEL` | T2, once | ✅ |
| Worker endpoint | `prompts/launch.conf` | `ANTHROPIC_BASE_URL` | T2, once | ✅ |
| Permission mode | `prompts/launch.conf` | `--permission-mode` | T2, once | ✅ |
| Worker credential | `DEEPSEEK_API_KEY` env or nearest `.env` | `ANTHROPIC_AUTH_TOKEN` | T2, once | ✅ (never in a file here) |
| Project memory | the target repo's own `CLAUDE.md` | memory files | T2, once | ✅ — it is just a repo file |
| Operator memory, skills, agents, MCP | `CLAUDE_CONFIG_DIR` | memory files | T2, once | **none, for either class** (D-062) |
| `orch`'s credential | the operator's Keychain entry | `CLAUDE_SECURESTORAGE_CONFIG_DIR=""` | T2, once | ❌ — unset it and orch is silently logged out |
| Pane identity | `FLEETOR_PANE` | env, read by `fleet` | T2, once | ❌ by design |
| Fleet messages | `fleetor-core::message` framing | a user turn | **T3, every send** | ❌ single-sourced |
| The three wedge-forever removals | `src-tauri/src/spawn.rs` | env | T2, once | ❌ documented in `launch.conf` |

---

## 4. The one runtime path, in detail

T3 is the only thing that repeats, and it is the only context a pane receives that it did not start with.

```mermaid
graph LR
    A["worker-1's model<br/>fleet send orch 'parser is green'"] --> B["fleet CLI<br/>FLEETOR_PANE → identity"]
    B -->|"unix socket"| C["Hub<br/>routes, does not queue"]
    C --> D["Message::framed()<br/>[fleet · worker-1] parser is green"]
    D --> E["sanitize<br/>strip ESC, CR → LF"]
    E --> F["serial writer<br/>drains all queued for that pane"]
    F -->|"ESC[200~ … ESC[201~ CR"| G["orch's pty<br/>arrives as a user turn"]
    C -.->|"after the ack"| H["event log → Activity view"]
```

The framing is single-sourced in `fleetor-core::message` and cross-checked by a test in `brief.rs`, so the briefs can never describe a framing the delivery path no longer produces. That is why `[fleet · worker-3]` appears verbatim in `worker.md` — it is pinned, not decorative. If you rewrite the "What arrives" section, keep an example of both shapes in it.

For everything about how that message travels and what can go wrong on the way, see [`fleet-comms-map.md`](./fleet-comms-map.md).

---

## 5. Changing a brief, end to end

1. `cp prompts/worker.md ~/.fleetor/prompts/` and edit it. Keep paragraphs on one line, keep every placeholder.
2. Restart the fleet. Watch the Activity feed: an `Info` naming your file means it loaded; a `Warn` names the placeholder you dropped and tells you the built-in is running instead.
3. `cargo test -p fleetor-core` if you changed the shipped file rather than an override — the tests pin the verb list, both fragments and the framing.

To make a change permanent, edit `prompts/*.md` in the repo and commit it. The override directory is for iterating; the repo is for deciding.
