#!/usr/bin/env python3
"""WP-04 spike: where a worker's own transcript lives, and what is in it.

The sibling of `examples/system-prompt-spike/probe.py` and
`examples/command-channel-spike/probe.py` — same shape, one live worker-posture
`claude` in a pty under an isolated `CLAUDE_CONFIG_DIR`, DeepSeek Flash — but
this one is not measuring the pty's byte stream. It is measuring what `claude`
*writes to disk* while that session runs: the question is whether
`fleetor-core`/`src-tauri` can read a live worker's own transcript to answer
"how full is its context window" without touching the message path at all.

Usage:
    python3 probe.py --turns 2 --seconds 60

Run outputs (the isolated `CLAUDE_CONFIG_DIR`, the target cwd, and a dump of
what was found) land in `work/` and are gitignored — same as the other spikes.
"""

import argparse
import json
import os
import pty
import re
import select
import shutil
import signal
import subprocess
import sys
import termios
import time
from pathlib import Path

DEEPSEEK_BASE_URL = "https://api.deepseek.com/anthropic"
MODEL_FLASH = "deepseek-v4-flash"


def read_env_file(start: Path, key: str):
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


def seed_config_dir(config_dir: Path, cwd: Path):
    """The exact two keys `src-tauri/src/spawn.rs::seed_config_dir` writes."""
    config_dir.mkdir(parents=True, exist_ok=True)
    path = config_dir / ".claude.json"
    doc = json.loads(path.read_text()) if path.is_file() else {}
    doc["hasCompletedOnboarding"] = True
    entry = doc.setdefault("projects", {}).setdefault(str(cwd), {})
    entry["hasTrustDialogAccepted"] = True
    entry["hasCompletedProjectOnboarding"] = True
    path.write_text(json.dumps(doc, indent=2))


def build_fixture(cwd: Path):
    if cwd.exists():
        shutil.rmtree(cwd)
    cwd.mkdir(parents=True)
    (cwd / "already.txt").write_text("committed\n")
    subprocess.run(["git", "init", "-q"], cwd=cwd, check=True)
    subprocess.run(["git", "add", "-A"], cwd=cwd, check=True)
    subprocess.run(
        ["git", "-c", "user.email=s@p", "-c", "user.name=spike", "commit", "-qm", "fixture"],
        cwd=cwd,
        check=True,
    )


def build_env(config_dir: Path, base_url: str, token: str, model: str) -> dict:
    """Exactly the worker posture from `src-tauri/src/spawn.rs::worker_command`."""
    env = dict(os.environ)
    env["TERM"] = "xterm-256color"
    env["COLORTERM"] = "truecolor"
    env["FLEETOR_PANE"] = "worker-1"
    env["CLAUDE_CONFIG_DIR"] = str(config_dir)
    env["ANTHROPIC_BASE_URL"] = base_url
    env["ANTHROPIC_AUTH_TOKEN"] = token
    env["ANTHROPIC_MODEL"] = model
    env.pop("ANTHROPIC_API_KEY", None)
    env.pop("ANTHROPIC_DEFAULT_OPUS_MODEL", None)
    env.pop("ANTHROPIC_DEFAULT_SONNET_MODEL", None)
    env.pop("CLAUDE_CODE_CHILD_SESSION", None)
    return env


def run(cwd: Path, env: dict, argv: list, sends: list, seconds: float, rows=40, cols=120):
    pid, fd = pty.fork()
    if pid == 0:
        os.chdir(str(cwd))
        os.execvpe(argv[0], argv, env)
        os._exit(127)

    try:
        import fcntl
        import struct

        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
    except Exception:
        pass

    chunks = []
    pending = list(sends)
    started = time.monotonic()
    deadline = started + seconds

    while time.monotonic() < deadline:
        if pending and time.monotonic() - started >= pending[0][0]:
            _, text = pending.pop(0)
            os.write(fd, b"\x1b[200~" + text.encode() + b"\x1b[201~")
            time.sleep(0.03)
            os.write(fd, b"\r")
            print(f"[probe] sent {text!r} at t={time.monotonic() - started:.1f}s", file=sys.stderr)

        try:
            ready, _, _ = select.select([fd], [], [], 0.1)
        except select.error:
            break
        if not ready:
            continue
        try:
            data = os.read(fd, 65536)
        except OSError:
            break
        if not data:
            break
        chunks.append(data)

    try:
        os.killpg(os.getpgid(pid), signal.SIGKILL)
    except (ProcessLookupError, PermissionError):
        os.kill(pid, signal.SIGKILL)
    os.waitpid(pid, 0)
    os.close(fd)
    return b"".join(chunks)


