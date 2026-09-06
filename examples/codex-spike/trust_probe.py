#!/usr/bin/env python3
"""How codex resolves the project trust key — the WP-25 P2 spike, re-measurable in one command.

    python3 examples/codex-spike/trust_probe.py

A **sibling** of `probe.py`, not an edit to it: C13 makes these suite infrastructure
rather than throwaways, and the two answer different questions. Measured against
`codex-cli 0.153.4`. Spends **zero tokens** — harder than it sounds and cheaper than it
sounds, because *no arm ever completes a turn*. Nothing is typed into any pane; the
fabricated `[model_providers.probe]` points at a loopback port with nothing listening,
so there is not even a capture server to answer.

## The instrument

The first-run gate renders itself into the pty:

    Do you trust the contents of this directory?
    Working with untrusted contents comes with higher risk of prompt injection.
    › 1. Yes, continue   2. No, quit

A trusted directory never shows it and instead reaches the composer, resolving its
header to `directory: <canonical cwd>` and its status line to `· <canonical cwd>`.
Either spelling counts as trusted, and both are checked because **the header elides**:
past roughly 44 characters it renders `/private/tmp/…/parent/child` and a marker that
only looked for the header would report a trusted pane as inconclusive. Each arm boots a
real `codex` TUI on a real pty with a fabricated `CODEX_HOME`, drains until one of those
markers appears, and kills the pane. **Neither marker is a third outcome, not a pass**:
an arm that renders nothing is reported `INCONCLUSIVE` and counted as a failure, because
"no gate" must never be able to mean "codex crashed before drawing".

## What the twelve arms establish

Trust is **not** an ancestor walk and **not** a single exact key. A directory is trusted
when *either* its canonicalized cwd *or* the git root it resolves to is present verbatim
in `[projects."…"]`. The consequences that decide issue #30's shape:

  * a key for a plain parent does **not** trust its child, with or without git
    (`parent-key-does-not-trust-child`, `parent-of-repo-key-does-not-trust-repo`), so the
    by-root reading of C16 is wrong as stated — and the second of those reproduces the
    operator's own configuration, which is where C16's inference came from;
  * a key for a git root **does** trust every directory inside it
    (`git-root-key-trusts-subdir`), while a key for an intermediate directory trusts only
    itself (`midpath-key-does-not-trust-subdir`);
  * for a **linked git worktree** the resolved root is the **main repository**, so a key
    for the worktree trusts the worktree root and *nothing below it*
    (`worktree-key-does-not-trust-worktree-subdir`, `main-repo-key-trusts-worktree-subdir`);
  * the key must be spelled **canonically** — `/tmp/x` is ignored where the cwd resolves
    to `/private/tmp/x` (`unresolved-symlink-key-ignored`), exactly the macOS case
    `spawn::project_key` already canonicalizes for;
  * the gate reads the **persisted** `config.toml`. The identical entry passed as
    `-c projects."…".trust_level="trusted"` parses, lands in the projects table, and does
    **not** clear the gate (`cli-override-does-not-clear-gate`).

## The three wake arms (#47)

`arms_wake` is the probe #30 named and could not run: **this instrument with FLEETOR's own
bring-up wake in front of it.** `pty::wake` presses `\r` until the pane stops painting and
`1. Yes, continue` is the gate's pre-selected option, so the question is whether waking a
pane answers a dialog nobody meant to answer. It does, and it is worse than that:

  * `wake-settles-a-trusted-pane` — the control. A seeded pane meets no gate and settles
    after one press, so a "trusted" reading below cannot be a wake that does nothing;
  * `wake-dismisses-the-gate` — an unseeded pane paints the gate and the **second press
    dismisses it**, leaving a pane at its composer that looks perfectly healthy;
  * `wake-grants-trust-on-disk` — and codex **persists** `trust_level = "trusted"` for that
    directory into the pane's own `CODEX_HOME/config.toml`, which is the finding: the wake
    did not merely skip a dialog, it granted trust and wrote it down.

These arms wake a pane rather than only watching one, so unlike every arm above them they do
type — one `\r`, on the bring-up path, into a composer that is empty. Still zero tokens: an
empty submit is not a turn, and the provider is still a dead loopback port.

Exit code is the number of failed arms, so CI can gate on it. Skips cleanly (exit 0,
loud message) when `codex` is not on PATH — C13's stated rule. The git-fixture arms skip
loudly on their own if `git` is missing, which is not the same event.

Set `CODEX_BIN` to drive a specific binary. That is not decoration: on the machine this
was recorded on, `codex` on PATH is a cmux wrapper shim, and the matrix was confirmed
against both it and the npm binary it execs.
"""

