#!/usr/bin/env python3
"""Does the codex brief survive a context clear? Re-measurable in one command.

    python3 examples/codex-spike/probe_clear.py

Measured against `codex-cli 0.153.4`. Spends **zero tokens**, by the same
mechanism `probe.py` uses (C13): the model provider is a local capture server
that answers every request with `data: [DONE]`, so no request ever reaches a
real endpoint and no assistant turn is ever completed.

C3 settled that `model_instructions_file` **replaces** the vendor's built-in
system prompt. It did not settle what happens after `/clear`. The brief is a
file the harness reads rather than a value snapshotted at boot, which is why it
is *expected* to survive — and expected is not measured. The failure it guards
against is this arc's signature one: a pane silently loses its identity mid-run
while looking perfectly healthy.

This is a **sibling** of `probe.py`, not a ninth arm in it. `probe.py`'s eight
arms and its exit-code contract are gated by `src-tauri/tests/vendor_binary_tier.rs`,
and this file needs a capture server that numbers *every* request rather than
overwriting one file — two turns have to be told apart. It reuses `probe.py`'s
scratch installation wholesale (the catalog clone, the provider block, the
sentinel brief) so the "missing field shell_type" lesson stays in one place.

Five arms, printing PASS/FAIL. Arms 1-4 carry the brief in the seeded
`config.toml`, the way a FLEETOR pane does; arm 5 repeats the question against
`-c model_instructions_file=…`, the spelling `probe.py` measures, so neither
carrier is reasoned from the other:

  1. clear-brief-before          the pre-clear turn carries the brief, not the
                                 built-in prompt — C3's property, re-measured in
                                 the **interactive TUI** rather than `codex exec`
  2. clear-actually-cleared      the post-clear turn's input no longer carries the
                                 pre-clear sentinel — the control, without which
                                 arm 3 is vacuous
  3. clear-brief-survives        **the question.** The post-clear turn's
                                 `instructions` still carries the brief and
                                 "You are Codex" is absent
  4. clear-brief-identical       the `instructions` field is byte-identical across
                                 the clear — rules out a partial or truncated re-read
  5. clear-brief-survives-via-flag  arm 3 again, brief carried on the command line

Exit code is the number of failed arms, so CI can gate on it. Skips cleanly
(exit 0, loud message) when `codex` is not on PATH — C13's stated rule.

Every URL here is loopback, deliberately, the same property
`vendor_binary_tier.rs` asserts of `probe.py`. That test now runs this file too
and gates on its exit code (#43), so a regression in the `/clear` measurement is
as loud as a regression in any of `probe.py`'s arms.

Its scratch root, its capture directory and its port are per-run, and it takes
`probe.py`'s one-run-at-a-time lock — see that module's header for why both
layers are there.
"""

import fcntl
import http.server
import json
import os
import pty
import re
import select
import shutil
import signal
import struct
import sys
import termios
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import probe  # noqa: E402  — the scratch installation, the build stamp, the PASS/FAIL printer

from probe import BUILD, FAILURES, check  # noqa: E402

PORT = 0  # assigned by the OS in `serve`; a fixed port is a collision waiting (#43)
ROOT = f"/tmp/codex-clear-spike-{os.getpid()}"
# Outside ROOT on purpose: `probe.build_scratch` deletes ROOT, and the second
# carrier's scratch would take the first carrier's evidence with it.
CAPROOT = f"/tmp/codex-clear-spike-{os.getpid()}-captures"
capdir = CAPROOT

PRE = "PRECLEAR-SENTINEL"
POST = "POSTCLEAR-SENTINEL"
BRIEF = "SENTINEL-MODELINSTR"
VENDOR_PROMPT_OPENER = "You are Codex"

# A real terminal answers these at startup; a bare pty does not, and codex paints
# nothing until it gives up waiting. Answering them is what turns C22's
# load-bearing "startup wait" into a readiness signal — see the notes.
TERMINAL_REPLIES = [
    (re.compile(rb"\x1b\[6n"), b"\x1b[1;1R"),                                    # cursor position
    (re.compile(rb"\x1b\]10;\?(\x07|\x1b\\)"), b"\x1b]10;rgb:ffff/ffff/ffff\x1b\\"),  # fg colour
    (re.compile(rb"\x1b\]11;\?(\x07|\x1b\\)"), b"\x1b]11;rgb:0000/0000/0000\x1b\\"),  # bg colour
    (re.compile(rb"\x1b\[\?u"), b"\x1b[?0u"),                                    # kitty keyboard
    (re.compile(rb"\x1b\[c"), b"\x1b[?1;2c"),                                    # device attributes
]

