# WP-03 — `fleet cmd`: deliver `/clear` and `/compact` with a logged why

status: **landed** (2026-08-06, D-045) size: L
depends-on: 01 blocks: — (WP-04 makes it *useful*)
brief-cost: estimated +~120 tokens across both prompt files; **actually spent +248 orch / +228 worker** — see D-045 and `00-index.md` standing tension 1

**Two things in this doc were overruled by measurement and are left standing so the reasoning is legible:**

1. **"The bytes land raw: no bracketed paste"** (Outcome, and criterion 3 under Design sketch) is **wrong**. A typed slash command opens Claude Code's menu and `Enter` selects the wrong entry; a bracketed paste filters and selects correctly. The command channel is a new *caller* of `write_paste` with an unframed body, and the 30 ms `SUBMIT_GAP` needed no change. `docs/command-channel-notes.md`.
2. **The spike question "does typing `/compact` + CR execute it"** was answered before this session started, as an incidental finding of WP-02's spike. The six probes here answered what was left: empty box (both run), unsubmitted text in the box (the command becomes prose — a real, undetectable failure), and mid-turn (queued by CC, runs when the turn ends).

## Outcome

Any pane — including a pane targeting **itself** — can run `fleet cmd <pane|self> "/compact focus on the current task" --why "finished task block 3; context is mostly stale exploration"`. The bytes land in the target pty **raw**: no `[fleet · …]` prefix, no bracketed paste, followed by the submit CR — so the receiving TUI executes them as a slash command instead of reading them as text. Every command is logged with sender, target, exact command, the **mandatory why**, and the write outcome. The why-log is the reasoning chain the vision asks for: future self-improvement can study *when and why* the fleet decided to clear or compact.

## Performance criteria

### Technical
- [ ] New wire op (e.g. `Op::Cmd { to, command, why }`) and CLI verb; clap makes `--why` **required** — a contract on the sender, enforced before anything enters the path, same class as `Hello.pane`.
- [ ] Allowlist `["/clear", "/compact"]` as a Tier 2 constant, matched on the command word (`/compact <args>` passes; `/model` is refused). A non-allowlisted or non-`/`-prefixed body is refused **at accept time**, loudly, reason on stderr — a fact-check against a constant, the refusal class D-034 explicitly keeps (like the self-send guard). Once accepted, nothing may delay, drop, or alter it.
- [ ] Self-targeting allowed **for cmd only**. The message self-send guard at `hub.rs:148` and its pinning test (`pane_messaging.rs` — "a self-send is refused… not even logged") stay **byte-identical**.
- [ ] Raw write goes through the same per-pane serial writer path so a command can never interleave with an in-flight paste; the write uses the pane's writer mutex exactly as `write`/`write_paste` do.
- [ ] Log after the write (Tier 1.6). The outcome word is `accepted` with its honest meaning — bytes reached a live pty. Whether the command *fired* (the input box may not have been empty; a menu may have swallowed it) is unknowable from this side and never claimed (Tier 1.5).
- [ ] **The diff of the message path is empty.** `Op::Send/Broadcast/Reply`, `Message::framed`, `sanitize`, and `write_paste` are untouched. State this as a checked criterion in the PR description.
- [ ] Spike first (`building.md` §4): `examples/` + `docs/command-channel-notes.md`, version-stamped. Must answer: does typing `/compact` + CR into a real `claude` execute it when the input is empty? with queued text already in the box? mid-turn? Does the slash-command menu popup swallow the CR (does it need the 30 ms gap, a longer gap, or no gap)? Findings win over this plan.
- [ ] New event representation rendered distinctly in the UI — never as a message row. Recommended: a new `FleetEvent::Command` variant. The frontend renders unknown event kinds as *nothing*, so `ui/src/fleet/types.ts` + the feeds must be extended in the same session.

