# FLEETOR — the 5-TUI messaging pivot

> **Status:** Phase 0 complete (`0c84b89`, branch `feat/tui-fleet`). Phases 1–6 not started.
> **Read `docs/tui-spawn-notes.md` first** — it carries the measured results of the Phase 0
> spike, several of which corrected assumptions in this plan. Where they differ, the notes win;
> the corrections have been folded into the phases below and are marked **✓ Phase 0**.
> The decision record is `DECISIONS.md` D-030.

## Context

FLEETOR today runs one orchestrator as a real `claude` TUI in a pty, and four **headless**
`claude -p --input-format stream-json` workers driven by a sync supervisor, which reach an async
hub over a unix socket through an **MCP shim** (a stdio JSON-RPC server plus a Stop hook). Around
that sits a large ticket/gate/review apparatus.

Two things came out of mapping it (`docs/fleet-comms-map.md`):

1. **Messaging works; everything else doesn't.** The shipped app runs `run_pool_fleet`, where
   `RunnerCommand::Assign(_) => {}`. A lead that calls `assign` gets an `Ack`, the ticket lands on
   the board as `Backlog`, and nothing runs it — ever. The board is a read-only surface with no
   live writer. Gate, review, and report are unreachable from the app.
2. **The headless model is what forces all the complexity.** Mail has to be queued and drained at
   turn boundaries (three delivery paths, a Stop hook, an atomic `take_mail`) only because a
   headless `-p` process has no writable stdin between turns. Turn detection, report ingestion,
   transcript reconstruction, and pid-interrupt all exist to compensate for not having a terminal.

The pivot: **delete MCP and the whole supervision model; make all five agents live `claude` TUIs.**
The product becomes messaging and visibility — watch and steer how the orchestrator and workers
talk to each other, with each agent's real TUI viewable in a tab. A live TUI has an always-writable
stdin, so the queue, the hook, and the supervisor all become unnecessary rather than merely unused.

**Outcome:** orchestrator TUI fixed on the left, four worker TUIs in tabs on the right, a message
feed carrying actual message bodies, and one `fleet` CLI verb set both directions.

---

## Decisions locked

| | Decision |
|---|---|
| **Orch→worker** | A `fleet` CLI invoked through Claude Code's built-in **Bash** tool (`fleet send 2 "…"`). Plain socket client reusing `fleetor_ipc::Client`. No MCP, no JSON-RPC, no shim. |
| **Worker model** | DeepSeek Flash with an isolated `CLAUDE_CONFIG_DIR`, spawned with **`--permission-mode auto`**. Orch keeps full inherit of the operator's own env/Opus. |
| **Deletion scope** | Everything non-messaging: `fleetor-shim`, `fleetor-cc`, supervisor/quality/gate/runner, report/review/gate/ticket, the board UI, the Phase-0 probe. |
| **Layout** | Orch fixed left (always visible) + worker tabs right, resizable divider. Replaces today's board panel. |
| **Delivery** | **Instant.** No idle guard, no parking, no queue. Bytes hit the target pty the moment the hub routes the message. |
| **Target repo** | **User-picked**, never defaulted. First run points at a dedicated testbed; a folder picker changes it. |
| **Branch** | New branch off `master` — `feat/tui-fleet`. |

---

## Target architecture

```
  orch claude TUI ──┐                            ┌── worker-1 claude TUI (Flash)
   (operator Opus)  │      ┌──────────┐          ├── worker-2
                    ├─pty──┤ Tauri app├───pty────┼── worker-3
   Bash: `fleet` ───┤      │ registry │          └── worker-4
        │           │      └────┬─────┘               │
        │           │           │                     └─ Bash: `fleet`
        │      pty://output/<pane>                          │
        │           │      (16ms-coalesced)                 │
        └───────── unix socket ──▶ Hub ◀───────────────────┘
                                   │  routes; no queue, no blocking
                                   ▼
                            events log (SQLite) ──▶ bus ──▶ feed / graph / band
```

Four structural changes from today:

- **No mail queue.** A live TUI is always writable, so delivery is immediate: hub → app → write
  bytes into the target pty. `save_mail`/`take_mail`, the three delivery paths, and the Stop hook
  all go. What persists is an append-only **message log** in the events table, which is what the
  feed replays.
- **No blocking.** `ask_lead`, the `oneshot` waiters, the ask timeout, `WorkerState::Blocked` and
  the whole `LeadEvent` queue die. A worker sends; the orch answers whenever.