import os
import pty
import re
import select
import shutil
import signal
import subprocess
import sys
import threading
import time

BUILD = "codex-cli 0.153.4"

# The scratch root must live under /tmp and not under `tempfile`'s longer paths, for two
# independent reasons: codex opens `app-server-control.sock` inside CODEX_HOME and AF_UNIX
# paths are capped near 104 bytes, and the symlink arms need a directory reachable by
# *both* the `/tmp` and `/private/tmp` spellings of one path.
ROOT = "/tmp/codex-trust"
HOME = ROOT + "/home"

# `os.path.realpath` is python's spelling of what `std::fs::canonicalize` does in
# `spawn::project_key`. Every expectation below is written against the canonical form.
CANON = os.path.realpath(ROOT)

# Loopback, and nothing is listening on it. No arm submits a turn, so no request is ever
# built; this is belt as well as braces.
DEAD_PROVIDER = "http://127.0.0.1:8791"

GATE = "Do you trust the contents of this directory"

BASE_CONFIG = (
    'model_provider = "probe"\n'
    "\n"
    "[model_providers.probe]\n"
    'name = "probe"\n'
    f'base_url = "{DEAD_PROVIDER}"\n'
    'wire_api = "responses"\n'
    'experimental_bearer_token = "sk-probe"\n'
)

FAILURES = []


def check(arm, ok, detail=""):
    print(f"  {'PASS' if ok else 'FAIL'}  {arm}{'  — ' + detail if detail else ''}")
    if not ok:
        FAILURES.append(arm)


# --- the fixtures -------------------------------------------------------------


def sh(*args, **kw):
    return subprocess.run(args, capture_output=True, text=True, timeout=60, **kw)


def build_scratch(have_git):
    """A fabricated CODEX_HOME, a plain nest, a git repo and a linked git worktree.

    The operator's real `~/.codex` is never read and never written — every arm rewrites
    `$ROOT/home/config.toml` and nothing else.
    """
    shutil.rmtree(ROOT, ignore_errors=True)
    for d in (HOME, ROOT + "/plain/parent/child", ROOT + "/repo/sub/deep"):
        os.makedirs(d)
    if not have_git:
        return
    sh("git", "init", "-q", ROOT + "/repo")
    sh("git", "-c", "user.email=probe@fleetor", "-c", "user.name=probe",
       "commit", "-q", "--allow-empty", "-m", "init", cwd=ROOT + "/repo")
    sh("git", "worktree", "add", "-q", "-b", "trust-probe", ROOT + "/wt",
       cwd=ROOT + "/repo")
    os.makedirs(ROOT + "/wt/inner", exist_ok=True)


# --- the measurement ----------------------------------------------------------


def boot(entries, cwd, extra=()):
    """Write the fabricated config and start one real codex TUI on a real pty.

    Returns `(pid, fd, trusted_markers)`. Nothing is ever typed by this function.
    """
    open(HOME + "/config.toml", "w").write(
        BASE_CONFIG
        + "".join(f'\n[projects."{e}"]\ntrust_level = "trusted"\n' for e in entries)
    )
    real = os.path.realpath(cwd)
    trusted_markers = ("directory: " + real, "· " + real)

    binary = os.environ.get("CODEX_BIN") or shutil.which("codex")
    pid, fd = pty.fork()
    if pid == 0:
        os.chdir(cwd)
        os.execve(
            binary,
            [binary, "--no-alt-screen", *extra],
            {**os.environ, "CODEX_HOME": HOME, "TERM": "xterm-256color",
             "COLORTERM": "truecolor"},
        )

    # A zero-sized pty makes the TUI draw nothing at all, which would read as
    # INCONCLUSIVE on every arm. Setting the window size is load-bearing.
    import fcntl
    import struct
    import termios
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
    return pid, fd, trusted_markers


def reap(pid, fd):
    for sig in (signal.SIGTERM, signal.SIGKILL):
        try:
            os.kill(pid, sig)
            for _ in range(20):
                if os.waitpid(pid, os.WNOHANG)[0]:
                    break
                time.sleep(0.05)
            else:
                continue
            break
        except (ProcessLookupError, ChildProcessError):
            break
    os.close(fd)


