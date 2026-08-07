#!/usr/bin/env python3
"""WP-14 spike: can `orch` run on a fleet-owned CLAUDE_CONFIG_DIR and keep its login?

WP-14 wants `orch`'s session transcript inside `~/.fleetor` so rotation can archive
it like a worker's. The only lever that moves it is `CLAUDE_CONFIG_DIR`, and orch is
the operator's *own* `claude` — so the question this answers before a line of Rust is
written is:

  1. Does an interactive `claude` on a fleet-owned config dir still reach a prompt,
     or does it land on a login screen? (If it logs out, WP-14 is a Tier 1 conflict
     and stops here.)
  2. Is `seed_config_dir`'s two-key seed load-bearing for orch the way it is for a
     worker (L1), or does orch's own auth carry it past onboarding?
  3. What lands *in* the new config dir — specifically, does `projects/<slug>/` appear,
     which is where the transcript rotation will harvest lives.

Zero token spend by construction: nothing is ever submitted to the pane. The probe
spawns, watches the screen for a few seconds, and SIGKILLs. Reaching the input box is
the whole answer; a turn would add nothing.

Three arms, run in one go:

    control  — no CLAUDE_CONFIG_DIR override. Orch exactly as it spawns today.
    virgin   — a fresh fleet-owned config dir, unseeded.
    seeded   — a fresh fleet-owned config dir with the two keys `seed_config_dir` writes.

Usage:
    python3 probe.py                      # all three arms
    python3 probe.py --arm seeded --seconds 12
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
# The one that would stop WP-14 dead.
LOGIN_MARKERS = [
    (r"signinwith|loginwith|invalidapikey|pleaserun/login|claude\.ai/login", "LOGIN REQUIRED"),
]
PROMPT_MARKERS = [
    (r"\?forshortcuts|/helpforhelp|tryedit<filepath>", "input box ready"),
]


def strip_ansi(text: str) -> str:
    text = re.sub(r"\x1b\][^\x07\x1b]*(\x07|\x1b\\)", "", text)
    text = re.sub(r"\x1b[@-Z\\-_]|\x1b\[[0-?]*[ -/]*[@-~]", "", text)
    return text


def scan(plain: str, markers):
    squashed = re.sub(r"\s+", "", plain).lower()
    return [label for pattern, label in markers if re.search(pattern, squashed)]


def seed(config_dir: Path, cwd: Path):
    """Exactly what `spawn::seed_config_dir` writes — the two keys, merged."""
    config_dir.mkdir(parents=True, exist_ok=True)
    path = config_dir / ".claude.json"
    doc = json.loads(path.read_text()) if path.is_file() else {}
    doc["hasCompletedOnboarding"] = True
    entry = doc.setdefault("projects", {}).setdefault(str(cwd), {})
    entry["hasTrustDialogAccepted"] = True
    entry["hasCompletedProjectOnboarding"] = True
    path.write_text(json.dumps(doc, indent=2))


def orch_env(config_dir, securestorage: bool = False):
    """The orch posture from `spawn.rs`: full inherit, then the fleet's overrides."""
    env = dict(os.environ)
    env["TERM"] = "xterm-256color"
    env["COLORTERM"] = "truecolor"
    env["FLEETOR_PANE"] = "orch"
    # Leaks through the environment inherit and silently disables transcript saving.
    env.pop("CLAUDE_CODE_CHILD_SESSION", None)
    env.pop("CLAUDE_SECURESTORAGE_CONFIG_DIR", None)
    if config_dir is not None:
        env["CLAUDE_CONFIG_DIR"] = str(config_dir)
    else:
        # The control arm must be the operator's real dir, not this session's.
        env.pop("CLAUDE_CONFIG_DIR", None)
    if securestorage:
        # Defined-but-empty. CC's keychain service name is
        # `Claude Code<suffix>-<sha256(config dir)[..8]>` whenever CLAUDE_CONFIG_DIR
        # is set; defining this var empty selects the *unsuffixed* service name —
        # the entry the operator's own `/login` already wrote.
        env["CLAUDE_SECURESTORAGE_CONFIG_DIR"] = ""
    return env


def spawn_and_watch(cwd: Path, env, seconds: float, rows: int, cols: int, binary: str) -> bytes:
    pid, fd = pty.fork()
    if pid == 0:
        os.chdir(str(cwd))
        os.execvpe(binary, [binary], env)
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


def tree(root: Path, limit: int = 40):
    if not root.exists():
        return ["<nothing was created>"]
    out = []
    for path in sorted(root.rglob("*")):
        rel = path.relative_to(root)
        out.append(f"{rel}/" if path.is_dir() else f"{rel}  ({path.stat().st_size}B)")
        if len(out) >= limit:
            out.append("…")
            break
    return out