# `probe.build_scratch` names the fabricated catalog entry this. The pane's
# status line reads `model:loading` until the config has resolved, so the slug
# appearing is the signal that it has.
MODEL_SLUG = "probe-model"

captures = []


# --- the capture server: every request, in order ------------------------------


class Handler(http.server.BaseHTTPRequestHandler):
    """`probe.Handler` overwrites one file. Two turns have to be told apart."""

    def log_message(self, *a):
        pass

    def do_POST(self):
        n = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(n)
        captures.append(body)
        open(os.path.join(capdir, f"req-{len(captures):04d}.json"), "wb").write(body)
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        self.wfile.write(b"data: [DONE]\n\n")


def serve():
    """`probe.serve`, numbering every request: the port is the OS's to pick."""
    global PORT
    server = http.server.HTTPServer(("127.0.0.1", 0), Handler)
    PORT = server.server_address[1]
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return PORT


# --- scratch installation -----------------------------------------------------


def build_scratch(carrier):
    """`probe.build_scratch`, with the brief carried one of the two ways it can be.

    `probe.py` passes `model_instructions_file` on the command line; a FLEETOR
    pane gets it from the `config.toml` its config dir was seeded with (C6).
    Both spellings are measured here rather than one being reasoned from the
    other — that reasoning is exactly what this spike exists to refuse.

    In the `config` carrier the key must land in the **top-level** table.
    Appending it to the file puts it inside `[model_providers.probe]`, where it
    is silently ignored and the built-in prompt is sent instead — a failure that
    looks exactly like a "no". That mistake was made once while writing this.
    """
    global capdir
    probe.PORT, probe.ROOT = PORT, ROOT
    probe.CAPTURED = os.path.join(ROOT, "unused-by-this-probe.json")
    home, wt = probe.build_scratch()
    capdir = os.path.join(CAPROOT, carrier)
    shutil.rmtree(capdir, ignore_errors=True)
    os.makedirs(capdir, exist_ok=True)

    path = os.path.join(home, "config.toml")
    lines = open(path).read().splitlines(True)
    if carrier == "config":
        first_table = next(i for i, line in enumerate(lines) if line.lstrip().startswith("["))
        lines.insert(first_table, f'model_instructions_file = "{home}/brief.md"\n')
    # Provider-level retry caps. Every turn fails — the capture server never
    # completes one — and the default retry ladder turns each turn into a dozen
    # identical requests and a minute of waiting.
    lines.append("request_max_retries = 0\nstream_max_retries = 0\n")
    open(path, "w").write("".join(lines))

    argv = [] if carrier == "config" else ["-c", f"model_instructions_file={home}/brief.md"]
    return home, wt, argv


# --- the pane -----------------------------------------------------------------


