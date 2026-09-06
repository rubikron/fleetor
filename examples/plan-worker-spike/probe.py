#!/usr/bin/env python3
"""WP-25 (#49) spike: can a *fenced* worker run on the operator's own plan?

Issue #49 asks for a per-seat credential source: the fleet's key (today, and the
default) or the operator's plan. The orchestrator already runs on the operator's
login, so the mechanism is known — `CLAUDE_SECURESTORAGE_CONFIG_DIR` **defined and
empty** selects the unsuffixed Keychain service name, the entry the operator's own
`/login` wrote (`docs/notes/orch-config-dir-notes.md` §2).

What is *not* known, and what this probe measures, is whether that keychain read
still works from inside the Fence — a **private `HOME`** and a PATH with no
operator rung (WP-08, D-052). `orch` has never been fenced, so nothing in the
codebase has ever exercised the pair together. C17 renamed checkpoint 6 precisely
because "auth follows HOME" had been one vendor's fact treated as universal; this
probe refuses to make the mirror-image assumption in the other direction.

The second question is L2 under the Fence. Today a worker has `ANTHROPIC_API_KEY`
`env_remove`d because an operator's shell key parks an interactive pane on an
api-key approval prompt forever. A plan-backed worker adopting `orch`'s credential
posture would stop scrubbing it — so: does an inherited `ANTHROPIC_API_KEY` still
wedge a pane that has a perfectly good keychain credential?

Zero token spend by construction. Nothing is ever submitted: the probe spawns an
interactive `claude` in a pty, watches the screen, and SIGKILLs. **Reaching the
input box with `Claude Max` on the banner and no `Not logged in` is the whole
answer** — a turn would add nothing that the mode line does not already say.

Five arms:

    unfenced-plan   — orch's posture. The known-good control from WP-14. Proves the
                      keychain read works at all on this machine right now.
    fenced-plan     — the same credential posture *inside the Fence*: private HOME,
                      worker PATH, own config dir. **The arm this ticket turns on.**
    fenced-plan-key — fenced-plan plus an inherited `ANTHROPIC_API_KEY`. Answers
                      whether a plan-backed worker may stop scrubbing it (L2).
    fenced-nokeyvar — fenced-plan with `CLAUDE_SECURESTORAGE_CONFIG_DIR` *removed*
                      rather than set empty. The negative control: this is what a
                      fenced pane gets today, and it must come back logged out, or
                      the fenced-plan arm proves nothing.
    fenced-fleetkey — today's worker exactly: the fleet endpoint and token, all four
                      names scrubbed. Must NOT reach the operator's plan.

Usage:
    python3 probe.py --bin "$HOME/.local/bin/claude"
    python3 probe.py --bin "$HOME/.local/bin/claude" --arm fenced-plan --seconds 14
"""

import argparse
import errno
import json
import os
import pty
import re
import select
import shutil
import signal
import sys
import termios
import time
from pathlib import Path

# Markers are matched against a whitespace-free copy of the screen: the TUI positions
# glyphs with cursor-movement escapes, so stripping ANSI also strips inter-word spaces.
ONBOARDING_MARKERS = [
    (r"choosethetextstyle|darkmode|lightmode", "theme picker"),
    (r"trustthefiles|doyoutrust|proceedwiththefiles", "trust dialog"),
    (r"usethisapikey|detectedacustomapikey|customapikey", "api-key approval"),
    (r"letsgetstarted|let'sgetstarted|securitynotes|presstocontinue", "welcome/intro"),
    (r"termsofservice|usagepolicy", "terms"),
]
# The one that would stop #49 dead if it appeared on the `fenced-plan` arm.
LOGIN_MARKERS = [
    (r"signinwith|loginwith|invalidapikey|notloggedin|pleaserun/login|claude\.ai/login",
     "LOGIN REQUIRED"),
]
PROMPT_MARKERS = [
    (r"\?forshortcuts|/helpforhelp|tryedit<filepath>", "input box ready"),
]
# What the banner says when the *plan* is what is behind the pane. This is the
# positive signal the whole ticket rests on; `API Usage Billing` is its opposite.
PLAN_MARKERS = [
    (r"claudemax|claudepro", "PLAN CREDENTIAL LIVE"),
]
METERED_MARKERS = [
    (r"apiusagebilling", "metered/API billing"),
]