- **Message bodies live in the event.** `FleetEvent::Mail` carries only metadata today. For a
  product about watching messages, the body must be in the event.
- **Identity is a pane, not a party.** `PaneId::{Orch, Worker(u8)}`, serialized as a bare string
  (`"orch"`, `"worker-2"`) so the CLI arg, DB payload, event field, and TS type are one thing.

New event enum — three variants, everything else deleted:

```rust
pub enum FleetEvent {
    Message { id, from: String, to: String, body: String,
              group: Option<String>,   // set when part of a broadcast fan-out
              accepted: bool,          // pane was live and bytes were queued to its pty
              detail: Option<String> },
    PaneState { pane: String, from: PaneState, to: PaneState },  // spawning|live|dead
    Notice { level: NoticeLevel, text: String },
}
```

`accepted`, not `delivered` — we queued bytes to a live pty; we cannot know the model read them.
The UI must never render it as "delivered."

`fleet` CLI surface: `send <pane> <text>` · `broadcast <text>` · `reply <text>` (routes to the last
inbound sender, tracked in a hub-side `last_inbound_from` map) · `roster` · `whoami`.
**Nonzero exit + stderr on a rejected send** is part of the contract — the model reads its own Bash
result and self-corrects. Sending to a dead pane fails loudly rather than queueing.

---

## Phases

Sequencing principle: **build the replacement first, delete last.** The old surface is deeply
entangled (`fleetor-cc` → `runner` → `src-tauri::build_factory`); deleting first means a long red
tree. Building first makes every deletion compiler-verified. The demo lands at end of Phase 3.

### Phase 0 — branch + interactive-spawn spike ✅ **DONE** (`0c84b89`)

All four questions answered against real CC 2.1.220; full results and bisect tables in
`docs/tui-spawn-notes.md`. Delivered: `examples/tui-spawn-spike/spike.py` (reusable pty harness),
`testbed-seed/` in-repo → materialized at `~/.fleetor/testbed/` with a baseline commit, and the
notes doc. Six findings changed the plan; each is folded into the phases below.

*Original scope, retained for reference:*

Two unknowns will otherwise make Phase 3 *look* like it works and silently not. Both are verified
real (see Landmines L1/L2). Answer them by hand in a terminal:

1. Launch `claude` **interactively** with `CLAUDE_CONFIG_DIR=$(mktemp -d)` plus the DeepSeek env
   from `fleetor-cc/src/spawn.rs::apply_env`. Record the minimum `.claude.json` seed that reaches a
   usable prompt with no onboarding — expected `hasCompletedOnboarding`, `theme`,
   `projects["<cwd>"].hasTrustDialogAccepted`, possibly `customApiKeyResponses`.
2. Confirm the byte sequence that submits a turn into a live TUI: bracketed paste
   `\x1b[200~<body>\x1b[201~`, then `\r`. Find the **minimum** gap before the `\r` that still works
   (target ~30ms) and confirm a multi-line body doesn't submit early.
3. Confirm `--permission-mode auto` on a Flash worker in the testbed never surfaces a prompt for
   Read/Write/Edit/Bash. This is the L3 wedge; `auto` is the fix, but verify rather than assume.
4. **Evaluate `tauri::ipc::Channel` for pty streaming** (see L6). If raw-byte payloads work,
   Phase 3 uses Channels and drops base64 entirely; if not, fall back to per-id event names. Cheap
   to test now, expensive to change mid-Phase 3.

Deliverable: a short `docs/tui-spawn-notes.md` with the exact seed JSON, the byte sequence and gap,
and the Channel verdict. Also create the testbed (`~/.fleetor/testbed/`) by hand so Phase 3 has
somewhere real to point at. Zero code; a few manual turns of spend.

### Phase 1 — the new contract, additive (backend only, nothing wired)

New types live *alongside* the old, so the tree stays green and every existing test passes.

**Create** `crates/fleetor-core/src/`:
- `pane.rs` — `PaneId`, `PaneState`, `Display`/`FromStr` (accepts `orch`/`2`/`w2`/`worker-2`)/serde.
- `message.rs` — `Message`, `frame_for_pane(from, body)` producing the `[fleet · worker-1] …`
  framing. Single-sourced with a test, the way `mail.rs` single-sources framing today.
- `brief.rs` — `orch_brief(roster)` / `worker_brief(me, roster)` + tests asserting each CLI verb
  literal appears, so a verb rename can't silently desync the brief from the binary.

