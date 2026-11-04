# opencode spike notes

**Decisions: D-140–D-145.** WP-32, registering opencode through the seam.

**Version stamp: `opencode v2.2.5`** (macOS, arm64), operator on the OpenCode Go plan.

Re-runnable, **zero tokens**: `python3 -I examples/opencode-spike/tui_probe.py <empty dir> --standalone --auto` and `system_prompt_probe.py <empty dir>`. **Spends turns**: `PROBE_MODEL=<provider/model> python3 -I examples/opencode-spike/turn_probe.py <empty dir> main|resume|baseline`.

**Never rewritten** (`building.md` §4). Findings append.

---

## 2026-11-02 — zero-token findings

1. **The default command refuses `--model` and `--agent`.** `Unrecognized flag: --agent in command opencode` / `--model`, exit 1. The CLI docs list both on the default command; on this build only `opencode mini` and `opencode run` take them. The seat's model and agent are therefore config: `model` and `default_agent` in `OPENCODE_CONFIG_CONTENT`. With that, the status line reads `Fleetor · auto` and `DeepSeek V4.1 Flash · OpenCode Go`.
2. **Without `--standalone`, the seat's environment does not reach the config.** `opencode debug config` lists only the operator's global config when the seat's `OPENCODE_CONFIG_DIR` is set; with `--standalone` it lists the seat's directory. The background service owns configuration, so every pane runs `--standalone` (D-144).
3. **`OPENCODE_CONFIG_DIR` replaces the global config directory, it does not layer on it.** The operator's `~/.config/opencode/opencode.json` (the `local-node` provider) drops out; setting `OPENCODE_CONFIG` to that file brings it back as a read source. The Go plan's 30 models are listed either way: the login is in the data directory's `opencode.db` (`credential`, `account` tables), not in `auth.json` and not in config.
4. **No trust gate.** An unseeded pane in a fresh `git init` reaches its input box (`Ask anything…`) directly.
5. **No session is created at launch.** `session` and `session_v2` hold no row for the pane's directory after 15s idle.
6. **The process tree is one process**: `opencode --standalone --auto`, comm `opencode`. The private server runs in-process.
7. **Terminal modes requested**: `2004` (bracketed paste), `1049`, `1000/1002/1003/1006` (mouse), `2026`, `2027`, `2031`, `25`.
8. **v2 plugins have a different shape.** The v1 form (a named async export returning hooks) is refused: `Plugin must export a default definition with an id and an effect or setup function.` A plain-object default export `{ id, async setup(ctx) }` with no SDK import loads from `$OPENCODE_CONFIG_DIR/plugins/`. `ctx` carries `tool` (`reload`, `list`, `transform`, `hook`) and `permission` (`hook`, `list`, `get`, `reply`, `assert`); `ctx.tool.hook` takes `(name, callback)`.
9. **The operator's `opencode.json` is never written** by a relocated pane (mtime unchanged).
10. **Usage is per session in the database**: `session` / `session_v2` carry `model`, `agent`, `tokens_input`, `tokens_output`, `tokens_reasoning`, `tokens_cache_read`, `tokens_cache_write`. A gauge candidate; not yet read after a real turn.

## Open — needs a real turn (spends the operator's Go plan)

Whether `ctx.tool.hook("execute.before", …)` throwing blocks the call (D-142); whether `external_directory: deny` holds under `--auto`; whether the `fleetor` agent's `prompt` replaces the default system prompt (sentinel not in the pasted bytes) and survives `/new` (D-145); bracketed paste + CR submits; `--session <id>` resumes; `/compact`; which row the gauge reads mid-turn; that `fleet` reaches the hub socket (no vendor sandbox, so expected).

## 2026-11-02 — paid findings (`examples/opencode-spike/turn_probe.py`, ~15 turns on `opencode-go/beam-free`)

Every arm ran `opencode --standalone --auto` with the seat config of D-141–D-145. `opencode-go/deepseek-v4.1-flash` failed every turn upstream (`This Go model requires Global regions … in your workspace's Privacy settings`), and `glm-5.3-flash` / `claude-haiku-5-5` returned `OpenCode Go 5 hour usage limit reached`; the free `beam-free` was used instead.