def strip_ansi(text: str) -> str:
    text = re.sub(r"\x1b\][^\x07\x1b]*(\x07|\x1b\\)", "", text)
    text = re.sub(r"\x1b[@-Z\\-_]|\x1b\[[0-?]*[ -/]*[@-~]", "", text)
    return text


def scan(plain: str, markers):
    squashed = re.sub(r"\s+", "", plain).lower()
    return [label for pattern, label in markers if re.search(pattern, squashed)]


def seed_config_dir(config_dir: Path, cwd: Path):
    """Exactly what `spawn::seed_config_dir` writes — the two keys, merged (L1)."""
    config_dir.mkdir(parents=True, exist_ok=True)
    path = config_dir / ".claude.json"
    doc = json.loads(path.read_text()) if path.is_file() else {}
    doc["hasCompletedOnboarding"] = True
    entry = doc.setdefault("projects", {}).setdefault(str(cwd), {})
    entry["hasTrustDialogAccepted"] = True
    entry["hasCompletedProjectOnboarding"] = True
    path.write_text(json.dumps(doc, indent=2))


def seed_worker_home(home: Path):
    """Exactly what `spawn::seed_worker_home` writes — one `.gitconfig` (D-052)."""
    home.mkdir(parents=True, exist_ok=True)
    (home / ".gitconfig").write_text(
        "[user]\n\tname = fleet worker-1\n\temail = worker-1@fleetor.local\n"
    )


def worker_path(existing: str) -> str:
    """`spawn::worker_augmented_path_from` with no fleet bin and no cargo home.

    The rung that matters is the one that is *absent*: nothing from the operator's
    `HOME` is on it, so a `claude` resolved through this PATH is the fleet's idea
    of the binary rather than the operator's shell's.
    """
    return f"/opt/homebrew/bin:/usr/local/bin:{existing}"


def base_env() -> dict:
    """The parent-environment inherit both builders start from, minus this session's
    own Claude Code bleed-through."""
    env = dict(os.environ)
    env["TERM"] = "xterm-256color"
    env["COLORTERM"] = "truecolor"
    # Leaks through the inherit and silently disables transcript saving.
    env.pop("CLAUDE_CODE_CHILD_SESSION", None)
    # This probe runs *inside* a Claude Code session; none of its wiring may reach
    # the child or the arms measure the parent rather than the pane.
    for name in list(env):
        if name.startswith("CLAUDE_CODE_") or name.startswith("CMUX_"):
            env.pop(name, None)
    env.pop("CLAUDE_SECURESTORAGE_CONFIG_DIR", None)
    env.pop("CLAUDE_CONFIG_DIR", None)
    env.pop("ANTHROPIC_API_KEY", None)
    return env


def arm_env(name: str, config_dir: Path, home: Path) -> dict:
    env = base_env()
    env["FLEETOR_PANE"] = "orch" if name == "unfenced-plan" else "worker-1"
    env["CLAUDE_CONFIG_DIR"] = str(config_dir)

    if name == "unfenced-plan":
        # `spawn::orch_command_with`: full inherit, the operator's own HOME and
        # PATH, the config dir, and the credential namespace. No Fence.
        env["CLAUDE_SECURESTORAGE_CONFIG_DIR"] = ""
        return env

    # Every remaining arm is fenced: `spawn::worker_command_with`'s private HOME
    # and its PATH with no operator rung.
    env["HOME"] = str(home)
    env["PATH"] = worker_path(env.get("PATH", ""))

    if name == "fenced-plan":
        env["CLAUDE_SECURESTORAGE_CONFIG_DIR"] = ""
    elif name == "fenced-plan-key":
        env["CLAUDE_SECURESTORAGE_CONFIG_DIR"] = ""
        # Not a real key. L2 fires on the *presence* of the variable — the pane
        # asks before it ever calls anything, which is why a bogus value is a
        # sufficient and zero-cost stimulus.
        env["ANTHROPIC_API_KEY"] = "sk-ant-probe-not-a-real-key"
    elif name == "fenced-nokeyvar":
        # The negative control: the variable absent, which is what today's scrub
        # leaves behind. CC then hashes CLAUDE_CONFIG_DIR into the service name and
        # finds an empty namespace.
        env.pop("CLAUDE_SECURESTORAGE_CONFIG_DIR", None)
    elif name == "fenced-fleetkey":
        env.pop("CLAUDE_SECURESTORAGE_CONFIG_DIR", None)
        env["ANTHROPIC_BASE_URL"] = "https://api.deepseek.com/anthropic"
        env["ANTHROPIC_AUTH_TOKEN"] = "sk-probe-not-a-real-key"
    else:
        raise SystemExit(f"unknown arm {name}")
    return env


