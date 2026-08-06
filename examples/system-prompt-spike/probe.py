#!/usr/bin/env python3
"""WP-02 spike: what changes when a pane's `claude` is launched with
`--system-prompt` instead of `--append-system-prompt`.

The sibling of `examples/tui-spawn-spike/spike.py`, which bisected the config
seed. Same shape — spawn a real interactive `claude` in a pty the way FLEETOR
does, log the raw bytes — with one addition that makes this question answerable
without guessing and without spending a token:

  **record mode** points `ANTHROPIC_BASE_URL` at a local Anthropic-compatible
  server that writes every request body to disk and streams back a canned
  reply. The system prompt CC actually assembled is then a file on disk, not an
  inference from how the model behaved. Diff two runs and you have the exact
  answer to "which default sections vanish".

  **live mode** is the worker posture for real — DeepSeek Flash, isolated
  seeded `CLAUDE_CONFIG_DIR`, `ANTHROPIC_API_KEY` removed — for the questions a
  fake endpoint cannot answer: does the pane reach its prompt, does it run Bash
  under `--permission-mode auto` without a dialog, do `/clear` and `/compact`
  still work.

Usage:
    python3 probe.py --mode record --flag system   --tag sysprompt
    python3 probe.py --mode record --flag append   --tag append
    python3 probe.py --mode live   --flag system --send "run: pwd"

Run outputs land in `work/` and are gitignored.
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
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

DEEPSEEK_BASE_URL = "https://api.deepseek.com/anthropic"
MODEL_FLASH = "deepseek-v4-flash"

# Markers reused from the Phase-0 spike. The TUI positions glyphs with cursor
# escapes, so once ANSI is stripped the inter-word spaces are gone too — every
# pattern is matched against a whitespace-free copy and is written without
# spaces.
ONBOARDING_MARKERS = [
    (r"choosethetextstyle|darkmode|lightmode", "theme picker"),
    (r"trustthefiles|doyoutrust|proceedwiththefiles", "trust dialog"),
    (r"usethisapikey|detectedacustomapikey|customapikey", "api-key approval"),
    (r"signinwith|invalidapikey|pleaserun/login", "login"),
    (r"letsgetstarted|let'sgetstarted|securitynotes|presstocontinue", "welcome/intro"),
]
# 2.1.223's idle TUI draws its mode line rather than the older "? for shortcuts"
# hint, so the Phase-0 markers no longer fire on a perfectly healthy pane.
PROMPT_MARKERS = [
    (r"\?forshortcuts|/helpforhelp|tryedit<filepath>", "input box ready (pre-2.1)"),
    (r"shift\+tabtocycle|automodeon|/effort", "mode line drawn"),
]
PERMISSION_MARKERS = [
    (r"doyouwantto(make|create|proceed|allow|run)", "permission prompt"),
    (r"1\.yes", "permission menu"),
]


# --- the recording endpoint ---------------------------------------------------


class Recorder(ThreadingHTTPServer):
    """An Anthropic-compatible endpoint that keeps every request it is sent."""

    daemon_threads = True
    allow_reuse_address = True

    def __init__(self, out_dir: Path, reply: str, tool: dict | None):
        super().__init__(("127.0.0.1", 0), _Handler)
        self.out_dir = out_dir
        self.reply = reply
        self.tool = tool
        self.requests: list[dict] = []
        self.lock = threading.Lock()

    @property
    def base_url(self) -> str:
        return f"http://127.0.0.1:{self.server_address[1]}"


def _sse(event: str, payload: dict) -> bytes:
    return f"event: {event}\ndata: {json.dumps(payload)}\n\n".encode()


class _Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_args):  # noqa: D102 - silence the default stderr spam
        pass

    def _body(self) -> dict:
        length = int(self.headers.get("content-length") or 0)
        raw = self.rfile.read(length) if length else b"{}"
        try:
            return json.loads(raw)
        except json.JSONDecodeError:
            return {"_unparsed": raw.decode("utf-8", "replace")}

    def do_POST(self):  # noqa: N802 - BaseHTTPRequestHandler's spelling
        body = self._body()
        with self.server.lock:
            self.server.requests.append({"path": self.path, "body": body})

        if "count_tokens" in self.path:
            return self._json({"input_tokens": 1})
        if body.get("stream"):
            return self._stream(body)
        return self._json(self._message(body))

    def do_GET(self):  # noqa: N802
        self._json({"data": []})

    def _message(self, body: dict) -> dict:
        content = [{"type": "text", "text": self.server.reply}]
        stop = "end_turn"
        if self.server.tool and _wants_tool(body):
            content = [{"type": "text", "text": self.server.reply}, dict(self.server.tool)]
            stop = "tool_use"
        return {
            "id": "msg_probe",
            "type": "message",
            "role": "assistant",
            "model": body.get("model", "probe"),
            "content": content,
            "stop_reason": stop,
            "stop_sequence": None,
            "usage": {"input_tokens": 1, "output_tokens": 1},
        }

    def _json(self, payload: dict):
        raw = json.dumps(payload).encode()
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def _stream(self, body: dict):
        message = self._message(body)
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("cache-control", "no-cache")
        # No content-length is knowable up front, so the end of the body has to
        # be the end of the connection — otherwise the client waits forever on a
        # stream it has already fully received.
        self.send_header("connection", "close")
        self.close_connection = True
        self.end_headers()

        head = dict(message, content=[], usage={"input_tokens": 1, "output_tokens": 0})
        self.wfile.write(_sse("message_start", {"type": "message_start", "message": head}))
        for index, block in enumerate(message["content"]):
            if block["type"] == "text":
                self.wfile.write(
                    _sse(
                        "content_block_start",
                        {"type": "content_block_start", "index": index,
                         "content_block": {"type": "text", "text": ""}},
                    )
                )
                self.wfile.write(
                    _sse(
                        "content_block_delta",
                        {"type": "content_block_delta", "index": index,
                         "delta": {"type": "text_delta", "text": block["text"]}},
                    )
                )
            else:
                self.wfile.write(
                    _sse(
                        "content_block_start",
                        {"type": "content_block_start", "index": index,
                         "content_block": dict(block, input={})},
                    )
                )
                self.wfile.write(
                    _sse(
                        "content_block_delta",
                        {"type": "content_block_delta", "index": index,
                         "delta": {"type": "input_json_delta",
                                   "partial_json": json.dumps(block["input"])}},
                    )
                )
            self.wfile.write(_sse("content_block_stop", {"type": "content_block_stop", "index": index}))
        self.wfile.write(
            _sse(
                "message_delta",
                {"type": "message_delta",
                 "delta": {"stop_reason": message["stop_reason"], "stop_sequence": None},
                 "usage": {"output_tokens": 1}},
            )
        )
        self.wfile.write(_sse("message_stop", {"type": "message_stop"}))
        self.wfile.flush()


def _wants_tool(body: dict) -> bool:
    """Only ask for the tool on the first turn — never in reply to its result."""
    for message in body.get("messages", []):
        for block in message.get("content", []) if isinstance(message.get("content"), list) else []:
            if isinstance(block, dict) and block.get("type") in ("tool_use", "tool_result"):
                return False
    return True


# --- the pane's environment ---------------------------------------------------


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


def build_env(args, repo_root: Path, base_url: str, token: str, model: str):
    """Exactly the worker posture from `src-tauri/src/spawn.rs`."""
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


def build_fixture(cwd: Path, config_dir: Path):
    """A scratch target that makes every context source individually visible.

    Each source carries its own nonsense marker, so a recorded system prompt
    says exactly which of them survived — project memory, user memory and a
    skill are three different questions.
    """
    if cwd.exists():
        shutil.rmtree(cwd)
    cwd.mkdir(parents=True)
    (cwd / "CLAUDE.md").write_text("# fixture project memory\n\nPROJECT_MEMORY_MARKER=zibbet\n")
    (cwd / "already.txt").write_text("committed\n")
    subprocess.run(["git", "init", "-q"], cwd=cwd, check=True)
    subprocess.run(["git", "add", "-A"], cwd=cwd, check=True)
    subprocess.run(
        ["git", "-c", "user.email=s@p", "-c", "user.name=spike", "commit", "-qm", "fixture"],
        cwd=cwd,
        check=True,
    )
    subprocess.run(["git", "checkout", "-qb", "fleet/worker-1"], cwd=cwd, check=True)
    (cwd / "dirty.txt").write_text("uncommitted\n")  # so git status has something to say

    config_dir.mkdir(parents=True, exist_ok=True)
    (config_dir / "CLAUDE.md").write_text("# fixture user memory\n\nUSER_MEMORY_MARKER=quorble\n")
    skill = config_dir / "skills" / "fixture-skill"
    skill.mkdir(parents=True, exist_ok=True)
    (skill / "SKILL.md").write_text(
        "---\nname: fixture-skill\ndescription: SKILL_MARKER=vandrel — a fixture skill.\n---\n\nbody\n"
    )


# --- running ------------------------------------------------------------------


def strip_ansi(text: str) -> str:
    text = re.sub(r"\x1b\][^\x07\x1b]*(\x07|\x1b\\)", "", text)
    return re.sub(r"\x1b[@-Z\\-_]|\x1b\[[0-?]*[ -/]*[@-~]", "", text)


def scan(plain: str, markers):
    squashed = re.sub(r"\s+", "", plain).lower()
    return [label for pattern, label in markers if re.search(pattern, squashed)]


def run(args, env, argv, sends):
    """Spawn under a pty, pump output, submit each `(after_seconds, mode, text)`.

    `mode` is `paste` — the bracketed paste the delivery path uses — or `type`,
    a slow character-by-character write. Slash commands need `type`: a `/` at
    the start of a *paste* is literal text, where a typed one opens CC's command
    menu, and that difference is the whole question for `/clear` and `/compact`.
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
    pending = list(sends)
    started = time.monotonic()
    deadline = started + args.seconds

    while time.monotonic() < deadline:
        if pending and time.monotonic() - started >= pending[0][0]:
            _, mode, text = pending.pop(0)
            if mode == "type":
                for char in text:
                    os.write(fd, char.encode())
                    time.sleep(args.type_delay)
            else:
                os.write(fd, b"\x1b[200~" + text.encode() + b"\x1b[201~")
            time.sleep(args.gap_ms / 1000.0)
            os.write(fd, b"\r")
            print(f"[probe] {mode}d {text!r}", file=sys.stderr)

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
    return b"".join(chunks), time.monotonic() - started


