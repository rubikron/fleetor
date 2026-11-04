# cursor spike notes

**Decisions: D-128.** WP-24, re-measuring the M-series on a new build.

**Version stamp: `cursor-agent 2026.10.22-27dcf5f`** (macOS, arm64). The M-series was measured on `2026.09.02-c22c1a3`; that install broke (`Cannot find module 'tree-sitter'`) and was replaced.

Re-runnable, **zero tokens**: `python3 -I examples/cursor-spike/tui_probe.py <empty dir> [args]`.

**Never rewritten** (`building.md` §4). Findings append.

---

## 2026-11-02 — zero-token findings

1. **The login lives only in the macOS keychain.** `cursor-agent status` under a private `HOME` reports `Not logged in`, with or without a copy of `~/.cursor`. `cursor-agent login` under a private `HOME` fails: `couldn't save your login to the macOS keychain (security exit code 154)`, because that `HOME` has no default keychain. The vendor's suggested fix (`security delete-generic-password -s cursor-access-token …`) is wrong here and would delete the operator's real login. Basis for D-128.
2. **`CURSOR_CONFIG_DIR` relocates the configuration and keeps the login.** With the real `HOME`, `CURSOR_CONFIG_DIR` and `CURSOR_DATA_DIR` set to fresh directories, `status` reports logged in, `cli-config.json` is written into the relocated directory, and the operator's `~/.cursor/cli-config.json` is untouched (mtime unchanged).
3. **An unseeded pane parks on a trust gate**: `⚠ Workspace Trust Required`, `[a] Trust this workspace` / `[q] Quit`.
4. **`--trust` clears the gate in the TUI too**, and the pane reaches its input box. It writes `$CURSOR_DATA_DIR/projects/<key>/.workspace-trusted` (`{"trustedAt", "workspacePath", "trustMethod": "cli-flag"}`), where `<key>` is the canonical cwd with every run of non-alphanumerics collapsed to one `-` and the ends trimmed (`/private/tmp/a/-b` → `private-tmp-a-b`; corrected 2026-11-02, see below).
5. **A chat is created at launch**, before any message: `$CURSOR_CONFIG_DIR/chats/<md5 of cwd>/<chat uuid>/meta.json`, `"hasConversation": false`.
6. **The process tree is two processes**: the launcher, whose command is `~/.local/bin/cursor-agent`, and a child running the bundled interpreter `~/.local/share/cursor-agent/versions/<build>/node`. The installer now also links the same binary as `agent`.

## Open — needs a real turn (spends the operator's Cursor plan)

Brief via `.cursor/rules/*.mdc` (M5) and whether it survives `/clear`; sandbox write refusal (M11, M12); socket reach under `networkAccess` (M28); bracketed-paste delivery (M26); `/compact`; the transcript format once `hasConversation` is true (M22); `--resume`.

## 2026-11-02 — paid findings (`examples/cursor-spike/turn_probe.py`, ~17 turns, model shown as `Composer 2.5 Fast`)

Every arm ran with `--trust --sandbox enabled`, `sandbox.networkAccess: allow_all` in the pane's `cli-config.json`, and the real `HOME` (D-128).

| Arm | `approvalMode: allowlist` (default) | `unrestricted` | `allowlist` + `permissions.allow: ["Shell(*)"]` |
|---|---|---|---|
| Brief: `alwaysApply` `.mdc` in the worktree reaches the model (sentinel not in the pasted bytes) | **pass** | — | — |
| Brief survives `/clear` | **pass** | — | — |
| Bracketed paste + CR submits (M26) | **pass** | — | — |
| `--resume <chat id>` keeps the conversation | **pass** | — | — |
| Shell runs without a prompt inside the worktree | not measured | **yes** | **yes** |
| Shell write outside the worktree | **parks on `Run this command? Not in allowlist`** | **written** | **written** |
| File-tool write outside the worktree | not refused visibly | **written** | **parks on `Write to this file?`** |
| Unix-socket connect outside the worktree | parked on the prompt — not measured | **connected** | not measured (a prompt was pending) |

