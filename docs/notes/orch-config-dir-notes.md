# WP-14 — giving `orch` its own `CLAUDE_CONFIG_DIR` without logging it out

Measured against **Claude Code 2.1.224** on macOS 24.6.0 (Darwin), 2026-08-07, via
`examples/orch-config-dir-spike/probe.py`. Everything below was **run**, not inferred,
except where it says otherwise. Run output lands in `examples/orch-config-dir-spike/work/`
and is gitignored.

The question WP-14 rests on: `orch` is the operator's own `claude`, and the only lever
that moves its session transcript inside `~/.fleetor` is `CLAUDE_CONFIG_DIR`. Does
setting it cost `orch` its login?

**Yes — and there is a documented-in-the-binary way to keep it.** Both halves matter,
because the failure is the quiet kind: the pane still reaches its input box, so every
`fleet send` into it reports `accepted` while the model behind it has no credential.

---

## 1. The four arms

All four spawn a real interactive `claude` in a pty in `~/.fleetor/testbed`, with the
orch posture from `spawn.rs` (full environment inherit, `TERM`/`COLORTERM`,
`FLEETOR_PANE`, `CLAUDE_CODE_CHILD_SESSION` removed). Nothing is ever submitted, so the
first three arms spend **zero tokens** — reaching the input box is the whole answer.

Run against `~/.local/bin/claude` directly, not the `claude` on `PATH`: this machine has
a wrapper shim in front of it, and `augmented_path()` puts `$HOME/.local/bin` early, so
the real binary is what a FLEETOR pane resolves.

| Arm | `CLAUDE_CONFIG_DIR` | Seed | Reached a prompt? | Banner said |
|---|---|---|---|---|
| `control` | *(unset — the operator's own)* | n/a | yes (behind the what's-new panel) | `Opus 5 (1M context) with high… · Claude Max`, `Welcome back` |
| `virgin` | fleet-owned, fresh | none | **no — theme picker + welcome** | — |
| `seeded` | fleet-owned, fresh | the two keys | yes | `Opus 5 (1M context) · API Usage Billing` + **`Not logged in · Run /login`** |
| `securestorage` | fleet-owned, fresh | the two keys | yes | `Opus 5 (1M context) · Claude Max` |

Two findings, in the order they were found.

### L1 applies to `orch` exactly as it applies to a worker

The `virgin` arm never reaches a prompt — theme picker, then the welcome screen. So
`seed_config_dir` is not a worker-only concern the moment `orch` stops using the
operator's already-onboarded dir. Same two keys, same absolute-path-keyed trust flag,
same consequence if skipped (`docs/notes/tui-spawn-notes.md` §1).

### The seed is not enough: a fleet-owned config dir is a *different login*

The `seeded` arm reaches the input box and is **not logged in**. The mode line carries
`Not logged in · Run /login` and the banner falls back from `Claude Max` to
`API Usage Billing` with no key behind it. This is the risk-register failure mode
verbatim: a healthy-looking pane whose every send is accepted and whose first turn fails.

---

## 2. Why — the mechanism, read out of the binary

`claude` stores its OAuth credential in the macOS Keychain, and **namespaces the service
name by the config directory**. From the 2.1.224 bundle:

```js
function D7(e=""){
  let t = process.env.CLAUDE_SECURESTORAGE_CONFIG_DIR,
      r = t !== void 0 ? !t : !process.env.CLAUDE_CONFIG_DIR,
      n = t !== void 0 ? t.normalize("NFC") : Tn(),
      o = r ? "" : `-${createHash("sha256").update(n).digest("hex").substring(0,8)}`;
  return `Claude Code${OAUTH_FILE_SUFFIX}${e}${o}`;
}
```

…used as `security find-generic-password -a "${USER}" -w -s "${D7(...)}"`.

Read it as three cases:

| `CLAUDE_SECURESTORAGE_CONFIG_DIR` | `CLAUDE_CONFIG_DIR` | Keychain service |
|---|---|---|
| unset | unset | `Claude Code…` — **the operator's own entry** |
| unset | set | `Claude Code…-<sha256(config dir)[..8]>` — a fresh, empty namespace |
| **set to `""`** | set | `Claude Code…` — **the operator's own entry** |
| set to a path | anything | that path's namespace |

So the escape hatch is an environment variable that is *defined and empty*. It changes
nothing about where the credential lives or who owns it — `claude` performs the identical
keychain read it performs today. FLEETOR reads nothing, copies nothing, and never opens
the operator's `CLAUDE_CONFIG_DIR`.

The `securestorage` arm confirms it end to end: fleet-owned config dir, two-key seed,
`CLAUDE_SECURESTORAGE_CONFIG_DIR=""` → `Opus 5 (1M context) · Claude Max`, no
`Not logged in`, input box ready.

> **This variable is not in `claude --help`.** It is an internal, and it is version-stamped
> here at CC 2.1.224 for that reason. See §4 for what happens when it stops working.

---

## 3. The transcript lands where rotation already looks

The three interactive arms leave **no `projects/` directory at all** — the session file
is written on the first *turn*, not at session start. So one arm costs money, and is kept
as cheap as a turn can be: print mode, the smallest model, one word in and one word out.

```
claude -p "reply with the single word: ok" --model haiku
```

under the fleet-owned config dir. Exit 0, stdout `ok`, and afterwards:

```
<config dir>/projects/-Users-bubblyducks--fleetor-testbed/
    e66240de-c413-4dd1-ac6a-b0b3407786de.jsonl   (13,257 B)
    memory/
```

That is character-for-character the shape `runs::harvest_transcripts` already walks for a
worker — `pane-config/<pane>/projects/<slug>/*.jsonl`, the slug being the resolved
absolute cwd with `/` and `.` replaced by `-` (`docs/notes/context-gauge-notes.md`
measured the same path for a worker at CC 2.1.223). Putting `orch`'s config dir at
`_shell/pane-config/orch/` therefore needs **no change to rotation at all**: the existing
scan finds it and files it under `transcripts/orch/`.

---

## 4. What this does not buy back, and what breaks it

**Not restored:** the operator's user `CLAUDE.md`, skills, subagents, MCP servers,
settings and hooks. Those are read from `CLAUDE_CONFIG_DIR`, and `orch`'s is now a
fleet-owned directory holding two onboarding keys.
`docs/notes/system-prompt-notes.md` §3 measured that they load for `orch` today; after
this change they do not. That is a real loss and the price of the package — see the
D-entry. `CLAUDE_SECURESTORAGE_CONFIG_DIR` moves the credential lookup and nothing else.

**If `CLAUDE_SECURESTORAGE_CONFIG_DIR` stops being honoured** by a future CC, `orch`
spawns into the fleet dir's own (empty) keychain namespace and shows
`Not logged in · Run /login`. The degradation is bounded and self-healing: running
`/login` once inside the `orch` pane writes the credential into *that* namespace and it
persists, because the config dir persists across runs. It cannot disturb the operator's
own entry. This is why `spawn_pane` puts the sentence on the Activity feed rather than
leaving the operator to diagnose a pane that looks fine.

---

## 5. Reproducing

```bash
python3 examples/orch-config-dir-spike/probe.py --bin "$HOME/.local/bin/claude"
python3 examples/orch-config-dir-spike/probe.py --bin "$HOME/.local/bin/claude" --arm securestorage
python3 examples/orch-config-dir-spike/probe.py --bin "$HOME/.local/bin/claude" --arm transcript  # costs one turn
```

The first three arms are free. `--arm transcript` is the only one that spends, and it
spends one haiku turn.
