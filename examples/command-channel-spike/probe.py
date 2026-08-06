#!/usr/bin/env python3
"""WP-03 spike: what a *pasted* slash command does to a live `claude` pane.

The third in the line of `examples/tui-spawn-spike/spike.py` (which bisected the
config seed) and `examples/system-prompt-spike/probe.py` (which recorded the
system prompt). Same harness — spawn a real interactive `claude` in a pty the
way `src-tauri/src/spawn.rs` does, log the raw bytes, strip ANSI, scan for
markers.

WP-02 already settled the *mechanism*: a slash command typed one character at a
time opens CC's command menu, the characters after `/` never reach its filter,
and `Enter` picks the menu's first entry. A **bracketed paste** of the whole
line filters and selects correctly. So this spike does not re-ask "type or
paste". It asks the three things WP-03's delivery arm actually depends on, all
of them about a *pasted* command:

  1. **Empty input box.** Does a pasted `/compact <args>` run? Does `/clear`?
  2. **Queued text already in the box.** The operator (or a message that landed
     and was not submitted) left text there. What does the paste become?
  3. **Mid-turn.** The pane is thirty seconds into a turn. Does the command
     execute, queue, or land as prose?

Only `--mode live` exists here, because every one of those is a question about
the real TUI's input handling and a fake endpoint would answer them wrong in the
reassuring direction.

Usage:
    python3 probe.py --scenario empty
    python3 probe.py --scenario queued
    python3 probe.py --scenario midturn

Run outputs land in `work/` and are gitignored. Each run bills DeepSeek Flash.
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
import subprocess
import sys
import termios
import time
from pathlib import Path

DEEPSEEK_BASE_URL = "https://api.deepseek.com/anthropic"
MODEL_FLASH = "deepseek-v4-flash"

# The TUI positions glyphs with cursor escapes, so once ANSI is stripped the
# inter-word spaces are gone — every pattern is matched against a whitespace-free
# copy and is written without spaces.
PROMPT_MARKERS = [
    (r"shift\+tabtocycle|automodeon|/effort", "mode line drawn"),
    (r"\?forshortcuts|/helpforhelp|tryedit<filepath>", "input box ready (pre-2.1)"),
]
# What each command looks like when it *fired*, as opposed to when its text was
# submitted as an ordinary message.
COMMAND_MARKERS = [
    (r"notenoughmessagestocompact", "/compact ran (refused: too short)"),
    (r"compacted|compactingconversation|contextcompacted|previousconversation",
     "/compact ran (compacted)"),
    (r"welcometoclaudecode", "/clear ran (banner redrawn)"),
    (r"unknowncommand|couldnotfindcommand", "a command was rejected by CC"),
]
# The failure this spike exists to detect: the command arrived as prose and the
# model answered it conversationally instead of the TUI executing it.
PROSE_MARKERS = [
    (r"you(')?vetyped|youmentioned|itlookslikeyou|you(')?reasking", "answered as prose"),
]
MENU_MARKERS = [
    (r"/add-dir", "slash menu opened"),
]


# --- the pane's environment (identical posture to spawn.rs) --------------------


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


def build_env(args, base_url: str, token: str, model: str):
    env = dict(os.environ)
    env["TERM"] = "xterm-256color"
    env["COLORTERM"] = "truecolor"
    env["FLEETOR_PANE"] = args.pane
    env["CLAUDE_CONFIG_DIR"] = str(args.config_dir)
    env["ANTHROPIC_BASE_URL"] = base_url
    env["ANTHROPIC_AUTH_TOKEN"] = token
    env["ANTHROPIC_MODEL"] = model
    env.pop("ANTHROPIC_API_KEY", None)  # L2: wedges the TUI on approval
    env.pop("ANTHROPIC_DEFAULT_OPUS_MODEL", None)
    env.pop("ANTHROPIC_DEFAULT_SONNET_MODEL", None)
    env.pop("CLAUDE_CODE_CHILD_SESSION", None)
    return env


def seed_config_dir(config_dir: Path, cwd: Path):
    """The two keys `docs/tui-spawn-notes.md` bisected to, plus a theme."""
    config_dir.mkdir(parents=True, exist_ok=True)
    path = config_dir / ".claude.json"
    doc = json.loads(path.read_text()) if path.is_file() else {}
    doc["hasCompletedOnboarding"] = True
    doc["theme"] = "dark"
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


# --- running ------------------------------------------------------------------


def strip_ansi(text: str) -> str:
    text = re.sub(r"\x1b\][^\x07\x1b]*(\x07|\x1b\\)", "", text)
    return re.sub(r"\x1b[@-Z\\-_]|\x1b\[[0-?]*[ -/]*[@-~]", "", text)


def scan(plain: str, markers):
    squashed = re.sub(r"\s+", "", plain).lower()
    return [label for pattern, label in markers if re.search(pattern, squashed)]


def run(args, env, argv, sends):
    """Spawn under a pty and perform each `(after_seconds, mode, text)`.

    Three modes, and the difference between them is the whole spike:

      `paste`   — bracketed paste then, after the submit gap, `\\r`. **Exactly
                  what `PaneRegistry::write_paste` does**, byte for byte.
      `noenter` — bracketed paste with no `\\r`, to leave text sitting in the
                  input box the way an unsubmitted operator keystroke would.
      `type`    — character by character. Kept only so a run can reproduce
                  WP-02's menu-hijack finding for contrast.
    """
    pid, fd = pty.fork()
    if pid == 0:
        os.chdir(str(args.cwd))
        os.execvpe(argv[0], argv, env)
        os._exit(127)

    try:
        import fcntl
        import struct

        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", args.rows, args.cols, 0, 0))
    except Exception:
        pass

    chunks = []
    marks = []
    pending = list(sends)
    started = time.monotonic()
    deadline = started + args.seconds

    while time.monotonic() < deadline:
        if pending and time.monotonic() - started >= pending[0][0]:
            _, mode, text = pending.pop(0)
            offset = sum(len(c) for c in chunks)
            if mode == "type":
                for char in text:
                    os.write(fd, char.encode())
                    time.sleep(args.type_delay)
                time.sleep(args.gap_ms / 1000.0)
                os.write(fd, b"\r")
            else:
                os.write(fd, b"\x1b[200~" + text.encode() + b"\x1b[201~")
                if mode != "noenter":
                    time.sleep(args.gap_ms / 1000.0)
                    os.write(fd, b"\r")
            marks.append((round(time.monotonic() - started, 1), mode, text, offset))
            print(f"[probe] +{marks[-1][0]:>5.1f}s {mode:<7} {text!r}", file=sys.stderr)

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

    try:
        os.killpg(os.getpgid(pid), signal.SIGKILL)
    except (ProcessLookupError, PermissionError):
        os.kill(pid, signal.SIGKILL)
    os.waitpid(pid, 0)
    os.close(fd)
    return b"".join(chunks), marks, time.monotonic() - started


# --- the scenarios ------------------------------------------------------------

# `(after_seconds, mode, text)`. Every command goes in as `paste`, because WP-02
# already proved `type` fires the wrong menu entry.
SCENARIOS = {
    # 1. The ordinary case: nothing in the box, one command at a time. `/compact`
    #    with arguments first (the form WP-03's brief teaches), `/clear` second —
    #    in that order so the compact evidence is not wiped by the clear.
    "empty": [
        (8.0, "paste", "say READY and nothing else"),
        (26.0, "paste", "/compact focus on the current task"),
        (52.0, "paste", "/clear"),
        (70.0, "paste", "say AFTERCLEAR and nothing else"),
    ],
    # 2. Text already sitting unsubmitted in the input box, then a command. This
    #    is the one the delivery arm cannot control: the operator types, or a
    #    message lands and is not submitted, and then `fleet cmd` arrives.
    "queued": [
        (8.0, "noenter", "half a sentence the operator was still writing"),
        (12.0, "paste", "/compact keep the current task"),
        (40.0, "noenter", "another unfinished thought"),
        (44.0, "paste", "/clear"),
    ],
    # 3. Nominally mid-turn — but `deepseek-v4-flash` answered a 40-line count in
    #    three seconds, so the command landed at an idle prompt. Kept because the
    #    negative result is the reason `busy-*` below exists.
    "midturn": [
        (8.0, "paste", "count from 1 to 40, one number per line, slowly, no tools"),
        (13.0, "paste", "/compact keep the counting task"),
        (60.0, "paste", "say STILLHERE and nothing else"),
    ],
    # 4/5. Genuinely mid-turn. A `sleep` under `--permission-mode auto` is the
    #      only way to hold a Flash pane busy for a known number of seconds, so
    #      the command lands while a tool call is in flight rather than while we
    #      hope the model is still typing.
    "busy-compact": [
        (8.0, "paste", "run exactly this in bash and say nothing else: sleep 45; echo SLEPT"),
        (26.0, "paste", "/compact keep the sleeping task"),
        (75.0, "paste", "say STILLHERE and nothing else"),
    ],
    "busy-clear": [
        (8.0, "paste", "run exactly this in bash and say nothing else: sleep 45; echo SLEPT"),
        (26.0, "paste", "/clear"),
        (75.0, "paste", "say AFTERCLEAR and nothing else"),
    ],
    # 6. Whether a mid-turn `/clear` *lands* cannot be read off a repainting TUI:
    #    the command menu re-emits the scrollback banner, which is also what a
    #    real reset draws. So ask the pane instead. A codeword is planted, a long
    #    turn is started, `/clear` lands mid-turn, and after the turn the pane is
    #    asked for the codeword. Knowing it means the clear did not land.
    "clear-witness": [
        (8.0, "paste", "remember the codeword ZIBBET. reply OK."),
        (24.0, "paste", "run exactly this in bash and say nothing else: sleep 40; echo SLEPT"),
        (40.0, "paste", "/clear"),
        (95.0, "paste", "what is the codeword? if you do not know one, say NOCODEWORD."),
    ],
    # 7. Contrast run, kept for the record: WP-02's typed-command hijack.
    "typed": [
        (8.0, "type", "/compact focus on the current task"),
    ],
}

# The pane's brief. Tool use is forbidden by default so a scenario measures the
# TUI's input handling and not the model's appetite for shell commands — except
# in the `busy-*` runs, where a real tool call is the clock.
BRIEF_NO_TOOLS = "You are a FLEETOR worker pane. Answer in one short line. Never use tools."
BRIEF_TOOLS = (
    "You are a FLEETOR worker pane. When asked to run a command, run it with Bash "
    "exactly as given, and say nothing else."
)


def main():
    repo_root = Path(__file__).resolve().parents[2]
    work = Path(__file__).resolve().parent / "work"

    p = argparse.ArgumentParser()
    p.add_argument("--scenario", choices=sorted(SCENARIOS), default="empty")
    p.add_argument("--tag", default=None)
    p.add_argument("--cwd", type=Path, default=None)
    p.add_argument("--config-dir", type=Path, default=None)
    p.add_argument("--pane", default="worker-1")
    p.add_argument("--permission-mode", default="auto")
    p.add_argument("--gap-ms", type=int, default=30, help="pty.rs SUBMIT_GAP")
    p.add_argument("--type-delay", type=float, default=0.04)
    p.add_argument("--seconds", type=float, default=None)
    p.add_argument("--rows", type=int, default=40)
    p.add_argument("--cols", type=int, default=120)
    p.add_argument("--tail", type=int, default=40)
    args = p.parse_args()

    sends = SCENARIOS[args.scenario]
    if args.seconds is None:
        args.seconds = sends[-1][0] + 24.0

    tag = args.tag or args.scenario
    work.mkdir(parents=True, exist_ok=True)
    args.cwd = (args.cwd or work / f"target-{tag}").expanduser().resolve()
    args.config_dir = (args.config_dir or work / f"cc-config-{tag}").expanduser().resolve()

    if args.config_dir.exists():
        shutil.rmtree(args.config_dir)
    build_fixture(args.cwd)
    seed_config_dir(args.config_dir, args.cwd)

    key = os.environ.get("DEEPSEEK_API_KEY") or read_env_file(repo_root, "DEEPSEEK_API_KEY")
    if not key:
        sys.exit("DEEPSEEK_API_KEY not found in env or any .env")

    # The worker's real brief prose is irrelevant here — the question is the TUI's
    # input handling — but the flag is the live one so the posture matches.
    brief = BRIEF_TOOLS if args.scenario.startswith("busy-") or args.scenario == "clear-witness" else BRIEF_NO_TOOLS
    argv = ["claude", "--permission-mode", args.permission_mode, "--system-prompt", brief]
    env = build_env(args, DEEPSEEK_BASE_URL, key, MODEL_FLASH)

    print(f"[probe] {tag}: {args.seconds:.0f}s, cwd={args.cwd}", file=sys.stderr)
    raw, marks, elapsed = run(args, env, argv, sends)

    plain = strip_ansi(raw.decode("utf-8", errors="replace"))
    (work / f"plain-{tag}.log").write_text(plain)
    (work / f"raw-{tag}.log").write_bytes(raw)

    print("\n" + "=" * 72)
    print(f"scenario {tag}  bytes {len(raw)}  in {elapsed:.1f}s")
    print(f"prompt   : {scan(plain, PROMPT_MARKERS) or 'NOT REACHED'}")
    print(f"command  : {scan(plain, COMMAND_MARKERS) or 'no command-execution marker'}")
    print(f"menu     : {scan(plain, MENU_MARKERS) or 'none'}")
    print(f"prose    : {scan(plain, PROSE_MARKERS) or 'none'}")
    print("sends    :")
    for at, mode, text, offset in marks:
        print(f"   +{at:>5.1f}s {mode:<7} @byte {offset:<8} {text!r}")
    print("=" * 72)
    lines = [line for line in plain.splitlines() if line.strip()]
    print("\n".join(lines[-args.tail:]))


if __name__ == "__main__":
    main()