def run_arm(name: str, cwd: Path, work: Path, args) -> dict:
    config_dir = None
    if name != "control":
        config_dir = work / f"pane-config-{name}" / "orch"
        if config_dir.exists():
            shutil.rmtree(config_dir)
        config_dir.mkdir(parents=True)
        if name != "virgin":
            seed(config_dir, cwd)

    env = orch_env(config_dir, securestorage=(name == "securestorage"))
    raw = spawn_and_watch(cwd, env, args.seconds, args.rows, args.cols, args.bin)
    plain = strip_ansi(raw.decode("utf-8", errors="replace"))
    (work / f"plain-{name}.log").write_text(plain)

    banner = [l.strip() for l in plain.splitlines() if "Claude Code v" in l or "·" in l]
    return {
        "arm": name,
        "config_dir": str(config_dir) if config_dir else "<operator's own>",
        "bytes": len(raw),
        "onboarding": scan(plain, ONBOARDING_MARKERS) or ["none"],
        "login": scan(plain, LOGIN_MARKERS) or ["none"],
        "prompt": scan(plain, PROMPT_MARKERS) or ["NOT REACHED"],
        "banner": banner[:4],
        "dir_after": tree(config_dir) if config_dir else ["n/a"],
    }


def transcript_arm(cwd: Path, work: Path, args):
    """The one arm that costs money: does the transcript land in the fleet's dir?

    `projects/<slug>/<uuid>.jsonl` is written on the first *turn*, not at session
    start — the interactive arms above leave no `projects/` at all — so proving it
    needs one real turn. Kept as cheap as a turn can be: print mode, the smallest
    model, one word in and one word out.
    """
    import subprocess

    config_dir = work / "pane-config-transcript" / "orch"
    if config_dir.exists():
        shutil.rmtree(config_dir)
    config_dir.mkdir(parents=True)
    seed(config_dir, cwd)

    env = orch_env(config_dir, securestorage=True)
    out = subprocess.run(
        [args.bin, "-p", "reply with the single word: ok", "--model", args.cheap_model],
        cwd=str(cwd),
        env=env,
        capture_output=True,
        text=True,
        timeout=180,
    )
    return {
        "arm": "transcript",
        "config_dir": str(config_dir),
        "bytes": len(out.stdout),
        "login": ["none"] if out.returncode == 0 else [f"exit {out.returncode}"],
        "onboarding": ["n/a"],
        "prompt": [out.stdout.strip()[:60] or out.stderr.strip()[:60]],
        "banner": [],
        "dir_after": tree(config_dir / "projects"),
    }


def main():
    work = Path(__file__).resolve().parent / "work"
    work.mkdir(parents=True, exist_ok=True)

    p = argparse.ArgumentParser()
    p.add_argument("--cwd", type=Path, default=Path.home() / ".fleetor" / "testbed")
    p.add_argument(
        "--arm",
        default="all",
        choices=["all", "control", "virgin", "seeded", "securestorage", "transcript"],
    )
    p.add_argument("--cheap-model", default="haiku", help="the transcript arm's model")
    # The `claude` on PATH may be a wrapper (cmux ships one). Point this at the real
    # binary — `augmented_path()` puts `$HOME/.local/bin` early, so that is what a
    # FLEETOR pane resolves.
    p.add_argument("--bin", default="claude")
    p.add_argument("--seconds", type=float, default=10.0)
    p.add_argument("--rows", type=int, default=40)
    p.add_argument("--cols", type=int, default=120)
    args = p.parse_args()

    cwd = args.cwd.expanduser()
    cwd.mkdir(parents=True, exist_ok=True)
    cwd = cwd.resolve()

    arms = (
        ["control", "virgin", "seeded", "securestorage"] if args.arm == "all" else [args.arm]
    )
    for name in arms:
        print(f"\n[probe] arm={name} cwd={cwd}", file=sys.stderr)
        result = transcript_arm(cwd, work, args) if name == "transcript" else run_arm(name, cwd, work, args)
        print("=" * 72)
        print(f"arm         : {result['arm']}")
        print(f"config dir  : {result['config_dir']}")
        print(f"bytes       : {result['bytes']}")
        print(f"login       : {result['login']}")
        print(f"onboarding  : {result['onboarding']}")
        print(f"prompt      : {result['prompt']}")
        print("banner      :")
        for line in result["banner"]:
            print(f"    {line}")
        print("config dir after the run:")
        for line in result["dir_after"]:
            print(f"    {line}")
        print("=" * 72)


if __name__ == "__main__":
    main()