class Pane:
    """A live codex TUI on a real pty, driven the way FLEETOR drives one."""

    def __init__(self, home, wt, argv=()):
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.chdir(wt)
            os.execve(
                probe.CODEX,
                [probe.CODEX, "--no-alt-screen", *argv],
                {**os.environ, "CODEX_HOME": home,
                 "TERM": "xterm-256color", "COLORTERM": "truecolor"},
            )
        # A 0x0 terminal is not a terminal: codex renders empty frames into it.
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
        self.buf = bytearray()
        self.answered = set()

    def pump(self, seconds):
        """Read the pane, answering its terminal queries as a real terminal would."""
        end = time.time() + seconds
        while time.time() < end:
            if not select.select([self.fd], [], [], 0.15)[0]:
                continue
            try:
                chunk = os.read(self.fd, 65536)
            except OSError:
                return
            if not chunk:
                return
            self.buf.extend(chunk)
            for i, (pattern, reply) in enumerate(TERMINAL_REPLIES):
                if i not in self.answered and pattern.search(chunk):
                    os.write(self.fd, reply)
                    self.answered.add(i)

    def screen(self):
        """Everything the pane has painted, escapes removed and whitespace collapsed.

        Codex repaints character by character with cursor moves between, so a
        word never survives as a word with its spacing intact. Collapsing all
        whitespace is what makes a substring test work at all — it is a
        readiness signal, not a rendering.
        """
        text = self.buf.decode("utf-8", "replace")
        text = re.sub(r"\x1b\][^\x07\x1b]*(\x07|\x1b\\)", "", text)
        text = re.sub(r"\x1b\[[0-9;?]*[a-zA-Z]", "", text)
        text = re.sub(r"\x1b.", "", text)
        return re.sub(r"\s+", "", text)

    def until(self, predicate, timeout):
        end = time.time() + timeout
        while time.time() < end:
            self.pump(0.3)
            if predicate():
                return True
        return False

    def settle(self, quiet, timeout):
        """Pump until no new request has arrived for `quiet` seconds."""
        end = time.time() + timeout
        seen, mark = len(captures), time.time()
        while time.time() < end:
            self.pump(0.3)
            if len(captures) != seen:
                seen, mark = len(captures), time.time()
            elif time.time() - mark >= quiet:
                return True
        return False

    def ready(self):
        """The pane has resolved its config **and** stopped repainting.

        Both halves are needed. The composer's placeholder paints once and is
        then scribbled over by an animated splash, so its presence alone fires
        while the pane is still ignoring input — the exact shape of C22's
        failure, met again in a new place.
        """
        if MODEL_SLUG not in self.screen():
            return False
        painted = len(self.buf)
        self.pump(1.5)
        return len(self.buf) == painted

    def wake(self, timeout=90):
        """Get to a composer that is listening.

        A fresh `CODEX_HOME` opens on an animated splash that does not end on
        its own — it ends on a keypress. `Enter` on an empty composer submits
        nothing, so pressing it until the pane goes quiet is safe, and it is a
        readiness *signal* rather than a guessed sleep.
        """
        end = time.time() + timeout
        while time.time() < end:
            if self.ready():
                return True
            os.write(self.fd, b"\r")
            self.pump(2.0)
        return self.ready()

    def say(self, text, timeout=60):
        """Deliver a message the way FLEETOR does — bracketed paste, then CR.

        The CR is not sent until the pasted text has actually painted in the
        composer. That is the readiness check C22 left open: a paste that never
        landed and a paste that landed but did not submit are different
        failures, and a fixed sleep cannot tell them apart.
        """
        before = len(captures)
        os.write(self.fd, b"\x1b[200~" + text.encode() + b"\x1b[201~")
        landed = self.until(lambda: text.replace(" ", "") in self.screen(), 15)
        if not landed:
            return False
        os.write(self.fd, b"\r")
        arrived = self.until(lambda: len(captures) > before, timeout)
        self.settle(3.0, 30)
        return arrived

    def command(self, text):
        """Deliver a command the way FLEETOR does — one unframed write, so `/`
        reaches column 0 (D-045).

        Byte-at-a-time delivery was measured too and behaves identically. Both
        spellings are only dropped while the pane is still on its splash, which
        is [`wake`]'s job and not the command channel's.
        """
        os.write(self.fd, text.encode())
        self.pump(1.5)
        os.write(self.fd, b"\r")
        self.pump(4.0)

    def close(self):
        try:
            os.kill(self.pid, signal.SIGTERM)
            os.waitpid(self.pid, 0)
        except (ProcessLookupError, ChildProcessError):
            pass


# --- reading the wire ---------------------------------------------------------


def turn_carrying(sentinel):
    """The first captured request whose body carries `sentinel`.

    Retries mean a turn is several identical requests, and a retry of turn one
    can land after the clear. Selecting by content rather than by position is
    what makes the reading independent of how the retry ladder happened to fall.
    """
    for body in captures:
        text = body.decode("utf-8", "replace")
        if sentinel in text:
            return json.loads(body)
    return None


def instructions_of(body):
    return body.get("instructions", "") if body else ""


# --- the arm ------------------------------------------------------------------


def drive(home, wt, argv):
    """One pane: a turn, a `/clear`, another turn. Returns the two request bodies.

    On failure the second element is a string saying which step did not happen,
    so an arm can fail with the reason rather than with a `None`.
    """
    captures.clear()
    pane = Pane(home, wt, argv)
    try:
        if not pane.wake():
            return None, "the pane never reached a listening composer"
        if not pane.say(f"{PRE} say ok"):
            return None, "the pre-clear turn never reached the wire"
        before = turn_carrying(PRE)

        pane.command("/clear")

        if not pane.say(f"{POST} say ok"):
            return before, "the post-clear turn never reached the wire"
        return before, turn_carrying(POST)
    finally:
        pane.close()