def spawn_and_watch(cwd: Path, env, seconds: float, rows: int, cols: int, binary: str) -> bytes:
    pid, fd = pty.fork()
    if pid == 0:
        os.chdir(str(cwd))
        os.execve(binary, [binary], env)
        os._exit(127)

    try:
        import fcntl
        import struct

        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
    except Exception:
        pass

    chunks = []
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        try:
            ready, _, _ = select.select([fd], [], [], 0.1)
        except select.error:
            break
        if not ready:
            continue
        try:
            data = os.read(fd, 65536)
        except OSError as exc:
            if exc.errno == errno.EIO:
                break
            raise
        if not data:
            break
        chunks.append(data)

    os.kill(pid, signal.SIGKILL)
    os.waitpid(pid, 0)
    os.close(fd)
    return b"".join(chunks)


def run_arm(name: str, cwd: Path, work: Path, args) -> dict:
    config_dir = work / f"pane-config-{name}"
    if config_dir.exists():
        shutil.rmtree(config_dir)
    config_dir.mkdir(parents=True)
    seed_config_dir(config_dir, cwd)

    home = work / f"home-{name}"
    if home.exists():
        shutil.rmtree(home)
    seed_worker_home(home)

    env = arm_env(name, config_dir, home)
    raw = spawn_and_watch(cwd, env, args.seconds, args.rows, args.cols, args.bin)
    plain = strip_ansi(raw.decode("utf-8", errors="replace"))
    (work / f"plain-{name}.log").write_text(plain)

    banner = [l.strip() for l in plain.splitlines() if "Claude Code v" in l or "·" in l]
    return {
        "arm": name,
        "home": env.get("HOME", "<operator's own>"),
        "securestorage": (
            "<absent>" if "CLAUDE_SECURESTORAGE_CONFIG_DIR" not in env else "defined, empty"
        ),
        "bytes": len(raw),
        "onboarding": scan(plain, ONBOARDING_MARKERS) or ["none"],
        "login": scan(plain, LOGIN_MARKERS) or ["none"],
        "plan": scan(plain, PLAN_MARKERS) or ["none"],
        "metered": scan(plain, METERED_MARKERS) or ["none"],
        "prompt": scan(plain, PROMPT_MARKERS) or ["NOT REACHED"],
        "banner": banner[:5],
    }


ARMS = ["unfenced-plan", "fenced-plan", "fenced-plan-key", "fenced-nokeyvar", "fenced-fleetkey"]


def main():
    work = Path(__file__).resolve().parent / "work"
    work.mkdir(parents=True, exist_ok=True)

    p = argparse.ArgumentParser()
    p.add_argument("--cwd", type=Path, default=Path.home() / ".fleetor" / "testbed")
    p.add_argument("--arm", default="all", choices=["all", *ARMS])
    # The `claude` on PATH may be a wrapper (cmux ships one, and C47 is the whole
    # reason this is an argument). Point it at the real binary.
    p.add_argument("--bin", default=str(Path.home() / ".local" / "bin" / "claude"))
    p.add_argument("--seconds", type=float, default=12.0)
    p.add_argument("--rows", type=int, default=40)
    p.add_argument("--cols", type=int, default=120)
    args = p.parse_args()

    cwd = args.cwd.expanduser()
    cwd.mkdir(parents=True, exist_ok=True)
    cwd = cwd.resolve()
    args.bin = str(Path(args.bin).expanduser().resolve())

    for name in (ARMS if args.arm == "all" else [args.arm]):
        print(f"\n[probe] arm={name} cwd={cwd} bin={args.bin}", file=sys.stderr)
        r = run_arm(name, cwd, work, args)
        print("=" * 72)
        print(f"arm            : {r['arm']}")
        print(f"HOME           : {r['home']}")
        print(f"securestorage  : {r['securestorage']}")
        print(f"bytes          : {r['bytes']}")
        print(f"prompt         : {r['prompt']}")
        print(f"onboarding     : {r['onboarding']}")
        print(f"login          : {r['login']}")
        print(f"plan           : {r['plan']}")
        print(f"metered        : {r['metered']}")
        print("banner         :")
        for line in r["banner"]:
            print(f"    {line}")
        print("=" * 72)


if __name__ == "__main__":
    main()
