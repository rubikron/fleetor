#!/usr/bin/env python3
"""Arm 2 of the WP-17 spike: does the scanner say the right thing about the
commands a real pane actually runs?

Free — it drives `src-tauri/src/write_guardrail.py` directly, with no `claude`
and no tokens. Arm 1 (`probe.py`) is the one that proves Claude Code honours the
decision at all.

    python3 examples/write-guardrail-spike/cases.py

Every case is a real command taken from the fleet's own flows (`fleet done`'s
`git commit`, a peer's three-dot diff, `cargo build`, `npx tsc`) or a failure the
guardrail exists to catch. `allow`/`deny` is what the scanner *should* say.
"""

import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
HOOK = os.path.join(ROOT, "src-tauri", "src", "write_guardrail.py")

WORKTREE = "/wt"
SHELL = "/shell"
CONFIG = "/shell/pane-config"

CASES = [
    # --- what a worker legitimately does, and must keep doing -----------------
    ("allow", "Bash", "cargo build --workspace"),
    ("allow", "Bash", "cargo test --workspace 2>&1 | tail -20"),
    ("allow", "Bash", "npx tsc --noEmit && npx vite build"),
    ("allow", "Bash", "npm ci"),
    ("allow", "Bash", "git add parser.rs"),
    ("allow", "Bash", "git commit -m 'worker-1: a parser nobody else has seen'"),
    ("allow", "Bash", "git diff HEAD...fleet/worker-1"),
    ("allow", "Bash", "git -C /wt commit -m x"),
    ("allow", "Bash", "git log --oneline | head -20"),
    ("allow", "Bash", "fleet send orch the block is done"),
    ("allow", "Bash", "fleet done T-4 cargo test --workspace"),
    ("allow", "Bash", "touch newfile.rs"),
    ("allow", "Bash", "mkdir -p src/parser"),
    ("allow", "Bash", "echo hi > out.txt"),
    ("allow", "Bash", "cargo build 2>&1 | tee /wt/build.log"),
    ("allow", "Bash", "chmod 755 ./script.sh"),
    ("allow", "Bash", "rm -rf target/debug/incremental"),
    ("allow", "Bash", "cp /usr/share/dict/words /wt/words"),
    ("allow", "Bash", "sed -i '' 's/a/b/' src/lib.rs"),
    ("allow", "Bash", "grep -rn TODO /Users/op/other-repo"),
    ("allow", "Bash", "cat /etc/hosts"),
    ("allow", "Bash", "ls -la /Users/op/.ssh"),
    ("allow", "Bash", "cargo build > /wt/log 2>&1"),
    ("allow", "Write", {"file_path": "/wt/src/lib.rs", "content": "x"}),
    ("allow", "Edit", {"file_path": "src/lib.rs", "old_string": "a", "new_string": "b"}),
    ("allow", "Bash", "python3 -c \"print(open('/etc/hosts').read())\""),
    # --- the accidents the guardrail exists to catch --------------------------
    ("deny", "Bash", "touch /Users/op/scratch"),
    ("deny", "Bash", "echo notes > /Users/op/notes.txt"),
    ("deny", "Bash", "echo notes >> /Users/op/notes.txt"),
    ("deny", "Bash", "cp /wt/patch.diff /Users/op/patch.diff"),
    ("deny", "Bash", "mv /wt/a /Users/op/a"),
    ("deny", "Bash", "rm -rf /Users/op/harness/fleetor/target"),
    ("deny", "Bash", "mkdir -p /Users/op/newthing"),
    ("deny", "Bash", "git -C /Users/op/harness/fleetor commit -am wip"),
    ("deny", "Bash", "git clone https://example.com/x.git /Users/op/x"),
    ("deny", "Bash", "sed -i '' 's/a/b/' /Users/op/harness/fleetor/building.md"),
    ("deny", "Bash", "dd if=/dev/zero of=/Users/op/big"),
    ("deny", "Bash", "touch ../../escaped"),
    ("deny", "Bash", "cd /wt && touch /Users/op/x"),
    ("deny", "Bash", "cargo build && echo done > /Users/op/done"),
    ("deny", "Write", {"file_path": "/Users/op/harness/fleetor/CLAUDE.md", "content": "x"}),
    ("deny", "Edit", {"file_path": "../../../escaped.rs", "old_string": "a", "new_string": "b"}),
    ("deny", "NotebookEdit", {"notebook_path": "/Users/op/x.ipynb"}),
    # --- inside the roots, and still refused ---------------------------------
    ("deny", "Edit", {"file_path": "/shell/pane-config/worker-1/settings.json"}),
    ("deny", "Bash", "rm /shell/pane-config/worker-1/settings.json"),
    ("allow", "Bash", "ls /shell/pane-config/worker-1"),
    ("allow", "Bash", "touch /shell/worktrees/worker-2/note"),
    # --- reads never reach the hook at all -----------------------------------
    ("allow", "Read", {"file_path": "/Users/op/.ssh/id_ed25519"}),
    ("allow", "Grep", {"pattern": "x", "path": "/Users/op"}),
]


def ask(tool, tool_input):
    payload = {
        "hook_event_name": "PreToolUse",
        "cwd": WORKTREE,
        "tool_name": tool,
        "tool_input": {"command": tool_input} if isinstance(tool_input, str) else tool_input,
    }
    out = subprocess.run(
        [
            sys.executable, HOOK,
            "--pane", "worker-1",
            "--root", WORKTREE,
            "--root", SHELL,
            "--deny", CONFIG,
        ],
        input=json.dumps(payload),
        capture_output=True,
        text=True,
        check=True,
    )
    answer = json.loads(out.stdout or "{}")
    decision = answer.get("hookSpecificOutput", {}).get("permissionDecision", "")
    return ("deny" if decision == "deny" else "allow"), answer


def main():
    failures = 0
    for expected, tool, tool_input in CASES:
        got, answer = ask(tool, tool_input)
        mark = "ok  " if got == expected else "FAIL"
        if got != expected:
            failures += 1
        shown = tool_input if isinstance(tool_input, str) else json.dumps(tool_input)
        print(f"{mark} {expected:5} {tool:12} {shown}")
        if got != expected:
            print(json.dumps(answer, indent=2))
    print(f"\n{len(CASES) - failures}/{len(CASES)} cases behave as specified")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