def arm_config_carrier(home, wt, argv):
    """The four arms, against the carrier a seeded pane actually uses."""
    names = ["clear-brief-before", "clear-actually-cleared",
             "clear-brief-survives", "clear-brief-identical"]
    before, after = drive(home, wt, argv)

    if before is None:
        for name in names:
            check(name, False, after)
        return
    if isinstance(after, str):
        check(names[0], BRIEF in instructions_of(before), f"{len(instructions_of(before))} chars")
        for name in names[1:]:
            check(name, False, after)
        return

    ins_before, ins_after = instructions_of(before), instructions_of(after)
    whole_after = json.dumps(after)

    check(
        "clear-brief-before",
        BRIEF in ins_before and VENDOR_PROMPT_OPENER not in json.dumps(before),
        f"instructions={len(ins_before)} chars",
    )

    # The control. If the clear did not take, the post-clear turn is only a
    # continuation and an unchanged `instructions` proves nothing at all.
    cleared = PRE not in whole_after
    check(
        "clear-actually-cleared",
        cleared,
        "the pre-clear turn is gone from the input" if cleared else
        "/clear did not take — the post-clear request still carries the pre-clear turn, "
        "so the arms below are vacuous",
    )

    # The question.
    check(
        "clear-brief-survives",
        BRIEF in ins_after and VENDOR_PROMPT_OPENER not in whole_after,
        f"instructions={len(ins_after)} chars, "
        f"{'no ' if VENDOR_PROMPT_OPENER not in whole_after else ''}{VENDOR_PROMPT_OPENER!r}",
    )

    check(
        "clear-brief-identical",
        bool(ins_before) and ins_before == ins_after,
        "byte-identical across the clear" if ins_before == ins_after
        else f"{len(ins_before)} chars before, {len(ins_after)} after",
    )


def arm_flag_carrier(home, wt, argv):
    """The same question against `-c model_instructions_file=…`, the spelling
    `probe.py` measures. One arm: the config carrier above owns the detail."""
    before, after = drive(home, wt, argv)
    if before is None or isinstance(after, str):
        check("clear-brief-survives-via-flag", False,
              after if isinstance(after, str) else "the pane never reached a listening composer")
        return
    ins_after, whole_after = instructions_of(after), json.dumps(after)
    check(
        "clear-brief-survives-via-flag",
        BRIEF in ins_after and PRE not in whole_after and VENDOR_PROMPT_OPENER not in whole_after,
        f"instructions={len(ins_after)} chars, cleared={PRE not in whole_after}",
    )


def main():
    if shutil.which("codex") is None:
        print(f"SKIP: codex is not on PATH. This tier is measured against {BUILD}.")
        return 0
    import subprocess

    if probe.hold_the_lock("codex-spike") is None:
        print(f"BUSY: another codex-spike run held {probe.LOCK} for {probe.LOCK_WAIT:.0f}s, so\n"
              f"      nothing was measured here. This is not a regression and not a clean\n"
              f"      machine either — re-run when the machine is quiet.")
        return 0

    serve()
    # The vendor binary, not the shim on PATH — `probe.py`'s header says why, and
    # this file's panes are exactly the ones whose `Stop` notified the operator.
    # It is resolved before the banner, so the banner reports the build of the
    # binary the arms actually run.
    probe.PORT, probe.ROOT = PORT, ROOT
    os.makedirs(ROOT, exist_ok=True)
    probe.CODEX = probe.resolve_vendor()

    actual = subprocess.run([probe.CODEX, "--version"],
                            capture_output=True, text=True).stdout.strip()
    print(f"codex-clear-spike — recorded against {BUILD}, running against {actual}")
    print(f"  vendor binary: {probe.CODEX}\n")
    if actual != BUILD:
        print("  NOTE: build differs from the recorded one. A failure below may be drift,\n"
              "        not a regression — re-record rather than patching around it.\n")

    home, wt, argv = build_scratch("config")
    time.sleep(0.5)
    arm_config_carrier(home, wt, argv)

    home, wt, argv = build_scratch("flag")
    time.sleep(0.5)
    arm_flag_carrier(home, wt, argv)

    print(f"\n{len(FAILURES)} failed" + (f": {', '.join(FAILURES)}" if FAILURES else ""))
    if FAILURES:
        print(f"  scratch kept at {ROOT} and the numbered requests at {CAPROOT}")
    else:
        shutil.rmtree(ROOT, ignore_errors=True)
        shutil.rmtree(CAPROOT, ignore_errors=True)
    return len(FAILURES)


if __name__ == "__main__":
    sys.exit(main())