def strip_ansi(text: str) -> str:
    text = re.sub(r"\x1b\][^\x07\x1b]*(\x07|\x1b\\)", "", text)
    return re.sub(r"\x1b[@-Z\\-_]|\x1b\[[0-?]*[ -/]*[@-~]", "", text)


# --- inspecting what claude wrote ----------------------------------------------


def find_transcripts(config_dir: Path):
    return sorted(config_dir.rglob("*.jsonl"))


def dump_transcript(path: Path, config_dir: Path):
    print(f"\n=== {path.relative_to(config_dir)} ===")
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    print(f"{len(lines)} lines")
    usage_progression = []
    top_level_types = {}
    for i, raw in enumerate(lines):
        try:
            entry = json.loads(raw)
        except json.JSONDecodeError as e:
            print(f"  line {i}: not JSON ({e})")
            continue
        etype = entry.get("type", "?")
        top_level_types[etype] = top_level_types.get(etype, 0) + 1
        usage = None
        if isinstance(entry.get("message"), dict):
            usage = entry["message"].get("usage")
        if usage is None:
            usage = entry.get("usage")
        if usage:
            usage_progression.append((i, etype, usage))

    print(f"top-level `type` counts: {top_level_types}")
    if lines:
        first = json.loads(lines[0])
        print(f"line 0 top-level keys: {sorted(first.keys())}")
        if isinstance(first.get("message"), dict):
            print(f"line 0 message keys: {sorted(first['message'].keys())}")

    print(f"\n{len(usage_progression)} lines carried a `usage` object:")
    for i, etype, usage in usage_progression:
        print(f"  line {i:3} ({etype:10}) usage={usage}")

    return usage_progression


def main():
    repo_root = Path(__file__).resolve().parents[2]
    work = Path(__file__).resolve().parent / "work"
    work.mkdir(parents=True, exist_ok=True)

    p = argparse.ArgumentParser()
    p.add_argument("--seconds", type=float, default=75.0)
    p.add_argument("--send-after", type=float, default=6.0)
    p.add_argument("--send-every", type=float, default=22.0)
    args = p.parse_args()

    cwd = work / "target-live"
    config_dir = work / "cc-config-live"
    if config_dir.exists():
        shutil.rmtree(config_dir)
    build_fixture(cwd)
    seed_config_dir(config_dir, cwd)

    key = os.environ.get("DEEPSEEK_API_KEY") or read_env_file(repo_root, "DEEPSEEK_API_KEY")
    if not key:
        sys.exit("DEEPSEEK_API_KEY not found in env or any .env from the repo root upward")

    brief = (
        "You are `worker-1` in a FLEETOR fleet spike. Answer arithmetic questions "
        "as briefly as possible — a bare number, nothing else."
    )
    argv = ["claude", "--permission-mode", "auto", "--system-prompt", brief]
    env = build_env(config_dir, DEEPSEEK_BASE_URL, key, MODEL_FLASH)

    sends = [
        (args.send_after, "what is 12 + 30? Reply with only the number."),
        (args.send_after + args.send_every, "now what is that number times 3? Reply with only the number."),
    ]

    print(f"[probe] cwd={cwd}", file=sys.stderr)
    print(f"[probe] config_dir={config_dir}", file=sys.stderr)
    raw = run(cwd, env, argv, sends, args.seconds)

    plain = strip_ansi(raw.decode("utf-8", errors="replace"))
    (work / "plain-live.log").write_text(plain)
    print("\n" + "=" * 72)
    print("tail of the pty output:")
    lines = [l for l in plain.splitlines() if l.strip()]
    print("\n".join(lines[-25:]))
    print("=" * 72)

    print("\n--- everything under CLAUDE_CONFIG_DIR ---")
    for path in sorted(config_dir.rglob("*")):
        if path.is_file():
            print(f"  {path.relative_to(config_dir)}  ({path.stat().st_size} bytes)")

    transcripts = find_transcripts(config_dir)
    if not transcripts:
        print("\nNO transcript (*.jsonl) found anywhere under CLAUDE_CONFIG_DIR.")
        return

    print(f"\n{len(transcripts)} transcript file(s) found:")
    for t in transcripts:
        print(f"  {t.relative_to(config_dir)}")

    for t in transcripts:
        dump_transcript(t, config_dir)


if __name__ == "__main__":
    main()
