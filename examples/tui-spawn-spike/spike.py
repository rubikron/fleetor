#!/usr/bin/env python3
"""Phase-0 spike: spawn a real `claude` TUI in a pty the way FLEETOR will.

Answers the four Phase-0 unknowns without touching the app:

  1. Does an interactive `claude` on a virgin CLAUDE_CONFIG_DIR hit onboarding?
     (Headless `-p` never needed the onboarding keys, so the worker config dirs
     on disk lack them — L1.)
  2. What is the minimum `.claude.json` seed that lands at a usable prompt?
  3. Does bracketed paste + a delayed `\\r` submit a turn, and what is the
     smallest gap that works?
  4. Does `--permission-mode auto` ever surface a prompt for Read/Write/Edit/Bash?

Everything is written to a log so the raw bytes can be inspected afterwards.
Run outputs live under `work/` and are gitignored.

Usage:
    python3 spike.py --cwd ~/.fleetor/testbed --seconds 6
    python3 spike.py --cwd ~/.fleetor/testbed --worker --send "run ./run-tests.sh"
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
import tty
from pathlib import Path

# ---- what the app will set ---------------------------------------------------

DEEPSEEK_BASE_URL = "https://api.deepseek.com/anthropic"
MODEL_FLASH = "deepseek-v4-flash"

# Candidate onboarding keys. The spike's job is to find which are load-bearing.
SEED_CANDIDATES = {
    "hasCompletedOnboarding": True,
    "theme": "dark",
    "hasTrustDialogAccepted": True,
    "bypassPermissionsModeAccepted": True,
}

# NOTE: the TUI positions individual glyphs with cursor-movement escapes, so once
# ANSI is stripped the inter-word spaces are gone too ("Choosethetextstyle").
# Every marker is therefore matched against a whitespace-free copy of the output,
# and every pattern below is written without spaces.
ONBOARDING_MARKERS = [
    (r"choosethetextstyle|darkmode|lightmode", "theme picker"),
    (r"trustthefiles|doyoutrust|proceedwiththefiles", "trust dialog"),
    (r"usethisapikey|detectedacustomapikey|customapikey", "api-key approval"),
    (r"signinwith|invalidapikey|pleaserun/login", "login"),
    (r"letsgetstarted|let'sgetstarted|securitynotes|presstocontinue", "welcome/intro"),
    (r"termsofservice|usagepolicy", "terms"),
]
PROMPT_MARKERS = [
    (r"\?forshortcuts|/helpforhelp|tryedit<filepath>", "input box ready"),
]
PERMISSION_MARKERS = [
    (r"doyouwantto(make|create|proceed|allow|run)", "permission prompt"),
    (r"1\.yes", "permission menu"),
]


def read_env_file(start: Path, key: str):
    """Walk up from `start` looking for a .env carrying `key`."""
    for directory in [start, *start.parents]:
        candidate = directory / ".env"
        if not candidate.is_file():
            continue
        for line in candidate.read_text(encoding="utf-8", errors="replace").splitlines():
            if line.strip().startswith(f"{key}="):
                value = line.split("=", 1)[1].strip().strip('"').strip("'")
                if value:
                    return value
    return None


def build_env(args, repo_root: Path):
    """The env the app will hand the child. Worker = isolated Flash; orch = inherit."""
    env = dict(os.environ)
    env["TERM"] = "xterm-256color"
    env["COLORTERM"] = "truecolor"
    env["FLEETOR_PANE"] = args.pane

    if not args.worker:
        return env  # orch: the operator's own environment, untouched

    api_key = os.environ.get("DEEPSEEK_API_KEY") or read_env_file(repo_root, "DEEPSEEK_API_KEY")
    if not api_key:
        sys.exit("DEEPSEEK_API_KEY not found in env or any .env — needed for --worker")

    env["CLAUDE_CONFIG_DIR"] = str(args.config_dir)
    env["ANTHROPIC_BASE_URL"] = DEEPSEEK_BASE_URL
    env["ANTHROPIC_AUTH_TOKEN"] = api_key
    env["ANTHROPIC_MODEL"] = MODEL_FLASH
    env["CLAUDE_CODE_EFFORT_LEVEL"] = "max"
    # Deliberately NOT set: ANTHROPIC_API_KEY. Interactive CC prompts for approval
    # on it where headless did not (L2). Proving that is part of this spike.
    if args.with_api_key:
        env["ANTHROPIC_API_KEY"] = api_key
    for stale in ("ANTHROPIC_DEFAULT_OPUS_MODEL", "ANTHROPIC_DEFAULT_SONNET_MODEL"):
        env.pop(stale, None)
    return env


def seed_config_dir(config_dir: Path, cwd: Path, keys):
    """Write the candidate `.claude.json` seed. `keys` selects which to include."""
    config_dir.mkdir(parents=True, exist_ok=True)
    path = config_dir / ".claude.json"
    doc = json.loads(path.read_text()) if path.is_file() else {}

    for key in keys:
        if key == "hasTrustDialogAccepted":
            projects = doc.setdefault("projects", {})
            entry = projects.setdefault(str(cwd), {})
            entry["hasTrustDialogAccepted"] = True
            entry["hasCompletedProjectOnboarding"] = True
        else:
            doc[key] = SEED_CANDIDATES[key]

    path.write_text(json.dumps(doc, indent=2))
    return path


def strip_ansi(text: str) -> str:
    text = re.sub(r"\x1b\][^\x07\x1b]*(\x07|\x1b\\)", "", text)
    text = re.sub(r"\x1b[@-Z\\-_]|\x1b\[[0-?]*[ -/]*[@-~]", "", text)
    return text


def scan(plain: str, markers):
    """Match against a whitespace-free lowercase copy — see the note on the markers."""
    squashed = re.sub(r"\s+", "", plain).lower()
    return [label for pattern, label in markers if re.search(pattern, squashed)]


def run(args, env, argv):
    """Spawn `claude` under a pty, pump output, optionally send a paste+CR."""
    pid, fd = pty.fork()
    if pid == 0:
        os.chdir(str(args.cwd))
        os.execvpe(argv[0], argv, env)
        os._exit(127)

    # A realistic window — CC lays out differently on a narrow one.
    try:
        import fcntl
        import struct

        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", args.rows, args.cols, 0, 0))
    except Exception:
        pass

    chunks = []
    chunk_count = 0
    sent_at = None
    started = time.monotonic()
    deadline = started + args.seconds

    while time.monotonic() < deadline:
        if args.send and sent_at is None and time.monotonic() - started >= args.send_after:
            body = args.send.encode()
            os.write(fd, b"\x1b[200~" + body + b"\x1b[201~")
            time.sleep(args.gap_ms / 1000.0)
            os.write(fd, b"\r")
            sent_at = time.monotonic()
            print(f"[spike] pasted {len(body)}B, CR after {args.gap_ms}ms", file=sys.stderr)

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
        chunk_count += 1

    os.kill(pid, signal.SIGKILL)
    os.waitpid(pid, 0)
    os.close(fd)

    elapsed = time.monotonic() - started
    return b"".join(chunks), chunk_count, elapsed, sent_at is not None


def main():
    repo_root = Path(__file__).resolve().parents[2]
    work = Path(__file__).resolve().parent / "work"

    p = argparse.ArgumentParser()
    p.add_argument("--cwd", type=Path, default=Path.home() / ".fleetor" / "testbed")
    p.add_argument("--config-dir", type=Path, default=work / "cc-config")
    p.add_argument("--worker", action="store_true", help="isolated DeepSeek Flash posture")
    p.add_argument("--pane", default="worker-1")
    p.add_argument("--permission-mode", default=None)
    p.add_argument("--append-system-prompt", default=None)
    p.add_argument("--with-api-key", action="store_true", help="also set ANTHROPIC_API_KEY (L2 test)")
    p.add_argument("--seed", default="", help="comma-separated seed keys, or 'all'")
    p.add_argument("--fresh", action="store_true", help="wipe the config dir first")
    p.add_argument("--send", default=None, help="text to bracketed-paste in")
    p.add_argument("--send-after", type=float, default=4.0)
    p.add_argument("--gap-ms", type=int, default=30)
    p.add_argument("--seconds", type=float, default=8.0)
    p.add_argument("--rows", type=int, default=40)
    p.add_argument("--cols", type=int, default=120)
    p.add_argument("--tail", type=int, default=40, help="lines of cleaned output to print")
    args = p.parse_args()

    args.cwd = args.cwd.expanduser().resolve()
    args.config_dir = args.config_dir.expanduser()
    work.mkdir(parents=True, exist_ok=True)

    if args.fresh and args.config_dir.exists():
        shutil.rmtree(args.config_dir)

    keys = []
    if args.seed == "all":
        keys = list(SEED_CANDIDATES)
    elif args.seed:
        keys = [k.strip() for k in args.seed.split(",") if k.strip()]
    if keys:
        seeded = seed_config_dir(args.config_dir, args.cwd, keys)
        print(f"[spike] seeded {seeded} with {keys}", file=sys.stderr)

    argv = ["claude"]
    if args.permission_mode:
        argv += ["--permission-mode", args.permission_mode]
    if args.append_system_prompt:
        argv += ["--append-system-prompt", args.append_system_prompt]

    env = build_env(args, repo_root)
    print(f"[spike] {' '.join(argv)}  cwd={args.cwd}  worker={args.worker}", file=sys.stderr)

    raw, chunk_count, elapsed, did_send = run(args, env, argv)

    stamp = time.strftime("%H%M%S")
    raw_path = work / f"raw-{stamp}.log"
    raw_path.write_bytes(raw)
    plain = strip_ansi(raw.decode("utf-8", errors="replace"))
    plain_path = work / f"plain-{stamp}.log"
    plain_path.write_text(plain)

    onboarding = scan(plain, ONBOARDING_MARKERS)
    prompt = scan(plain, PROMPT_MARKERS)
    permission = scan(plain, PERMISSION_MARKERS)

    print("\n" + "=" * 70)
    print(f"bytes {len(raw)}  chunks {chunk_count}  in {elapsed:.1f}s"
          f"  ({chunk_count / max(elapsed, 0.01):.0f} reads/sec)")
    print(f"onboarding : {onboarding or 'none detected'}")
    print(f"prompt     : {prompt or 'NOT REACHED'}")
    print(f"permission : {permission or 'none'}")
    if did_send:
        print("sent       : bracketed paste + CR")
    print(f"logs       : {raw_path.name}, {plain_path.name}")
    print("=" * 70)

    lines = [l for l in plain.splitlines() if l.strip()]
    print("\n".join(lines[-args.tail:]))


if __name__ == "__main__":
    main()