| Arm | Result |
|---|---|
| Bracketed paste + CR submits | **pass** — every turn |
| Brief sentinel (`agent.fleetor.prompt`) reaches the model | **pass** |
| Brief survives `/new` (new session row, still agent `fleetor`) | **pass** |
| Plugin `execute.before` throw refuses a `shell` call | **pass** — `executed: false`, `status: error`; the model reads the thrown message |
| Plugin throw refuses a `write` call | **pass** |
| `external_directory: deny` refuses a `write` outside the cwd under `--auto` | **pass** — `Permission denied: external_directory` |
| `external_directory: deny` refuses a `shell` write outside the cwd | **fail** — the file was written |
| `--auto` runs a shell write inside the cwd unprompted | **pass** |
| Unix socket outside the worktree reachable from `shell` | **pass** — no vendor sandbox |
| `/compact` | **pass** — a `compaction` record (`reason: manual`) with a summary, same session |
| `--session <id>` resumes the conversation | **pass** |

1. **The hook event** is `{tool, sessionID, agent, messageID, id, input}`. Tool names are opencode's own: `shell` (not `bash`), `write` with `input.path`. The hook runs before the permission check: the `ext-write` call reached the hook, then `external_directory` refused it.
2. **The agent prompt replaces opencode's base prompt** (`examples/opencode-spike/system_prompt_probe.py`, zero tokens, a local stub provider recording the request). The default agent's system prompt opens `You are an AI agent running in OpenCode…` with sections `# Harness`, `# Communication`, `# Working in codebases`, `# Your Model`, `# Code Mode`; the `fleetor` agent's opens with the brief and keeps only `# Your Model` and `# Code Mode` (tool documentation, ~34k of its 35.5k chars).
3. **The transcript is `session_message`** (v2), keyed by `session_id`, types `user`, `assistant`, `compaction`, `idle`. Each `assistant` row carries `tokens: {input, output, reasoning, cache: {read, write}}`; the context in use is the last one's `input + cache.read + cache.write`. `session_v2.tokens_*` are running totals, not the context.
4. **Input-token comparison is not a replacement test.** One `ok` turn cost 10,735 + 448 cached on `fleetor` and 9,399 + 2,048 on the default agent; the base prompt is small beside ~34k chars of tool documentation, so the totals barely differ.

## 2026-11-04 — zero-token findings for the registration (`examples/opencode-spike/guardrail_probe.py`)

1. **The write tools** (from `ctx.tool.list()`, tools whose permission is `edit`, plus `shell`): `shell {command, workdir, timeout, background}`, `write {path, content}`, `edit {path, oldString, newString, replaceAll}`, `patch {patchText}`, `worktree_remove {directory, force}`. The plugin turns `patchText` headers (`*** Add/Update/Delete File:`, `*** Move to:`) into `edits[].file_path`, `directory` into `path`, and runs a `shell` call's check in its `workdir`.
2. **The guardrail plugin refuses every write outside the root and none inside**, driven through a local stub provider that emits the tool call: `write`, `shell`, `shell` with `workdir`, `edit` and `patch` outside are refused (5 journal entries); `write` and `shell` inside land.
3. **The posture reads back from `opencode debug agents --standalone`** with the seat's `OPENCODE_CONFIG_CONTENT`, under the operator's real config: the `fleetor` agent's last `*`/`*` rule is `allow`, its last `external_directory` rule is `deny`, and every rule is `{action, resource, effect}` (D-149).
4. **An inline agent `prompt` in `OPENCODE_CONFIG_CONTENT` reaches the agent** (`debug agents` shows it as `system`), so a pane carries its brief without a path in its environment.
5. **`opencode run` takes its directory from `$PWD`, the TUI from the process cwd.** Measured both ways under a conflicting `PWD`. Panes run the TUI, so a pane's inherited `PWD` is harmless; a probe using `run` must set `PWD`.
6. **`opencode run` waits on an open stdin.** Probes pass `stdin=DEVNULL`.