def system_text(request: dict) -> str:
    system = request.get("body", {}).get("system")
    if isinstance(system, str):
        return system
    if isinstance(system, list):
        return "\n\n".join(b.get("text", "") for b in system if isinstance(b, dict))
    return ""


def main():
    repo_root = Path(__file__).resolve().parents[2]
    work = Path(__file__).resolve().parent / "work"

    p = argparse.ArgumentParser()
    p.add_argument("--mode", choices=["record", "live"], default="record")
    p.add_argument("--flag", choices=["system", "append", "none"], default="system")
    p.add_argument("--tag", default=None)
    p.add_argument("--brief", type=Path, default=None, help="file whose text becomes the brief")
    p.add_argument("--cwd", type=Path, default=None)
    p.add_argument("--config-dir", type=Path, default=None)
    p.add_argument("--pane", default="worker-1")
    p.add_argument("--permission-mode", default="auto")
    p.add_argument("--send", action="append", default=[],
                   help="text to submit, repeatable; prefix with `type:` to keystroke it instead "
                        "of bracketed-pasting it (slash commands need that)")
    p.add_argument("--send-after", type=float, default=6.0)
    p.add_argument("--send-every", type=float, default=14.0)
    p.add_argument("--gap-ms", type=int, default=30)
    p.add_argument("--type-delay", type=float, default=0.04,
                   help="seconds per keystroke; CC's slash menu needs time to filter")
    p.add_argument("--seconds", type=float, default=20.0)
    p.add_argument("--rows", type=int, default=40)
    p.add_argument("--cols", type=int, default=120)
    p.add_argument("--tail", type=int, default=30)
    p.add_argument("--reply", default="OK from the probe endpoint.")
    p.add_argument("--tool-bash", default=None, help="record mode: make the canned reply run this")
    args = p.parse_args()

    tag = args.tag or f"{args.mode}-{args.flag}"
    work.mkdir(parents=True, exist_ok=True)
    args.cwd = (args.cwd or work / f"target-{tag}").expanduser().resolve()
    args.config_dir = (args.config_dir or work / f"cc-config-{tag}").expanduser().resolve()

    if args.config_dir.exists():
        shutil.rmtree(args.config_dir)
    build_fixture(args.cwd, args.config_dir)
    seed_config_dir(args.config_dir, args.cwd)

    brief = args.brief.read_text() if args.brief else "You are a FLEETOR pane. BRIEF_MARKER=frimble."

    argv = ["claude"]
    if args.permission_mode:
        argv += ["--permission-mode", args.permission_mode]
    if args.flag == "system":
        argv += ["--system-prompt", brief]
    elif args.flag == "append":
        argv += ["--append-system-prompt", brief]

    server = None
    if args.mode == "record":
        tool = None
        if args.tool_bash:
            tool = {"type": "tool_use", "id": "toolu_probe", "name": "Bash",
                    "input": {"command": args.tool_bash, "description": "probe"}}
        server = Recorder(work, args.reply, tool)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        env = build_env(args, repo_root, server.base_url, "probe-token", "probe-model")
    else:
        key = os.environ.get("DEEPSEEK_API_KEY") or read_env_file(repo_root, "DEEPSEEK_API_KEY")
        if not key:
            sys.exit("DEEPSEEK_API_KEY not found in env or any .env — needed for --mode live")
        env = build_env(args, repo_root, DEEPSEEK_BASE_URL, key, MODEL_FLASH)

    sends = [
        (args.send_after + i * args.send_every,
         "type" if text.startswith("type:") else "paste",
         text.removeprefix("type:"))
        for i, text in enumerate(args.send)
    ]
    print(f"[probe] {tag}: claude {' '.join(argv[1:3])} --{args.flag}-... cwd={args.cwd}", file=sys.stderr)

    raw, elapsed = run(args, env, argv, sends)
    if server:
        server.shutdown()

    plain = strip_ansi(raw.decode("utf-8", errors="replace"))
    (work / f"plain-{tag}.log").write_text(plain)
    (work / f"raw-{tag}.log").write_bytes(raw)

    print("\n" + "=" * 72)
    print(f"tag {tag}  bytes {len(raw)}  in {elapsed:.1f}s")
    print(f"onboarding : {scan(plain, ONBOARDING_MARKERS) or 'none detected'}")
    print(f"prompt     : {scan(plain, PROMPT_MARKERS) or 'NOT REACHED'}")
    print(f"permission : {scan(plain, PERMISSION_MARKERS) or 'none'}")

    if server:
        path = work / f"requests-{tag}.json"
        path.write_text(json.dumps(server.requests, indent=2))
        print(f"requests   : {len(server.requests)} → {path.name}")
        for request in server.requests:
            text = system_text(request)
            if text:
                (work / f"system-{tag}.txt").write_text(text)
                print(f"system     : {len(text)} chars → system-{tag}.txt")
                for marker in ("BRIEF_MARKER", "PROJECT_MEMORY_MARKER", "USER_MEMORY_MARKER",
                               "SKILL_MARKER", "fleet/worker-1", "Claude Code"):
                    print(f"   {'HIT ' if marker in text else 'miss'} {marker}")
                break
    print("=" * 72)
    lines = [l for l in plain.splitlines() if l.strip()]
    print("\n".join(lines[-args.tail:]))


if __name__ == "__main__":
    main()
