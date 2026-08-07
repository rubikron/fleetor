#!/usr/bin/env python3
"""The write guardrail (WP-17): a `PreToolUse` hook that refuses a tool call
which would write outside this pane's own allowlist of roots.

Installed into every pane's `CLAUDE_CONFIG_DIR` at spawn by
`src-tauri/src/guardrail.rs`, which also writes the `settings.json` entry that
invokes it. The roots arrive on argv, so this file is the same for every pane and
the *policy* is the command line — one artifact to review, one place per pane the
policy is written down.

Three rules, and they are the whole of it:

  1. **Writes only.** The hook is registered for `Bash`, `Write`, `Edit`,
     `MultiEdit` and `NotebookEdit` and for nothing else, so `Read`, `Grep`,
     `Glob` and every other read never reach this file at all. Reads stay open
     deliberately (WP-17 §Scope): a Bash command a hook allows can read anything
     internally, so read-blocking is friction wearing enforcement's clothes, and
     its allowlist has to cover every toolchain path — which wedges a pane in the
     way that looks exactly like a healthy one.
  2. **A denial names the path and the rule.** The reason string is written for
     the model that has to recover from it, the same principle as
     `prompts/delivery-contract.md`: a pane that cannot tell a guardrail from a
     broken path retries forever or reports confident nonsense.
  3. **Failing is loud, never silent.** An internal error denies the call and
     says so. A guardrail that quietly does nothing is worse than no guardrail,
     because the operator believes in it.

**What this enforces and what it merely deters.** For `Write`/`Edit`/`MultiEdit`/
`NotebookEdit` the destination is a field in the tool call, so the check is exact
and the refusal is enforcement. For `Bash` there is no such field: this scans the
command for paths in *write positions* — a redirection target, an argument to one
of the mutating commands in `MUTATORS` — and can only see what the command
actually names. `cargo build` writing into `~/.cargo`, or
`python3 -c "open(x,'w')"` with the path assembled at runtime, are invisible here
and always will be. That is deliberate: the realistic failure is an accident with
the path written out in full, and `docs/notes/write-guardrail-notes.md` measures
what the alternatives cost.
"""

import argparse
import json
import os
import shlex
import sys
import time

# --- what counts as a write ---------------------------------------------------

#: Commands whose every path-shaped argument is a thing they write.
WRITES_ALL_ARGS = {
    "touch", "mkdir", "rmdir", "rm", "unlink", "shred", "mkfifo", "truncate",
    "chmod", "chown", "chgrp", "tee",
}

#: Commands whose *last* path-shaped argument is the destination. Their earlier
#: arguments are sources — reads — and denying those would be read-blocking by
#: the back door.
WRITES_LAST_ARG = {"cp", "mv", "ln", "rsync", "install", "unzip"}

#: `sed -i` / `perl -i` edit in place; the first non-flag argument is the script.
EDITS_IN_PLACE = {"sed", "perl"}

#: `git` subcommands that only read. Anything else, run with `-C <dir>`, is
#: treated as writing into that directory — an improve run's most likely
#: accident is a `git -C <the operator's own checkout> commit`.
GIT_READ_ONLY = {
    "log", "show", "diff", "status", "blame", "describe", "grep", "shortlog",
    "whatchanged", "ls-files", "ls-tree", "ls-remote", "cat-file", "rev-parse",
    "rev-list", "merge-base", "name-rev", "for-each-ref", "symbolic-ref",
    "check-ignore", "var", "help", "version",
}

#: Redirection operators that create or extend a file. `<` is a read and is not
#: here; `>&1`-style descriptor dups are filtered out by the target check.
REDIRECTS = (">", ">>", "&>", "&>>", ">|")

#: Where one simple command ends and the next begins.
SEPARATORS = {";", "&&", "||", "|", "|&", "&", "\n"}

#: Tool inputs that name a destination directly. Everything here is exact.
PATH_FIELDS = ("file_path", "notebook_path", "path")

#: The tools this hook is registered for. Any other tool reaching it is a
#: mismatch between this file and the settings.json that invokes it, and is
#: allowed rather than guessed at — the hook must not become a deny-by-default
#: layer nobody asked for.
WRITE_TOOLS = {"Bash", "Write", "Edit", "MultiEdit", "NotebookEdit"}


# --- paths ---------------------------------------------------------------------