1. **M11 does not hold on this build.** `--sandbox enabled` is not an auto mode: under the default `allowlist`, every shell command not in the allowlist waits for approval. `unrestricted` is the TUI's "Run Everything" and writes outside the workspace by both channels, so it is `--force` by another name. Allowlisting `Shell(*)` runs shell commands unprompted and unconfined. No setting measured here both auto-runs and refuses an outside write.
2. **A pending approval prompt reads keystrokes.** Its choices are single letters (`y`, `n`, `p`, tab, shift+tab), and the next arm's pasted text arrived while one was open. A fleet message typed into a parked cursor pane can answer its prompt. Not measured whether a bracketed paste is ignored by the prompt or read as keys.
3. **The transcript is SQLite**: `$CURSOR_CONFIG_DIR/chats/<md5 of cwd>/<chat uuid>/store.db` (WAL), plus `prompt_history.json`. `/clear` starts a new chat directory. The status line shows a context percentage (`6.5%`).
4. `/compact` was accepted with no visible error; what it did is not measured.
5. Not measured: `approvalMode: auto-review` (a server classifier, M11's rejected shape); whether the sandbox needs the helper binary (`CURSOR_AGENT_HELPER_*`) to be active at all.

## 2026-11-02 — is the sandbox active? (`examples/cursor-spike/sandbox_probe.py`, ~2 turns)

**No.** With `sandbox.mode: enabled`, `--sandbox enabled` and `Shell(sleep)`, `Shell(touch)`, `Shell(echo)` allowlisted, the command ran unprompted as a plain `zsh` → `sleep` child of the pane, `cursorsandbox` never appeared in the process tree (polled every second for 60s), and `touch` outside the worktree succeeded. The bundle chooses a policy per tool call (`insecure_none`, `workspace_readonly`, `workspace_readwrite`), and the CLI falls back to `insecure_none` when none is set; combined with the earlier table — commands not on the allowlist prompt rather than running sandboxed — this account and build never sandboxed a command. Not established: why (account plan, a server-side gate, or a local setting not found).

Aside: the pane also started `mcp@latest` and `typescript-language-server`, i.e. MCP servers and a language server it found on its own.

## 2026-11-02 — the write guardrail as cursor's hook (`examples/cursor-spike/hook_probe.py`, ~16 turns)

Run with `approvalMode: allowlist`, `permissions.allow: ["Shell(*)"]` (so no shell command prompts) and the fleet's unmodified `write_guardrail.py`.

1. **Where cursor reads hooks:** `/Library/Application Support/Cursor/hooks.json`, `$HOME/.cursor/hooks.json` (through `homedir()`, not `CURSOR_CONFIG_DIR` — a `hooks.json` there never fired), `<workspace>/.cursor/hooks.json`, and Claude Code's `$HOME/.claude/settings.json`, `<workspace>/.claude/settings.json` and `<workspace>/.claude/settings.local.json`. Under D-128 a cursor worker therefore also runs the operator's own user-level hooks from both vendors.
2. **The payload is the guardrail's shape:** `tool_name` (`Shell`, `Write`, `Delete`, `Read`; an edit to an existing file arrives as `Write`), `tool_input.command` / `tool_input.file_path`, and `cwd` empty, so the script falls back to its process cwd.
3. **A deny is honoured only in Claude's spelling.** From `<workspace>/.cursor/hooks.json` with event `preToolUse`, the guardrail returned a deny (journalled) for every outside write and cursor ignored it: the shell write happened, the file-tool writes fell through to `Write to this file?`. The bundle translates `hookSpecificOutput` only when `hookEventName` is `PreToolUse`.
4. **Claude Code's install, unchanged, works.** `<workspace>/.claude/settings.local.json` holding `{"hooks": {"PreToolUse": [{"matcher": "Shell|Write|Delete|…", "hooks": [{"type": "command", "command": <the guardrail>}]}]}}` with `--event PreToolUse`:

| Arm | Result |
|---|---|
| Shell write outside the worktree | **refused**, no prompt |
| File-tool create outside | **refused**, no prompt |
| File-tool edit of an existing outside file | **refused**, no prompt |
| File-tool delete outside | **refused**, no prompt |
| Shell write inside | works |
| File-tool write inside | works |

## 2026-11-02 — seeding instead of `--trust` (zero tokens, `tui_probe.py` variant)

1. **`CURSOR_DATA_DIR` may equal `CURSOR_CONFIG_DIR`.** With both pointed at one directory the pane reaches its input box; `chats/`, `projects/`, `cli-config.json` and `statsig-cache.json` sit side by side.
2. **A pre-written `.workspace-trusted` clears the gate without `--trust`.** The first attempt keyed the directory `…claude-503--Users…` and the gate appeared; cursor had keyed it `…claude-503-Users…`. Runs of separators collapse to one `-`. With that rule the gate does not appear.
3. **The chat directory is `chats/<md5 of the canonical cwd>/<chat id>/`**, confirmed with `md5 -qs`.
4. **The pane is one process**, `~/.local/bin/cursor-agent --use-system-ca …/index.js`, so `comm` ends in `cursor-agent`.