**Modify**: `wire.rs` (add `Op::{Send, Broadcast, Reply, Roster}`, `OpResult::{Delivered, Roster}`,
`Hello { pane }`) · `event.rs` (add the two new variants + `kind()` arms) · `lib.rs` ·
`fleetor-server/src/hub.rs` (add `AppCommand`, `DeliveryResult`, `Hub::with_app`, four handler arms,
`last_inbound_from`, and a per-pane token bucket — see L5).

**The hub→pty seam**: extend the proven `dispatcher` pattern (`Hub::with_dispatcher`, hub.rs
L100-107) rather than collapsing routing into `src-tauri`. It must become **request/response**, not
fire-and-forget, because `fleet send 3` needs a real yes/no and only the pty registry knows whether
pane 3 is live:

```rust
pub enum AppCommand {
    Deliver { to: PaneId, text: String, ack: oneshot::Sender<DeliveryResult> },
    Roster  { ack: oneshot::Sender<Vec<(PaneId, PaneState)>> },
}
```

Hub awaits the ack, *then* appends the `Message` event with the real outcome —
persist-then-emit-after-ack, so the feed never lies. Keeping this in the hub is what preserves
in-process, zero-spend testing (the pattern `crates/fleetor-server/tests/messaging.rs` already uses).

The ack round-trip is a **HashMap lookup and a pty write** — sub-millisecond. There is a 2s
*timeout* on it purely so a wedged app can't park the CLI forever; it is a ceiling that should never
be reached, not a delay. Nothing in the delivery path sleeps or batches.

**Create** `crates/fleetor-server/tests/pane_messaging.rs` — the load-bearing test. In-process hub +
a fake `AppCommand` receiver recording deliveries. Covers: direct send accepted; send to a dead pane
rejected with the right detail; broadcast fans to N−1 with one shared `group`; `reply` routes to the
last inbound sender; rate-limit rejection; bodies land in the log.

**Verify**: `cargo test` workspace green, old tests untouched.

### Phase 2 — unwire the old fleet from the app

Surgical, and it makes Phase 5's deletion possible.

**Modify** `src-tauri/src/fleet.rs` — delete `spawn_pool_fleet` (L191-229), `build_factory`
(L416-458), `WorkerBackend`, `FakeWithEnv`, `resolve_backend`, `fake_script_path`, `shim_path`, and
the two factory tests. Replace with a plain `Hub::with_app(...).run(transport)` on the runtime.
**Keep** `spawn_follower`, `shell_dir`/`socket_path`, `load_api_key`/`parse_deepseek_key`, `note`.
**Replace** `scratch_repo()`/`ensure_scratch_repo()` with `ensure_testbed()` (seeds
`~/.fleetor/testbed/` with a real small project + `git init`) and a `target()` resolver reading
`~/.fleetor/config.json`, falling back to the testbed with a visible `Notice`. Drop the `fleetor-cc`
dep from `src-tauri/Cargo.toml`; add `tauri-plugin-dialog`.

**Verify**: `cargo check` in `src-tauri`; app launches; feed still streams; the four headless
standby workers are gone. Board shows empty — fine, deleted in Phase 4.

### Phase 3 — N-pane pty registry + the `fleet` CLI ⇒ **the demo**

The two halves must land together to be demonstrable.

**Rewrite in place** `src-tauri/src/pty.rs` (keep the file — the pty logic's git history is
valuable):
- `PtyState(Mutex<Option<Session>>)` → `PaneRegistry { panes: Mutex<HashMap<PaneId, Pane>> }`.
- `Pane { master, writer, child, out_channel: String, exit_channel: String, last_input_at, pending }`.
- Commands take a `pane` arg: `pty_spawn(app, state, pane, rows, cols)`, `pty_write(state, pane, data)`
  (also stamps `last_input_at`), `pty_resize`, new `pty_kill` for per-tab restart.
- `kill_session` → `kill_all`, and fix the leak: SIGTERM the child's **process group** (openpty gives
  it its own session), brief wait, then SIGKILL. A leaked Opus after window close is a money bug.
- **Per-id event channels** — `pty://output/orch`, `pty://output/2`, `pty://exit/2`. Not a shared
  channel with an id in the payload: with per-id names, pane 2's component *cannot* receive pane 1's
  bytes, and Tauri doesn't wake all five listeners on every chunk. Precompute the channel `String`
  into `Pane` at spawn so the reader thread doesn't `format!` per read.