def resolve(path, cwd):
    """`path` as an absolute, symlink-resolved string, without requiring it to
    exist. macOS makes this load-bearing: `/tmp` is a symlink to `/private/tmp`,
    so a root and a candidate that name the same directory two ways must still
    compare equal."""
    if path.startswith("~"):
        path = os.path.expanduser(path)
    if not os.path.isabs(path):
        path = os.path.join(cwd, path)
    path = os.path.normpath(path)
    # Resolve the longest existing prefix, keep the rest verbatim: a file about
    # to be created has no realpath of its own, but its parent does.
    head, tail = path, []
    while head and head != os.sep and not os.path.exists(head):
        head, part = os.path.split(head)
        tail.append(part)
    resolved = os.path.realpath(head) if head else os.sep
    for part in reversed(tail):
        resolved = os.path.join(resolved, part)
    return os.path.normpath(resolved)


def within(path, root):
    """Is `path` inside `root` (or is it `root`)? Compared componentwise, so
    `/a/bc` is not inside `/a/b`."""
    if path == root:
        return True
    return path.startswith(root.rstrip(os.sep) + os.sep)


def looks_like_a_path(token):
    """A conservative filter for arguments that could name a file. Keeps `chmod
    755` from being read as a path and keeps `git commit -m message` quiet."""
    if not token or token.startswith("-"):
        return False
    if "=" in token.split("/")[0] and not token.startswith(("/", ".", "~")):
        return False  # an env assignment or a `key=value` flag
    return True


# --- the Bash scanner ----------------------------------------------------------


def split_commands(tokens):
    """Split a token stream into simple commands at shell separators."""
    out, current = [], []
    for token in tokens:
        if token in SEPARATORS:
            if current:
                out.append(current)
            current = []
        else:
            current.append(token)
    if current:
        out.append(current)
    return out


def redirection_targets(tokens):
    """Every file a simple command redirects into, plus the tokens consumed."""
    targets, consumed = [], set()
    for i, token in enumerate(tokens):
        stripped = token.lstrip("0123456789")
        glued = None
        if stripped in REDIRECTS:
            if i + 1 < len(tokens):
                targets.append(tokens[i + 1])
                consumed.add(i)
                consumed.add(i + 1)
            continue
        for op in (">>", ">"):
            if stripped.startswith(op) and len(stripped) > len(op):
                glued = stripped[len(op):]
                break
        if glued:
            targets.append(glued)
            consumed.add(i)
    # `2>&1` and friends name a descriptor, not a file.
    return [t for t in targets if not t.startswith("&")], consumed


def command_word(tokens):
    """The program a simple command runs, skipping leading `VAR=value` prefixes
    and `env`/`sudo`-style wrappers, with its remaining arguments."""
    i = 0
    while i < len(tokens) and "=" in tokens[i] and not tokens[i].startswith(("/", ".", "~", "-")):
        i += 1
    while i < len(tokens) and os.path.basename(tokens[i]) in ("env", "sudo", "nohup", "time", "command", "nice"):
        i += 1
        while i < len(tokens) and tokens[i].startswith("-"):
            i += 1
    if i >= len(tokens):
        return None, []
    return os.path.basename(tokens[i]), tokens[i + 1:]


def git_targets(args):
    """Where a `git` invocation writes, when it says so on the command line."""
    targets, sub, directory = [], None, None
    i = 0
    while i < len(args):
        if args[i] == "-C" and i + 1 < len(args):
            directory = args[i + 1]
            i += 2
            continue
        if args[i].startswith("-"):
            i += 1
            continue
        sub = args[i]
        rest = [a for a in args[i + 1:] if looks_like_a_path(a)]
        break
    else:
        rest = []
    if sub in ("clone", "init") and rest:
        targets.append(rest[-1] if sub == "clone" else rest[0])
    if sub == "worktree" and rest[:1] == ["add"]:
        targets.extend(rest[1:2])
    if directory and (sub is None or sub not in GIT_READ_ONLY):
        targets.append(directory)
    return targets


def bash_write_targets(command):
    """Every path a Bash command *says* it will write to."""
    try:
        lexer = shlex.shlex(command, posix=True, punctuation_chars=True)
        lexer.whitespace_split = True
        tokens = list(lexer)
    except ValueError:
        # An unbalanced quote is not something to guess about. The command would
        # fail in the shell anyway; let it, rather than denying it here.
        return []

    targets = []
    for simple in split_commands(tokens):
        redirects, consumed = redirection_targets(simple)
        targets.extend(redirects)
        rest = [t for i, t in enumerate(simple) if i not in consumed]
        name, args = command_word(rest)
        if name is None:
            continue
        paths = [a for a in args if looks_like_a_path(a)]
        if name in WRITES_ALL_ARGS:
            targets.extend(paths)
        elif name in WRITES_LAST_ARG:
            flagged = [args[i + 1] for i, a in enumerate(args) if a == "-t" and i + 1 < len(args)]
            targets.extend(flagged or paths[-1:])
        elif name in EDITS_IN_PLACE:
            if any(a == "-i" or a.startswith("-i") for a in args):
                targets.extend(paths[1:])
        elif name == "dd":
            targets.extend(a[3:] for a in args if a.startswith("of="))
        elif name == "git":
            targets.extend(git_targets(args))
    return targets