def observe(entries, cwd, extra=(), budget=14.0):
    """Boot one real codex TUI and report `"gate"`, `"trusted"` or `"inconclusive"`.

    Returns as soon as a marker appears, so a gated arm costs about three seconds. The
    pane is killed before anything is typed into it: that is the whole zero-token story.
    """
    pid, fd, trusted_markers = boot(entries, cwd, extra)

    buf, verdict, end = b"", "inconclusive", time.time() + budget
    while time.time() < end:
        if select.select([fd], [], [], 0.2)[0]:
            try:
                chunk = os.read(fd, 65536)
            except OSError:
                break
            if not chunk:
                break
            buf += chunk
            text = strip(buf)
            if GATE in text:
                verdict = "gate"
                break
            if any(m in text for m in trusted_markers):
                verdict = "trusted"
                break

    reap(pid, fd)
    return verdict


# --- the wake, in front of the gate (#47) -------------------------------------
#
# `pty::wake` presses the key a splash ends on until the pane stops painting, and
# `1. Yes, continue` is the trust gate's *pre-selected* option. #30 could not
# measure what that does to a gated pane without destroying its own control, so it
# named this probe as the thing that would settle it. The three constants below are
# `pty.rs`'s, spelled again here rather than approximated: a wake that pressed on a
# different cadence would answer a different question.

WAKE_KEY = b"\r"
PRESS_SETTLE = 0.6   # pty::PRESS_SETTLE
QUIET_SAMPLE = 0.4   # pty::QUIET_SAMPLE


def observe_woken(entries, cwd, extra=(), budget=25.0):
    """Boot one real codex TUI **and wake it the way `PaneRegistry::spawn` does**.

    Unlike [`observe`], this does not stop at the first marker: the whole question is
    what the pane looks like *after* the wake has finished pressing. Returns
    `(verdict, gate_seen, presses, persisted)` — the last being whatever
    `trust_level` rows the pane wrote back into its own `config.toml`, which is how a
    silently granted trust would leave a trace.
    """
    pid, fd, trusted_markers = boot(entries, cwd, extra)

    painted = [0]
    stop = threading.Event()
    presses = [0]

    def waking():
        # `pty::wake`, transcribed: press, let the press's own repaint land, then
        # ask whether the pane painted anything across one quiet sample.
        while not stop.is_set():
            try:
                os.write(fd, WAKE_KEY)
            except OSError:
                return
            presses[0] += 1
            if stop.wait(PRESS_SETTLE):
                return
            before = painted[0]
            if stop.wait(QUIET_SAMPLE):
                return
            if before > 0 and painted[0] == before:
                return

    waker = threading.Thread(target=waking, daemon=True)
    waker.start()

    buf, gate_seen, end = b"", False, time.time() + budget
    while time.time() < end:
        if select.select([fd], [], [], 0.2)[0]:
            try:
                chunk = os.read(fd, 65536)
            except OSError:
                break
            if not chunk:
                break
            buf += chunk
            painted[0] += len(chunk)
            if GATE in strip(buf):
                gate_seen = True
        if not waker.is_alive():
            # The wake has settled. Give the pane one more moment to finish
            # repainting whatever the last press left it on, then read the screen.
            time.sleep(1.0)
            while select.select([fd], [], [], 0.2)[0]:
                try:
                    chunk = os.read(fd, 65536)
                except OSError:
                    break
                if not chunk:
                    break
                buf += chunk
            break

    stop.set()
    waker.join(timeout=3)

    # **The last marker wins, not the first.** `buf` is everything the pane has ever
    # painted, and a gate that was answered is still in it — reading it the way
    # [`observe`] does would report `"gate"` for a pane that is sitting at its
    # composer. Which marker the pane painted *last* is the state it ended in.
    text = strip(buf)
    gate_seen = gate_seen or GATE in text
    at_gate = text.rfind(GATE)
    at_composer = max(text.rfind(m) for m in trusted_markers)
    if at_composer > at_gate:
        verdict = "trusted"
    elif at_gate >= 0:
        verdict = "gate"
    else:
        verdict = "inconclusive"

    reap(pid, fd)
    # What the pane wrote back into its own `config.toml`. A trust row that was not
    # seeded is the whole of #47: trust granted on the operator's behalf, by a
    # keypress nobody meant as an answer.
    persisted = sorted(
        set(re.findall(r'^\[projects\.(".*?")\]', open(HOME + "/config.toml").read(),
                       re.M))
        - {f'"{e}"' for e in entries}
    )
    return verdict, gate_seen, presses[0], persisted


def strip(b):
    """The rendered screen as plain text: escapes out, runs of whitespace collapsed."""
    t = re.sub(rb"\x1b\[[0-9;?]*[a-zA-Z]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)|\x1b[()][B0]",
               b" ", b)
    return re.sub(r"\s+", " ", t.decode("utf8", "replace"))


def arm(name, want, entries, cwd, extra=()):
    got = observe(entries, cwd, extra)
    check(name, got == want, f"expected {want}, saw {got}")