### Semantic
- Both prompt files teach the verb with the mandatory why: *note why you decided to send it, so future self-improvements can develop the reasoning chain* (the vision's own words, lightly edited).
- Worker prompt gains the post-task self-maintenance move: *when you finish a task block, evaluate your own context; if it is mostly stale, run `fleet cmd self "/compact keep <current task context>" --why "…"`.*
- Orch prompt gains the after-`/clear` rule: *if you clear a worker, immediately `fleet send` it its task context back.* A prompt rule, never a code gate.
- Commands render visibly distinct from messages in the Messages/Activity feeds, why displayed alongside.

## Invariant guardrails

- **The sharpest one in the roadmap (Tier 1.4 / D-034).** The original ask was phrased "the message router must chop off the prefix when typing into workers." That describes a content-inspecting transform *inside the message path* — the exact shape argued and lost twice. The framing here is different and non-negotiable: **commands are not messages; nothing is chopped.** A separate verb, separate op, separate delivery arm, raw write. If the design drifts toward inspecting message bodies for leading slashes, stop and re-read this section.
- **Refuse only at accept time**, against a constant. After acceptance: no queue semantics beyond the existing per-pane serial drain, no timeout, no retry.
- **Tier 1.5/1.6 verbatim.** Log outcomes after the write; `accepted` never inflates to "executed."
- **No automation.** No "compact at 80%" daemon, no system-triggered commands. The observer (WP-04) informs; an *agent* decides and owns the why. Auto-anything here is the deleted supervisor growing back.

## Current state (verified 2026-08-06 — do not re-explore)

- Framing that blocks slash commands today: `crates/fleetor-core/src/message.rs:86-99` (every delivery starts `[fleet · …]`), `sanitize` at `:118`; bracketed paste + CR in `src-tauri/src/pty.rs` — `PASTE_START` `:55`, `SUBMIT_GAP` (30 ms) `:64`, `write_paste` `:175-189`. The raw keystroke path already exists: `PaneRegistry::write` `:162-167` (used by operator keystrokes via `TerminalPane.tsx:185` onData). `writable()` gate `:245` uses `accepts_input()` (not-Dead), which cmd should share.
- Delivery loop: `src-tauri/src/deliver.rs` — `AppCommand` handling in `spawn_delivery` `:67`, per-pane serial writer `spawn_writer` `:99` (drains via `try_recv` `:106`, joins with `SEPARATOR` `:53`). Recommended: a separate `AppCommand::Command` variant so `Deliver`'s batching contract is untouched — but the command must still be serialized through the same per-pane writer task to avoid interleaving.
- Hub: `crates/fleetor-server/src/hub.rs` — `handle` dispatch `:136`, self-send guard `:148`, `deliver` `:227`, `log_message` `:236`, `ask_app` `:251` (no deadline, by design).
- Wire: `crates/fleetor-core/src/wire.rs` — `Op` `:54`, `OpResult` `:78`; a test pins wire tag == CLI verb. Events: `crates/fleetor-core/src/event.rs:20` (3 variants; the module doc `:4-8` explains why nothing speculative survives there).
- Verb/prompt coupling (post-WP-01): `validate_orch`/`validate_worker` refuse a prompt that fails to teach every `VERBS` entry — the new verb, both prompt files, the clap enum, and the pinned tests must move in **one commit**.
- Related CC flag noticed on 2.1.223: `--autocompact <auto|tokens>` exists. Deliberate `/compact`-with-a-why is this package; autocompact is a spawn-time Tier 2 default that may complement it.

## Scope

### In
Wire op + hub arm + `AppCommand::Command` + raw-write delivery + CLI subcommand + allowlist + event variant + UI rendering + prompt text + tests (allowlist refusal; self-cmd accepted; message self-send still refused; raw bytes verified against a fake-pane pty; message-path diff empty).

### Out
Any other slash command (the allowlist is the boundary — `/model`, `/exit` refused); system-triggered commands; Carryover handover files; touching `--autocompact` defaults (open question only).

## Design sketch & open questions

1. **Event shape.** Recommended: new `FleetEvent::Command { from, to, command, why, accepted, detail }`. A `Notice` would bury the reasoning chain; a marked `Message` would violate "commands are not messages."
2. **`AppCommand` variant vs a `kind` field on `Deliver`.** Recommended: separate variant — `Deliver`'s batch-and-join contract (D-039) stays untouched.
3. **Submit timing.** The paste path waits 30 ms before CR. A bare command may need different timing if the slash menu intercepts — the spike decides; hardcode nothing until measured.
4. **Spawning panes.** Recommended: same rule as messages — `accepts_input()` (only Dead refuses); kernel-buffered bytes at a booting TUI are the tolerable failure.
5. **`--autocompact` interplay.** Recommended: leave CC defaults alone this session; note the flag in the notes doc as a Tier 2 lever for later measurement.

## Session prompt

```
You are working in /Users/bubblyducks/harness/fleetor. Read
docs/futureDesign/requirements/03-command-channel.md in full, then
building.md §1, §4, §9 — especially Tier 1.4 and the guardrails section of
the doc: commands are a new parallel path, NOT a transform on messages; the
message path's diff must be empty. Spike first against a real claude TUI
(examples/ + docs/command-channel-notes.md): verify typed-slash-command +
CR execution semantics (empty input / queued text / mid-turn / menu
timing). Then implement fleet cmd with the mandatory --why, the
["/clear","/compact"] allowlist refused only at accept time, self-targeting
for cmd only (message self-send guard byte-identical), raw unframed
unpasted write through the per-pane serial writer, a new FleetEvent
rendered distinctly in the UI, and prompt/VERBS/clap/tests moving in one
commit. Finish with the session exit checklist.
```

## Session exit checklist

- [ ] Spike notes committed, version-stamped.
- [ ] Full test matrix green; message-path diff verified empty (state it in the PR).
- [ ] `decisions.md` entry: the verb, the allowlist constant, the why requirement.
- [ ] Both prompt files + `VERBS` + clap + pinned tests moved in one commit.
- [ ] `00-index.md` status updated.