# --- the decision --------------------------------------------------------------


def offenders(tool, tool_input, cwd, roots, denied):
    """The paths this tool call would write outside the allowlist, in order."""
    if tool == "Bash":
        candidates = bash_write_targets(tool_input.get("command", "") or "")
    else:
        candidates = [tool_input[f] for f in PATH_FIELDS if isinstance(tool_input.get(f), str)]
        for edit in tool_input.get("edits", []) or []:
            if isinstance(edit, dict) and isinstance(edit.get("file_path"), str):
                candidates.append(edit["file_path"])

    out = []
    for raw in candidates:
        path = resolve(raw, cwd)
        if any(within(path, d) for d in denied):
            out.append((raw, path, "policy"))
        elif not any(within(path, r) for r in roots):
            out.append((raw, path, "outside"))
    return out


def refusal(pane, hits, roots, denied):
    """What the pane is told. Written to be recovered from, not merely obeyed."""
    lines = []
    for raw, path, why in hits:
        if why == "policy":
            lines.append(
                f"  {raw} -> {path}\n"
                f"      refused: that is fleet policy — the pane config directories hold this "
                f"guardrail's own rules, and no pane edits its own."
            )
        else:
            lines.append(f"  {raw} -> {path}\n      refused: outside every root {pane} may write to.")
    allowed = "\n".join(f"  {r}" for r in roots)
    off_limits = "".join(f"\n  {d}   (inside the roots above, and still off limits)" for d in denied)
    return (
        f"FLEETOR write guardrail: this would write outside {pane}'s own workspace.\n\n"
        + "\n".join(lines)
        + "\n\nYou may write anywhere under:\n"
        + allowed
        + off_limits
        + "\n\nReading is not restricted — only writing. Redo the write inside one of those "
        "roots, or if the work genuinely belongs outside them, say so to the operator with "
        "`fleet send operator` and stop. Retrying the same path will be refused again."
    )


def journal(path, record):
    """Append one line for the Activity feed. Best-effort: the operator losing a
    Notice must never turn into the pane losing a decision."""
    if not path:
        return
    try:
        with open(path, "a", encoding="utf-8") as handle:
            handle.write(json.dumps(record) + "\n")
    except OSError:
        pass


def deny(reason):
    return {
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        }
    }


def main():
    parser = argparse.ArgumentParser(description="FLEETOR write guardrail (WP-17)")
    parser.add_argument("--pane", required=True)
    parser.add_argument("--root", action="append", default=[])
    parser.add_argument("--deny", action="append", default=[])
    parser.add_argument("--journal", default="")
    args = parser.parse_args()

    raw = sys.stdin.read()
    try:
        event = json.loads(raw) if raw.strip() else {}
        tool = event.get("tool_name", "")
        tool_input = event.get("tool_input", {}) or {}
        cwd = event.get("cwd") or os.getcwd()

        if tool not in WRITE_TOOLS:
            return {}

        roots = [resolve(r, cwd) for r in args.root]
        denied = [resolve(d, cwd) for d in args.deny]
        hits = offenders(tool, tool_input, cwd, roots, denied)
        if not hits:
            return {}

        reason = refusal(args.pane, hits, roots, denied)
        journal(
            args.journal,
            {
                "ts": int(time.time() * 1000),
                "pane": args.pane,
                "tool": tool,
                "paths": [h[1] for h in hits],
                "level": "warn",
            },
        )
        return deny(reason)
    except Exception as error:  # noqa: BLE001 — see rule 3 in the module docstring
        journal(
            args.journal,
            {
                "ts": int(time.time() * 1000),
                "pane": args.pane,
                "tool": "guardrail",
                "paths": [],
                "level": "error",
                "detail": f"{type(error).__name__}: {error}",
            },
        )
        return deny(
            "FLEETOR write guardrail: the guardrail itself failed and refused this call rather "
            f"than letting it through unchecked ({type(error).__name__}: {error}). Tell the "
            "operator with `fleet send operator` — this is a bug in the fleet, not in your work."
        )


if __name__ == "__main__":
    json.dump(main(), sys.stdout)
    sys.stdout.write("\n")
