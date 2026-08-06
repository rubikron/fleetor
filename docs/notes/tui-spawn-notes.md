# Phase 0 — interactive `claude` spawn notes

Measured against **Claude Code 2.1.220** on macOS 24.6.0, via
`examples/tui-spawn-spike/spike.py` (a pty harness that spawns a real `claude` the way the
app will, logs raw bytes, and can bracketed-paste a message in). Run outputs land in
`examples/tui-spawn-spike/work/` and are gitignored.

Everything below is **verified by running it**, not inferred.

---

## 1. The config seed — L1 confirmed, and it is absolute

A virgin `CLAUDE_CONFIG_DIR` does not "sometimes" hit onboarding. It **never reaches a
prompt at all**:

```
Let's get started.
Choose the text style that looks best with your terminal
❯ 2. Dark mode ✔
```

The four worker config dirs on disk (`~/.fleetor/_shell/cc-config/worker-*/.claude.json`)
are exactly in this state — they carry `machineID`/`userID`/`firstStartTime` and none of the
onboarding keys, because headless `-p` never needed them. Every `fleet send` into such a
pane would have reported success into a theme picker.

### Bisect

| Seed | Result |
|---|---|
| *(none)* | theme picker — **blocked** |
| `theme` only | theme picker + intro — **blocked** |
| `hasCompletedOnboarding` | clears theme; **blocked** on trust dialog |
| `hasCompletedOnboarding` + `theme` | **blocked** on trust dialog |
| `hasCompletedOnboarding` + `projects[cwd].hasTrustDialogAccepted` | ✅ **prompt reached** |
| all four candidates | ✅ prompt reached |

### The minimum seed — two keys

```json
{
  "hasCompletedOnboarding": true,
  "projects": {
    "<absolute cwd>": {
      "hasTrustDialogAccepted": true,
      "hasCompletedProjectOnboarding": true
    }
  }
}
```

`theme` is **not** required (`hasCompletedOnboarding` alone clears the picker). Set it anyway
for deterministic rendering — it is free and keeps the five panes visually identical.

> **⚠ Implementation consequence — `hasTrustDialogAccepted` is keyed by absolute project
> path.** It is not a global flag. When the user picks a new target folder, `seed_config_dir`
> must be re-run for **every pane's cwd under the new target**, or the whole fleet silently
> reverts to a trust dialog. This is a hard requirement on the Phase 3 target picker, and the
> most likely way to reintroduce L1 after fixing it.

A healthy boot looks like:

```
▐▛███▜▌  Claude Code v2.1.220
▝▜█████▛▘ deepseek-v4-flash · API Usage Billing
   ▘▘ ▝▝  ~/.fleetor/testbed
❯
⏸ manual mode on · ? for shortcuts   ◈ max · /effort
```

Note `⏸ manual mode on` — the default posture really would wedge on the first tool call.

## 2. `ANTHROPIC_API_KEY` — L2 confirmed

Identical seed, identical everything, one extra env var:

| Env | Result |
|---|---|
| `ANTHROPIC_AUTH_TOKEN` only | ✅ prompt reached |
| `ANTHROPIC_AUTH_TOKEN` **+ `ANTHROPIC_API_KEY`** | ❌ api-key approval prompt — **blocked** |

`crates/fleetor-cc/src/spawn.rs::apply_env` L232-246 sets **both**. Headless didn't care;
interactive CC asks *"Do you want to use this API key?"* and never reaches the input box.

**`worker_command` must set `ANTHROPIC_AUTH_TOKEN` and must not set `ANTHROPIC_API_KEY`.**

## 3. Bracketed paste — works, including multi-line

Sequence: `\x1b[200~` + body + `\x1b[201~`, then `\r`.

| Gap before `\r` | Submitted? |
|---|---|
| 0 ms | ✅ yes |
| 10 ms | ✅ yes |
| 30 ms | ✅ yes |

Ordering in the pty stream is preserved, so even a 0 ms gap works. **Use 30 ms anyway** —
it costs nothing perceptible and doesn't rely on CC batching the paste-end marker and the CR
within one input-handler tick, which is a version-dependent detail.

**Multi-line bodies do not submit early.** A four-line message
(`[fleet · worker-3] three things:\nfirst\nsecond\nReply with exactly: got 3`) arrived as one
message and the model replied `got 3`. CC collapses it in the UI with a
*"paste again to expand"* affordance; the full content reaches the model.