# --- arms ---------------------------------------------------------------------


def arms_plain():
    child = CANON + "/plain/parent/child"
    arm("no-entry-gates", "gate", [], child)
    arm("exact-key-trusted", "trusted", [child], child)
    # The by-root question, in its purest form: a plain parent, no git anywhere.
    arm("parent-key-does-not-trust-child", "gate", [CANON + "/plain/parent"], child)
    # Same key, same cwd, passed as a CLI override instead of persisted.
    arm("cli-override-does-not-clear-gate", "gate", [], child,
        ("-c", f'projects."{child}".trust_level="trusted"'))


def arms_symlink():
    child = CANON + "/plain/parent/child"
    unresolved = ROOT + "/plain/parent/child"
    if unresolved == child:
        print("  SKIP  symlink arms — /tmp is not a symlink on this machine")
        return
    arm("unresolved-symlink-key-ignored", "gate", [unresolved], child)
    arm("canonical-key-trusts-unresolved-cwd", "trusted", [child], unresolved)


def arms_git():
    repo, deep = CANON + "/repo", CANON + "/repo/sub/deep"
    arm("git-root-key-trusts-subdir", "trusted", [repo], deep)
    arm("midpath-key-does-not-trust-subdir", "gate", [repo + "/sub"], deep)
    # The operator's own shape, and the one that falsifies C16's inference: `~/harness`
    # is a plain directory carrying a trust entry, and `~/harness/fleetor` is the git
    # repository inside it. That entry does not trust this repository.
    arm("parent-of-repo-key-does-not-trust-repo", "gate", [CANON], repo + "/sub")


def arms_worktree():
    repo, wt = CANON + "/repo", CANON + "/wt"
    arm("worktree-key-trusts-worktree-root", "trusted", [wt], wt)
    # The sharp one for issue #30: a linked worktree resolves to the MAIN repo, so a key
    # for the worktree covers the worktree root and nothing under it.
    arm("worktree-key-does-not-trust-worktree-subdir", "gate", [wt], wt + "/inner")
    arm("main-repo-key-trusts-worktree-subdir", "trusted", [repo], wt + "/inner")


def arms_wake():
    """**#47's question**: does `pty::wake` answer a trust gate it was not seeded past?

    The gated arm is the one that matters. Its control is the same wake against a
    *seeded* pane, which must settle at the composer — without it a gated arm that
    reported "trusted" could just as easily be a wake that does nothing at all.
    """
    child = CANON + "/plain/parent/child"

    verdict, gate_seen, presses, persisted = observe_woken([child], child)
    check("wake-settles-a-trusted-pane", verdict == "trusted",
          f"expected trusted, saw {verdict} after {presses} presses "
          f"(gate seen: {gate_seen})")

    verdict, gate_seen, presses, persisted = observe_woken([], child)
    print(f"        gated pane + wake: verdict={verdict} gate_seen={gate_seen} "
          f"presses={presses} newly-trusted={persisted}")
    check("wake-dismisses-the-gate", gate_seen and verdict == "trusted",
          f"the gate appeared and the wake pressed through it, ending at "
          f"{verdict}")
    check("wake-grants-trust-on-disk", persisted == [f'"{child}"'],
          f"the pane persisted a trust row nobody seeded: {persisted}")


def main():
    binary = os.environ.get("CODEX_BIN") or shutil.which("codex")
    if binary is None:
        print(f"SKIP: codex is not on PATH. This tier is measured against {BUILD}.")
        return 0
    assert DEAD_PROVIDER.startswith("http://127.0.0.1:"), "the provider must be loopback"

    actual = sh(binary, "--version").stdout.strip()
    print(f"codex-trust-spike — recorded against {BUILD}, running against {actual}")
    print(f"  binary: {binary}")
    if actual != BUILD:
        print("\n  NOTE: build differs from the recorded one. A failure below may be drift,\n"
              "        not a regression — re-record rather than patching around it.\n")

    have_git = shutil.which("git") is not None
    build_scratch(have_git)
    print(f"  scratch: {CANON}  (CODEX_HOME is fabricated; ~/.codex is never touched)\n")

    arms_plain()
    arms_symlink()
    arms_wake()
    if have_git:
        arms_git()
        arms_worktree()
    else:
        print("  SKIP  git arms — git is not on PATH; the worktree finding is unmeasured")

    print(f"\n{len(FAILURES)} failed" + (f": {', '.join(FAILURES)}" if FAILURES else ""))
    return len(FAILURES)


if __name__ == "__main__":
    sys.exit(main())
