# opencode spike notes

**Decisions: D-140–D-145.** WP-32, registering opencode through the seam.

**Version stamp: `opencode v2.2.5`** (macOS, arm64), operator on the OpenCode Go plan.

Re-runnable, **zero tokens**: `python3 -I examples/opencode-spike/tui_probe.py <empty dir> --standalone --auto`.

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