## 4. `--permission-mode auto` — no prompts

`auto` is a valid choice on 2.1.220 (`acceptEdits`, `auto`, `bypassPermissions`, `manual`,
`dontAsk`, `plan`). Spawned with it, the footer reads `⏵⏵ auto mode on (shift+tab to cycle)`.

Sent *"Run ./run-tests.sh and reply with just the number of tests that passed."* — the worker
ran the shell command with **no permission prompt** and answered `11`, the correct count.

Bash + Read are covered. Write/Edit were not separately exercised; if one ever prompts, the
symptom is a pane that stops responding to delivered messages, and the escape hatch is the
per-tab restart planned for Phase 4.

## 5. `tauri::ipc::Channel` — **do not adopt yet.** Coalescing is the real fix.

Read from `tauri-2.11.5/src/ipc/channel.rs`. `InvokeResponseBody::Raw(Vec<u8>)` exists, but
the transport depends on a size threshold — `MAX_RAW_DIRECT_EXECUTE_THRESHOLD = 1024`:

- **< 1 KB** → `serde_json::to_string(&bytes)` producing a **JSON array of integers**
  (`[27,91,50,...]`) injected via `webview.eval()` as `new Uint8Array([...]).buffer`.
  That is roughly **4× the wire bytes of base64** (1.33×), plus a JS array parse.
  **For small chunks, Channels are worse than what we do today.**
- **≥ 1 KB** → queued and pulled by the webview over `fetch`. Efficient transfer, but an
  extra async round-trip per chunk.

So the plan's optimistic framing ("Channels eliminate base64") is wrong as stated. What is
true:

- Channels do remove **listener fan-out** and event-name matching (they are point-to-point) —
  that part stands, and it is chokepoint #2 from L6.
- Their raw path only pays off **above 1 KB**, which is precisely what coalescing produces.

**Revised Phase 3 decision:** ship **per-pane event names + base64 + a 16 ms / 64 KB
coalescing window**. That is the minimal delta from today's known-good code and it fixes
chokepoints #1 and #2 outright. Revisit Channels in Phase 6 **with a measurement**, and only
in combination with coalescing so every payload clears the 1 KB threshold.

Supporting datapoint: the spike harness `select()`s on a 100 ms timeout and reads up to 64 KB,
which is accidentally the same shape as the proposed coalescer. A full worker turn produced
**45 chunks over 28 s (~2 reads/sec)** — trivial event volume. That doesn't disprove the
firehose (today's code reads 8 KB in a tight loop with no batching); it demonstrates that
~10 Hz coalescing flattens it completely.

## 6. Incidental finding — `CLAUDE_CODE_CHILD_SESSION` leaks in

Every spike run showed:

```
⚠ Transcript saving is off — inherited CLAUDE_CODE_CHILD_SESSION marker
```

The spike inherits it because it was launched from inside a Claude Code session. The **orch**
pane inherits the operator's full environment by design (`pty.rs` L117-119), so if the app is
ever launched from a terminal inside a `claude` session, the orch — and every worker, which
also inherits process env before overriding — picks this up and silently disables transcript
persistence.

**Add `CLAUDE_CODE_CHILD_SESSION` to the `env_remove` list on both spawn paths**, alongside
the existing `ANTHROPIC_DEFAULT_OPUS_MODEL` / `ANTHROPIC_DEFAULT_SONNET_MODEL` removals.

---

## Net changes to the plan

1. Seed is **two keys**, not four — and `hasTrustDialogAccepted` is **per absolute path**, so
   the target picker must re-seed every pane cwd. New hard requirement on Phase 3.
2. `worker_command`: `ANTHROPIC_AUTH_TOKEN` yes, `ANTHROPIC_API_KEY` **no**. Confirmed, not assumed.
3. Paste gap: **30 ms**, multi-line safe. No further investigation needed.
4. `--permission-mode auto`: confirmed prompt-free for Bash/Read.
5. `tauri::ipc::Channel`: **deferred to Phase 6 with a measurement.** Phase 3 ships per-pane
   event names + base64 + coalescing. Coalescing is load-bearing, not an optimization.
6. New: `env_remove("CLAUDE_CODE_CHILD_SESSION")` on both spawn paths.