- **Coalesce in the reader thread** — accumulate until ~16ms elapsed or 64KB buffered, then emit
  once. ~15 lines, highest-value perf item (see L6).
- Delete the MCP wiring: L30-34, L48-84, L105, L123-130.
- Factor the registry behind an `emit: Arc<dyn Fn(&str, String) + Send + Sync>` callback so the
  Tauri commands are thin wrappers and the registry is testable without an `AppHandle`.

**Create** `src-tauri/src/spawn.rs`:
- `orch_command(...)` — full env inherit (today's pty.rs L117-119), *then* override
  `PATH`/`TERM`/`COLORTERM`/`FLEETOR_PANE=orch`/`FLEET_SOCKET`. Order matters: ours must win.
- `worker_command(slot, ...)` — the `apply_env` posture transplanted from
  `crates/fleetor-cc/src/spawn.rs` L232-259, plus **`--permission-mode auto`**. cwd is a per-slot
  worktree under the user's chosen target.
  - **✓ Phase 0:** set `ANTHROPIC_AUTH_TOKEN`, **never `ANTHROPIC_API_KEY`** — with it set, the
    interactive TUI blocks on api-key approval and never reaches the input box. `apply_env`
    currently sets both.
  - **✓ Phase 0:** `env_remove("CLAUDE_CODE_CHILD_SESSION")` on **both** spawn paths, alongside the
    existing Opus/Sonnet default-model removals. It leaks through env inherit and silently disables
    transcript saving.
  - **✓ Phase 0:** `--permission-mode auto` verified prompt-free for Bash/Read (ran the testbed
    suite, returned the right count). The default is `manual`, so this flag is load-bearing.
- `seed_config_dir(dir, cwd)` — idempotent, merge-not-clobber.
  - **✓ Phase 0:** the minimum seed is **two keys**, not four — `theme` is unnecessary:
    ```json
    { "hasCompletedOnboarding": true,
      "projects": { "<abs cwd>": { "hasTrustDialogAccepted": true,
                                   "hasCompletedProjectOnboarding": true } } }
    ```
  - **⚠ ✓ Phase 0 — hard requirement:** `hasTrustDialogAccepted` is keyed by **absolute project
    path**, not global. When the user picks a new target, this must be re-run for *every pane's cwd
    under the new target* or the entire fleet silently reverts to a trust dialog. This is the most
    likely way to reintroduce L1 immediately after fixing it.
- `fleet_bin_path()` + `augmented_path()` with an explicit resolution ladder (see L4).
- Both commands pass the Phase-1 briefing via `--append-system-prompt` — *not* a `CLAUDE.md` in the
  worker's cwd, which would show in `git status` and could be deleted by the worker itself.

**Create** `src-tauri/src/deliver.rs` — the `AppCommand` receiver: registry lookup → **write
immediately** (bracketed paste, then `\r`) → resolve the ack. No idle guard, no `pending` buffer, no
flush ticker. This deletes the entire `lead://inject` / `injectQueue` / `lastInputAt` /
`INJECT_IDLE_MS` / flush-interval apparatus from React outright rather than moving it.

The only remaining delay is the **30ms between the closing `\x1b[201~` and the `\r`**.
**✓ Phase 0:** gaps of 0/10/30ms all submit reliably — pty stream ordering is preserved — and a
four-line body arrives intact without submitting early. Use 30ms regardless, so we don't depend on
CC batching the end-marker and the CR within one input-handler tick (a version-dependent detail).

*Tradeoff accepted:* an instant write into the **orch** pane can land mid-line if the operator is
typing at that moment. Worth it — worker→orch traffic is low-volume, and a mangled line is visible
and recoverable, whereas a delayed message is invisible and confusing. If it turns out to bite, the
fix is a "hold" toggle in the orch pane head, not a reinstated timer.

**Modify** `src-tauri/src/main.rs` — register `PaneRegistry`, the pane commands, and
`fleet_pick_target`/`fleet_target`; drop `fleet_board`/`fleet_assign`; init `tauri-plugin-dialog`;
`kill_all` on `CloseRequested`. And `src-tauri/capabilities/default.json` — add `dialog:allow-open`
to `permissions` (currently `["core:default"]` only, so the picker silently fails without it).

**Gut** `crates/fleetor-cli/` to the `fleet` binary: `Cargo.toml` bin rename `fleetor` → `fleet`,
deps down to `fleetor-core`/`fleetor-ipc`/`clap`/`anyhow`/`tokio` (features `rt` current-thread, not
`rt-multi-thread` — this is spawned once per Bash call, so startup and link size matter). Delete
`src/{probe/,quality.rs,run.rs,supervise.rs}`; rewrite `main.rs` (~120 lines).

**Create** `tests/fake-pane/fake-pane.sh` — a 5-line echo loop. With a `FLEETOR_PANE_CMD` override on
the spawn path this gives a fully deterministic, zero-token integration test.
**Create** `src-tauri/tests/panes.rs` — drives `PaneRegistry` directly: 5 panes spawn, each gets bytes
on *its own* channel and no other's, `pty_write` stamps idle, delivery parks then flushes,
`kill_all` reaps all five.

**Verify**: `cargo test` workspace + `src-tauri`. Then the manual demo — launch, start the fleet, type
into the orch *"run `fleet send 2 'say hello back with fleet reply'`"*, watch worker 2's terminal
receive it and answer. That's the whole product in one gesture, before any UI work.

### Phase 4 — the UI

**Delete** `TicketBoard.tsx`, `WorkerTranscripts.tsx` (its tab-strip pattern at L24-40 gets lifted
into the new tab strip first).

**Create**:
- `TerminalGrid.tsx` — `<PanelGroup>` with orch fixed left (`defaultSize={52} minSize={30}`) and a
  right panel holding the worker tab strip plus **all four worker `TerminalPane`s mounted at once**,
  three `.is-hidden`. Mounting all of them is mandatory, not an optimization — see L7.
- `MessageFeed.tsx` — the primary surface. One row per `Message` with the **body**, from→to, and a
  muted `undelivered — <detail>` when `accepted: false`. Broadcasts collapse by `group`.

**Modify**: `TerminalPane.tsx` becomes `{ pane, scrollback, live }`-driven, channels derived from
`pane`, inject apparatus and `SessionGate` deleted — the `[]`-dep mount effect, `decodeBase64`,
`FitAddon`, the 0×0 refit guard and the `ResizeObserver`+window-resize pair all stay unchanged
(already per-instance and correct) · `App.tsx` hoists the spend gate to a workspace overlay, adds
shared `selectedWorker` state (the missing piece `DashboardBand.tsx` L114 needed) and extends the
resize nudge to fire on **tab change** (L8) · `Sidebar.tsx` `View = "fleet" | "messages"` ·
`DashboardBand.tsx` cells become pane cells with live/dead + sent/received counts · **keep and
repurpose** `FleetGraph.tsx` — it is literally a message-flow graph, which is now the product; strip
`blocked`/`ask_lead`, and fix `edgeKind` which only reads `e.to` today so worker→orch never lit ·
`types.ts`/`useFleet.ts`/`api.ts` (wrap *all* pane commands here, fixing the existing convention
violation where `TerminalPane` invokes `pty_write` raw) · `styles.css` — retarget `.band` from
`1.55fr repeat(4,1fr) 0.95fr`, delete board/blocked rules; `.pane-slot`, `.terminal-pane` (incl. the
`min-width:0` from `6c557f2`), `.terminal-wrap`, `.terminal-host`, `.pane__head`, `.pane-gate*`,
`.is-hidden`, `.nav*` all survive as-is.

**Spend gate + target picker**: one fleet-level gate, not five, and the gate is where the target gets
chosen. The card states the target path (defaulting to the testbed, clearly labelled as such) with a
**Choose folder…** button wired to `fleet_pick_target`, plus two honest spend lines —
`orchestrator — your Opus, your account` and `4 workers — DeepSeek Flash on your DEEPSEEK_API_KEY`.
If no key resolves, offer **orchestrator-only** start rather than refusing to launch. The top bar
target becomes clickable to re-pick once running (with a confirm, since it respawns the panes).
Per-worker "restart" in each tab head replaces `Op::WorkerRestart`.

**Landmine**: change `autoSaveId="fleetor-shell-split"` (App.tsx L61) — a restructured PanelGroup
under the same id restores nonsense sizes from localStorage, on your machine only.

**Verify**: `tsc --noEmit && vite build`. Manual: all five terminals interactive; tab-switch preserves
each buffer (the `cc7c894` regression); divider drag reflows without clipping (the `6c557f2`
regression).

### Phase 5 — the deletion (now compiler-verified)

**Delete crates**: `crates/fleetor-shim/`, `crates/fleetor-cc/`.
**Delete modules**: `fleetor-server/src/{supervisor,quality,gate,runner}.rs`;
`fleetor-core/src/{report,review,gate,ticket,fenced,ownership,mail}.rs`.
**Delete tests/fixtures**: the eight superseded `fleetor-server/tests/*.rs`, `tests/fake-claude/`,
`tests/fixtures/{hooks,mcp,ndjson}/`.
**Modify**: root `Cargo.toml` members · `fleetor-server/Cargo.toml` (drop `fleetor-cc`, drop `libc` —
only `WorkerControl::kill` used it) · `fleetor-core/src/store.rs` — `Store` reduces to
`append_event`/`events_since`/`latest_seq` · `bus.rs` L203-237 delegation block shrinks in lockstep ·
`fleetor-db` — **append migration 0003** (never edit an existing one, per the file's own convention)
dropping `tickets`/`leases`/`mail`/`knowledge_proposals`/`backlog` **and `DELETE FROM events`**, plus
make `events_since` skip-and-warn on an undecodable payload instead of `?`-ing out (L9).

Do **not** keep `Op::Assign`, `Op::FleetStatus`, or `RunnerCommand` "just in case" — they are the seed
of the ticket system growing back. `AppCommand` fully replaces `RunnerCommand`.

**Verify**: `cargo test` workspace + `src-tauri`, `tsc --noEmit && vite build`. Expect ~3,500 lines out.

### Phase 6 — hardening

Briefings validated against a real Flash worker (the one phase with deliberate, bounded live spend) ·
rate limiter tuned and the anti-amplification clause adversarially tested (worker-1 broadcasts
"status?"; confirm no fountain) · coalescing window measured, chunks/sec instrument, decide on
`addon-canvas` · `kill_all` process-group teardown verified with `ps` after window close · README /
`building.md` / `docs/fleet-comms-map.md` rewritten against the new topology.

---

## Landmines

**L1 — Onboarding will silently wedge all four workers. VERIFIED.** All four
`~/.fleetor/_shell/cc-config/worker-*/.claude.json` contain `machineID`, `userID`, `firstStartTime`,
cached experiment data — and **none** of `hasCompletedOnboarding`, `theme`, or
`projects[cwd].hasTrustDialogAccepted`, because headless `-p` never needed them. An interactive
`claude` on those dirs sits on a theme picker forever while every `fleet send` reports
`accepted: true`. This is the #1 way Phase 3 looks like it works and doesn't. Hence Phase 0.

**L2 — `ANTHROPIC_API_KEY` triggers an interactive approval prompt.** `apply_env` sets both
`ANTHROPIC_API_KEY` *and* `ANTHROPIC_AUTH_TOKEN`. Headless didn't care; interactive CC asks "use this
API key?" and stores the answer under `customApiKeyResponses`. Drop the former for TUI workers, or
pre-seed the approval. Same class of wedge as L1, easy to miss because only the second var bites.

**L3 — Delivery is remote control of a text field, not messaging.** If a worker pane is showing a
permission prompt or a `/`-menu, injected bytes become menu navigation. Mitigations: bracketed paste
+ delayed `\r`; a worker permission posture that never prompts; and above all **make it visibly
best-effort** — `accepted` must never render as "delivered." The product is visibility; a delivery
model that quietly lies is the worst failure mode available.

**L4 — the shim's binary path is a hand-made symlink. VERIFIED.**
`src-tauri/target/debug/fleetor-shim → /Users/…/target/debug/fleetor-shim`, created by hand,
untracked, reproducible by nothing. The `shim_path()` doc comment claiming the workspace builds it
there is simply false — two separate cargo workspaces, two target dirs. **Do not inherit this trick
for `fleet`.** Explicit resolution ladder (`FLEETOR_FLEET_BIN` → sibling of `current_exe()` →
`<cwd>/../target/<profile>/fleet`), an existence check at spawn that emits a loud `Notice`, and
`cargo build -p fleet` added to `beforeDevCommand`/`beforeBuildCommand` in `tauri.conf.json`.

**L5 — broadcast amplification is the highest-consequence new failure mode.** Five peers, all able to
`broadcast`, all instructed to be helpful. Worker 1 broadcasts → three receive → each acknowledges →
each fans out → runaway burning real DeepSeek tokens while you watch a beautiful animated graph of
the fire. Today's system is immune only because workers are parked headless processes with no
volition between turns; live TUIs have volition. **All three mitigations required**: hub-side
per-pane token bucket that rejects with a readable error; an explicit brief clause *"never reply to a
broadcast unless it names you"*; a per-pane messages/min figure in the band so the ramp is visible.

**L6 — the Tauri event firehose. The chokepoints, in detail.**

The word "firehose" is about **event count, not byte volume**. Here is the full path every chunk
takes today (`pty.rs` L148-165 → `TerminalPane.tsx` L20-25, L74-77):

```
[Rust reader thread]                        [WKWebView / JS]
1. reader.read(&mut buf)  ──────────────▶
2. STANDARD.encode(...)      alloc, +33%
3. app.emit(name, string)    JSON serialize + escape
4.   └─ Tauri IPC bridge ────main-thread hop───▶ 5. listener callback fires
                                                 6. atob(payload)      alloc
                                                 7. per-char loop → Uint8Array
                                                 8. term.write(bytes)
```

**Why the count is high.** `read()` returns as soon as *any* data is available, not when the 8192
buffer fills. A `claude` TUI redraws its spinner and streams tokens in small writes — a few hundred
bytes each. So a 200-byte spinner frame makes one complete trip through all eight steps. The event
rate tracks the TUI's **repaint rate**, not its data rate. Steps 2, 3, 4, 6 and 7 are all
**per-event costs that barely shrink with payload size**, which is exactly what makes coalescing so
effective: merging 50 × 200B into 1 × 10KB removes 49 copies of the expensive part and adds almost
nothing to the cheap part.

**Ranked chokepoints:**

1. **The IPC bridge hop (step 4)** — dominant. Each `emit` crosses a Rust thread → WKWebView
   boundary as a serialized message. Per-event, largely size-independent, and it contends with the
   main thread that also has to render.
2. **Listener fan-out (step 5)** — `AppHandle::emit` broadcasts to *every* listener registered on
   that name. One shared `pty://output` means all five panes' callbacks wake for every chunk from
   every pane and four of them discard it — **5× the JS wakeups for zero benefit.** This is the
   whole argument for per-pane channels.
3. **base64 (steps 2, 6, 7)** — 33% inflation, one alloc in Rust, one in JS, plus `decodeBase64`'s
   per-byte JS loop. It exists only because Tauri's event payloads are JSON and raw bytes aren't
   JSON-safe.
4. **xterm's DOM renderer (step 8)** — the one everybody assumes is the problem, and mostly isn't.
   xterm already batches writes into an internal queue and flushes on rAF, and `display: none` on
   hidden panes skips layout and paint entirely. The keep-mounted constraint (L7) and the perf story
   happily coincide here — say so explicitly so nobody "optimizes" by unmounting.

**Fixes, in order of value:**

- **Per-pane channels** kills #2 outright, and is also a correctness win: pane 2 *cannot* receive
  pane 1's bytes if it isn't listening on that name.
- **Coalescing** (~16ms / 64KB window in the reader thread) collapses #1 and #3 by an order of
  magnitude. ~15 lines, backend-only, no API change.
- **✓ Phase 0 — `tauri::ipc::Channel` is NOT the win this plan assumed. Deferred to Phase 6.**
  Read from `tauri-2.11.5/src/ipc/channel.rs`: `InvokeResponseBody::Raw` exists, but under
  `MAX_RAW_DIRECT_EXECUTE_THRESHOLD = 1024` it serializes to a **JSON array of integers**
  (`[27,91,50,…]`) — roughly **4× the wire bytes of base64** — so for small chunks Channels are
  *worse* than what we do today. Above 1 KB it uses an efficient `fetch` pull, but that is exactly
  the size coalescing produces. Channels do genuinely remove fan-out (#2). **Phase 3 ships per-pane
  event names + base64 + coalescing** — minimal delta from known-good code; revisit Channels in
  Phase 6 behind a measurement, and only combined with coalescing so every payload clears 1 KB.
- Workers get `scrollback: 2000` (memory), orch keeps 10000.
- **Do not add `addon-webgl`** — context limits and context-loss-on-hide in WKWebView are worse than
  what they fix at 5 instances. If the two visible panes still stutter after the above, add
  `addon-canvas` to those two only.

Add a chunks/sec counter during Phase 6 so "is it the IPC?" is answerable without a profiler.

**L7 — every terminal must stay mounted.** There is no screen-replay mechanism in the backend
(commit `cc7c894`): unmount an xterm and its buffer is destroyed, `pty_spawn` no-ops, and the pane
comes back blank until the child repaints. `.is-hidden` keep-mounted is mandatory.

**L8 — the resize nudge must extend to tab switches.** `App.tsx` L36-40 nudges on view change.
Switching worker 2 → 3 takes pane 3 from 0×0 to sized, and its `refit()` guard correctly refuses to
fit at 0×0 — so without a nudge on tab change, pane 3 returns at a stale grid. Presents as exactly
the `cc7c894` bug.

**L9 — stale `state.db` rows crash boot after the enum trims. Downgraded — nothing in it matters.**
`events_since` hard-errors on an undecodable payload and the existing db is full of
`tool-activity`/`report-filed` rows. Since there's nothing worth keeping, migration 0003 just does
`DELETE FROM events` alongside the table drops. Skipping the skip-and-warn hardening; if it ever
recurs the symptom is now a known one.

**L10 — don't parse worker pty output to rebuild transcripts.** Deleting `WorkerTranscripts.tsx` and
`ToolActivity` will feel like losing observability. Terminal output is ANSI-mangled, redrawn in
place, and fragile against every `claude` release. The TUI *is* the view now.

---

## The testbed and the target picker

Two related changes: the fleet must never *default* to operating on a repo, and there needs to be a
dedicated place to exercise it.

**The testbed** — `~/.fleetor/testbed/`, created and `git init`-ed on first run, seeded with a small
but real multi-file project (a handful of source files, a README, a test command) so workers have
something genuine to read, edit, and talk to each other about. It lives under `~/.fleetor/` so the
existing teardown promise holds: `rm -rf ~/.fleetor` removes every trace. It is **not** inside the
repo — workers writing into a tracked directory would dirty `git status` on every run.

This replaces `ensure_scratch_repo()` / `scratch_repo()` (`fleet.rs` L107-109, L545-560), which
today hardcodes `~/.fleetor/_shell/repo` as an empty git dir.

**The target picker** — the fleet operates on whatever folder the user chose; the testbed is only
the first-run *value*, not a fallback the code reaches for.

- Add `tauri-plugin-dialog` (not currently a dep) and grant `dialog:allow-open` in
  `src-tauri/capabilities/default.json` (today: `["core:default"]` only).
- New commands `fleet_pick_target()` (native folder picker → validate it's a directory → persist →
  return the new `FleetConfig`) and `fleet_target()`.
- Persist to `~/.fleetor/config.json` — `{ "target": "<abs path>" }`. A path that no longer exists
  falls back to the testbed **with a visible `Notice`**, never silently.
- Top bar shows the real target and branch (`fleet_config` / `git_branch` already do this) and
  becomes clickable to re-pick. Changing the target while panes are live requires respawning them —
  gate it behind a confirm that says so.
- All five panes take their `cwd` from the chosen target. Workers additionally get per-slot
  worktrees beneath it, as today's `build_factory` did (`wt/worker-N`).

**Worker permission posture — decided.** Workers spawn with **`--permission-mode auto`** (confirmed a
valid choice on CC 2.1.220: `acceptEdits`, `auto`, `bypassPermissions`, `manual`, `dontAsk`, `plan`).
This resolves the prompt-wedge in L3 without going to `bypassPermissions`. The orch keeps the
operator's own default — it has a human watching it.

---

## Verification

| Layer | Command |
|---|---|
| Crates | `cargo test` (workspace root) |
| Tauri backend | `cargo check` + `cargo test` in `src-tauri/` |
| UI | `npx tsc --noEmit && npx vite build` |
| Integration, zero-spend | `crates/fleetor-server/tests/pane_messaging.rs` (fake `AppCommand` sink) and `src-tauri/tests/panes.rs` (`FLEETOR_PANE_CMD=tests/fake-pane/fake-pane.sh`) |
| End-to-end, live | Phase 3 demo, in the testbed: start the fleet, type `fleet send 2 "…"` into the orch, watch worker 2 receive and `fleet reply` — with no perceptible delay |
| Target picker | First run lands on `~/.fleetor/testbed/`; picking another folder persists across restart; a deleted target falls back with a visible `Notice`, not silently |
| Regressions to re-check by hand | tab-switch preserves each xterm buffer (`cc7c894`); divider drag reflows without clipping (`6c557f2`); `ps` shows no orphaned `claude` after window close |
