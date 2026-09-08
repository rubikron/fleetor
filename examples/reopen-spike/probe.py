#!/usr/bin/env python3
"""Can a past run's panes be reopened from local logs? (WP-27, R3/R4/R6's spikes)

    python3 examples/reopen-spike/probe.py --spend
    python3 examples/reopen-spike/probe.py --reuse     # free, re-reads a past run

*** THIS PROBE SPENDS REAL MONEY, but only to CREATE the sessions it then
*** reopens. The reopen itself is free: launching a vendor TUI against a
*** recorded session RENDERS it, and no model call happens until a turn is
*** submitted. So the expensive half is 2 tiny turns per harness (create,
*** then resume-and-append) and every question about reopening is measured
*** for free under a pty.
*** Without `--spend` (or FLEETOR_SPEND_OK=1) it prints this banner and exits 0.

Measured against `2.1.263 (Claude Code)` and `codex-cli 0.153.4`.

WHY A PTY. WP-27's panes are interactive TUIs. `-p`/`exec` answers where a
session lands and what its id is, but it cannot answer the two questions that
are interactive-only: whether a fenced config dir walls the reopened pane behind
a login, and whether codex's working-directory picker fires where a read-only
pane could never answer it. Those arms drive the real TUI through `pty.fork()`,
the same way `examples/tui-spawn-spike/spike.py` does.

BOTH VENDOR BINARIES ARE RESOLVED ABSOLUTELY. C47 recorded that the PATH `codex`
is a cmux shim; on this machine the PATH `claude` is one too, so a probe that
took either from PATH would be measuring cmux plus the vendor. Neither
operator config dir is ever written to — both are copied into a fabricated home.

Arms, PASS/FAIL:

  claude-code
    1. cc-turn-completes        one `-p` turn under a fenced CLAUDE_CONFIG_DIR   [spend]
    2. cc-session-under-config  a `.jsonl` lands under <cfg>/projects/<slug>/
    3. cc-id-is-filename        the file stem is the id `--resume` accepts (R6)
    4. cc-resume-renders        interactive `--resume <id>` reaches a live TUI    [pty, free]
    5. cc-resume-no-login-wall  ...without /login, onboarding or a trust dialog   [pty, free]
    6. cc-resume-shows-prior    the rendered screen carries the first turn        [pty, free]
    7. cc-resume-same-file      a resumed turn appends to the SAME .jsonl (R3/R4) [spend]

  codex
    8.  cx-turn-completes       one `exec` turn under a fenced CODEX_HOME         [spend]
    9.  cx-store-written        `thread_history_1.sqlite` appears (C12)
    10. cx-id-in-thread-items   `thread_id` is readable from the store (R6)
    11. cx-resume-renders       `codex resume <uuid>` reaches a live TUI          [pty, free]
    12. cx-cwd-picker-fires     does stock resume still show the cwd picker?      [pty, free]
    13. cx-resume-cwd-answers   `-c tui.resume_cwd=current` suppresses it         [pty, free]
    14. cx-resume-same-thread   a resumed turn appends to the SAME thread_id      [spend]

Arm 12 is the one CARRIED fact in WP-27 that was never re-measured (decisions.md
R0 lets vendor facts carry; this checks the carry still holds at 0.153.4). It is
recorded PASS when the picker fires — that is what the earlier shakedown found —
and the arm prints loudly either way, so a vendor that fixed it is noticed
rather than assumed.

Exit code is the number of failed arms. Skips cleanly (exit 0, loud message)
when a vendor binary is absent.
"""

import argparse
import fcntl
import glob
import json
import os
import pty
import re
import select
import shutil
import signal
import sqlite3
import struct
import subprocess
import sys
import termios
import time

CC_BUILD = "2.1.263 (Claude Code)"
CX_BUILD = "codex-cli 0.153.4"

ROOT = "/tmp/fleetor-reopen-spike"

# Both resolved absolutely — the PATH entries are cmux shims (C47, widened).
CC_VENDOR = os.path.expanduser("~/.local/share/claude/versions/2.1.263")
CX_VENDOR = ("/opt/homebrew/lib/node_modules/@openai/codex/node_modules/"
             "@openai/codex-darwin-arm64/vendor/aarch64-apple-darwin/bin/codex")

CC_OPERATOR = os.path.expanduser("~/.claude")
CX_OPERATOR = os.path.expanduser("~/.codex")

# A marker the resumed pane must be able to show us it remembers.
MARKER = "PELICAN-7731"
TURN_1 = f"Remember this token and reply with exactly it, nothing else: {MARKER}"
TURN_2 = "Reply with exactly the token I asked you to remember, nothing else."

FAILURES = []
SKIPPED = []


def check(arm, ok, detail=""):
    print(f"  {'PASS' if ok else 'FAIL'}  {arm}{'  — ' + detail if detail else ''}")
    if not ok:
        FAILURES.append(arm)


def note(text):
    print(f"        {text}")


def banner(text):
    print(f"\n=== {text} ===")


# --- pty driver ---------------------------------------------------------------

def render(argv, env, cwd, seconds=10.0, wake=None, wake_after=3.0):
    """Launch a TUI under a pty, pump its output, and return what it painted.

    Nothing is ever submitted, so no turn runs and nothing is spent — this only
    watches the vendor draw a session it already had on disk.

    **Teardown never writes to the master, and that is load-bearing.** The first
    version of this sent ^C to the pty before killing, and hung for eleven
    minutes on a twelve-second deadline: a write to a pty master whose child has
    stopped reading blocks forever, so the child sat as a zombie and `waitpid`
    was never reached. Signal the child's process group instead — `pty.fork`
    makes it a session leader — and reap without blocking.
    """
    pid, fd = pty.fork()
    if pid == 0:  # child — session leader, its own pgid
        try:
            os.chdir(cwd)
            os.execve(argv[0], argv, env)
        except BaseException:
            os._exit(127)

    # **A pty with no window size paints nothing.** `pty.fork` leaves the size at
    # 0x0; Claude Code coped, codex drew only its terminal-mode preamble and no
    # content at all. Measured — this is why `cx-resume-renders` first read 12
    # characters. Set it before the child draws.
    try:
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
    except OSError:
        pass

    out = bytearray()
    os.set_blocking(fd, False)
    started = time.time()
    deadline = started + seconds
    woken = wake is None
    try:
        while time.time() < deadline:
            # **The wake is `pty.rs`'s WAKE_KEY, and it is sent WHILE THE CHILD IS
            # ALIVE AND READING** — which is what makes it safe, unlike the
            # teardown-time write that hung this probe for eleven minutes. Codex
            # answers BringUp::AfterWaking: it opens on a splash that ends on a
            # keypress and discards anything sent before then, so a render that
            # never wakes it photographs the splash and calls it the session.
            if not woken and time.time() - started >= wake_after:
                try:
                    os.write(fd, wake)
                except (BlockingIOError, OSError):
                    pass
                woken = True
            r, _, _ = select.select([fd], [], [], 0.25)
            if fd in r:
                try:
                    chunk = os.read(fd, 65536)
                except BlockingIOError:
                    continue
                except OSError:
                    break
                if not chunk:
                    break
                out += chunk
    finally:
        try:
            os.killpg(os.getpgid(pid), signal.SIGKILL)
        except (ProcessLookupError, PermissionError, OSError):
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        for _ in range(60):  # reap without ever blocking
            try:
                if os.waitpid(pid, os.WNOHANG)[0]:
                    break
            except ChildProcessError:
                break
            time.sleep(0.05)
        try:
            os.close(fd)
        except OSError:
            pass
    return out.decode("utf-8", "replace")


ANSI = re.compile(r"\x1b\[[0-9;?]*[A-Za-z]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)|\x1b[()][A-B0-2]|\x1b[=>]")


def plain(screen):
    """ANSI-stripped text, for reading a render by eye."""
    return ANSI.sub("", screen)


def squash(text):
    """ANSI-stripped, whitespace-removed, lowercased.

    **A TUI paints words apart with cursor moves, not spaces.** Stripping the
    escapes leaves `Accessingworkspace:` — so every multi-word needle silently
    fails to match and a wall-detection arm reports a clean screen it never
    read. Both sides of every comparison go through this.
    """
    return "".join(ANSI.sub("", text).split()).lower()


def painted(screen, needle):
    return squash(needle) in squash(screen)


# --- claude code ---------------------------------------------------------------

def cc_home(work):
    """A fenced CLAUDE_CONFIG_DIR seeded to D-030's measured two-key minimum."""
    cfg = os.path.join(work, "config")
    cwd = os.path.join(work, "cwd")
    os.makedirs(cfg, exist_ok=True)
    os.makedirs(cwd, exist_ok=True)
    # MEASURED: the key must be the REALPATH. Seeding /tmp/... while the process
    # resolves to /private/tmp/... leaves the pane on a trust dialog it cannot be
    # sent past, which is exactly how a reopened pane would hang.
    cwd = os.path.realpath(cwd)
    # D-030: a virgin config dir never reaches a prompt. hasTrustDialogAccepted
    # is keyed by ABSOLUTE path, so it names this cwd specifically.
    json.dump(
        {"hasCompletedOnboarding": True,
         "projects": {cwd: {"hasTrustDialogAccepted": True,
                            "hasCompletedProjectOnboarding": True}}},
        open(os.path.join(cfg, ".claude.json"), "w"),
    )
    return cfg, cwd


def cc_env(cfg):
    env = dict(os.environ)
    env["CLAUDE_CONFIG_DIR"] = cfg
    # C73: the login keychain follows HOME, so a private HOME would wall the
    # pane behind /login. The operator's HOME is kept, which is what WP-27's
    # reopened panes will also do.
    env.pop("CLAUDE_CODE_CHILD_SESSION", None)  # D-030: leaks through, kills persistence
    env.pop("ANTHROPIC_API_KEY", None)          # D-030: blocks the interactive TUI
    env["TERM"] = "xterm-256color"
    return env


def cc_sessions(cfg):
    return sorted(glob.glob(os.path.join(cfg, "projects", "*", "*.jsonl")))


def run_claude_code(work, spend):
    banner("claude-code")
    cfg, cwd = cc_home(work)
    env = cc_env(cfg)

    # A session already on disk is reused rather than re-bought. Re-running this
    # probe to re-measure a FREE arm must never re-spend on an arm that passed.
    if cc_sessions(cfg):
        check("cc-turn-completes", True, "session already on disk — not re-spent")
    elif spend:
        r = subprocess.run([CC_VENDOR, "-p", TURN_1],
                           env=env, cwd=cwd, capture_output=True, text=True, timeout=240)
        ok = r.returncode == 0 and MARKER in (r.stdout or "")
        check("cc-turn-completes", ok, (r.stdout or r.stderr or "")[:120].replace("\n", " "))
    else:
        check("cc-turn-completes", False, "no session on disk and no --spend")

    files = cc_sessions(cfg)
    check("cc-session-under-config", bool(files),
          f"{len(files)} .jsonl under {os.path.relpath(files[0], cfg) if files else 'projects/'}")
    if not files:
        return None
    path = files[0]
    stem = os.path.basename(path)[: -len(".jsonl")]

    # R6's Claude Code answer: the id --resume wants IS the filename stem. Proven
    # by the file's own rows carrying the same sessionId, not by shape alone.
    ids = set()
    for line in open(path, encoding="utf-8", errors="replace"):
        try:
            ids.add(json.loads(line).get("sessionId"))
        except Exception:
            pass
    ids.discard(None)
    check("cc-id-is-filename", ids == {stem}, f"stem={stem} rows={sorted(ids)}")

    screen = render([CC_VENDOR, "--resume", stem], env, cwd, seconds=12)
    flat = plain(screen)
    check("cc-resume-renders", len(flat.strip()) > 40, f"{len(screen)} bytes painted")

    # **Onboarding and trust only — deliberately NOT the credential.** This probe
    # does not seed `.credentials.json`, which the product always does
    # (`harness.rs` write_operator_login: a fenced pane has no login keychain on
    # its search list). So an interactive render here says "Not logged in"; that
    # is this probe's gap, not the reopen path's, and chasing the operator's
    # keychain would prompt them to prove something WP-27 already relies on.
    # What R4 actually risks is a per-run config dir starting COLD — an
    # onboarding or trust wall — and that is what these words test.
    walls = [w for w in ("Welcome to Claude Code", "Do you trust the files",
                         "Quick safety check", "Yes, I trust this folder",
                         "Select login method", "onboarding") if painted(screen, w)]
    check("cc-resume-no-cold-start-wall", not walls, f"hit: {walls}" if walls else "clean")
    if painted(screen, "/login"):
        note("render says Not logged in — expected: this probe seeds no credential (see above)")
    seen = painted(screen, MARKER)
    check("cc-resume-shows-prior", seen,
          "marker painted" if seen else "marker absent from the render")

    if spend:
        before = set(cc_sessions(cfg))
        r = subprocess.run([CC_VENDOR, "-p", "--resume", stem, TURN_2],
                           env=env, cwd=cwd, capture_output=True, text=True, timeout=240)
        after = set(cc_sessions(cfg))
        remembered = MARKER in (r.stdout or "")
        check("cc-resume-same-file", after == before and remembered,
              f"new files={len(after - before)} remembered={remembered}")
    else:
        SKIPPED.append("cc-resume-same-file")
        print("  SKIP  cc-resume-same-file — needs --spend")
    return stem


# --- codex ---------------------------------------------------------------------

def cx_home(work):
    """A fenced CODEX_HOME: C39's allowlist plus the credential."""
    home = os.path.join(work, "codex-home")
    cwd = os.path.join(work, "codex-cwd")
    os.makedirs(home, exist_ok=True)
    os.makedirs(cwd, exist_ok=True)
    for name in ("auth.json", "config.toml", "models.json"):
        src = os.path.join(CX_OPERATOR, name)
        if os.path.isfile(src):
            shutil.copy2(src, os.path.join(home, name))
    return home, cwd


def cx_env(home):
    env = dict(os.environ)
    env["CODEX_HOME"] = home
    env["TERM"] = "xterm-256color"
    return env


def cx_store(home):
    hits = glob.glob(os.path.join(home, "thread_history_*.sqlite"))
    return hits[0] if hits else None


def cx_thread_ids(store):
    """Every thread_id in the store, newest last — R6's codex answer (C12)."""
    con = sqlite3.connect(f"file:{store}?mode=ro", uri=True)
    try:
        rows = con.execute(
            "SELECT thread_id, COUNT(*) FROM thread_items GROUP BY thread_id"
        ).fetchall()
    finally:
        con.close()
    return {tid: n for tid, n in rows}


def run_codex(work, spend):
    banner("codex")
    home, cwd = cx_home(work)
    env = cx_env(home)

    if cx_store(home):
        check("cx-turn-completes", True, "thread store already on disk — not re-spent")
    elif spend:
        r = subprocess.run([CX_VENDOR, "exec", "--skip-git-repo-check", TURN_1],
                           env=env, cwd=cwd, capture_output=True, text=True, timeout=300,
                           stdin=subprocess.DEVNULL)
        ok = r.returncode == 0 and MARKER in (r.stdout or "")
        check("cx-turn-completes", ok, (r.stdout or r.stderr or "")[-160:].replace("\n", " "))
    else:
        check("cx-turn-completes", False, "no thread store on disk and no --spend")

    store = cx_store(home)
    check("cx-store-written", bool(store), os.path.basename(store) if store else "absent")
    if not store:
        return None

    threads = cx_thread_ids(store)
    check("cx-id-in-thread-items", bool(threads), f"{len(threads)} thread(s): {list(threads)[:2]}")
    if not threads:
        return None
    tid = max(threads, key=threads.get)

    # **The picker is cwd-DEPENDENT, and that is the correction.** WP-27 carried
    # a claim that it "fires unconditionally on this version" even when the launch
    # cwd matches the recorded one. Measured here: it does not. The earlier
    # shakedown drove a viewer whose ephemeral cwd could never match, so it only
    # ever saw the firing case. Both cases are measured below so neither claim can
    # be asserted from one setup again.
    picker_words = ("Choose working directory", "working directory to resume",
                    "use the current directory", "use session dir")

    same = plain(render([CX_VENDOR, "--no-alt-screen", "resume", tid], env, cwd,
                        seconds=20, wake=b"\r"))
    check("cx-resume-renders", painted(same, MARKER),
          "the resumed session's own turns are on screen")
    check("cx-picker-quiet-on-matching-cwd",
          not [w for w in picker_words if painted(same, w)],
          "no picker when the launch cwd is the recorded one")

    other = os.path.join(os.path.dirname(cwd), "elsewhere")
    os.makedirs(other, exist_ok=True)
    diff = plain(render([CX_VENDOR, "--no-alt-screen", "resume", tid], env, other,
                        seconds=20, wake=b"\r"))
    fired = [w for w in picker_words if painted(diff, w)]
    check("cx-picker-fires-on-different-cwd", bool(fired),
          f"picker on a mismatched cwd: {fired}" if fired else
          "no picker even on a mismatched cwd — the override may be unnecessary")

    fixed = plain(render([CX_VENDOR, "--no-alt-screen", "resume", tid,
                          "-c", "tui.resume_cwd=current"], env, other,
                         seconds=20, wake=b"\r"))
    still = [w for w in picker_words if painted(fixed, w)]
    check("cx-resume-cwd-answers", not still,
          "the override answers the picker in the case that raises it"
          if not still else f"picker survived the override: {still}")

    if spend:
        before = cx_thread_ids(store)
        r = subprocess.run([CX_VENDOR, "exec", "resume", "--skip-git-repo-check", tid, TURN_2],
                           env=env, cwd=cwd, capture_output=True, text=True, timeout=300,
                           stdin=subprocess.DEVNULL)
        after = cx_thread_ids(store)
        grew = after.get(tid, 0) > before.get(tid, 0)
        new_threads = set(after) - set(before)
        check("cx-resume-same-thread", grew and not new_threads,
              f"items {before.get(tid,0)}->{after.get(tid,0)} new threads={len(new_threads)} "
              f"rc={r.returncode}")
    else:
        SKIPPED.append("cx-resume-same-thread")
        print("  SKIP  cx-resume-same-thread — needs --spend")
    return tid


# --- main -----------------------------------------------------------------------

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--spend", action="store_true", help="run the turns that cost money")
    ap.add_argument("--reuse", action="store_true", help="re-read a previous run, spend nothing")
    ap.add_argument("--only", choices=("cc", "cx"), help="one harness only")
    args = ap.parse_args()

    spend = args.spend or os.environ.get("FLEETOR_SPEND_OK") == "1"
    if not spend and not args.reuse:
        print(__doc__.split("Arms, PASS/FAIL:")[0])
        print("Refusing to spend. Pass --spend to create the sessions, or --reuse to")
        print("re-read a previous run for free.")
        return 0

    if args.reuse and not os.path.isdir(ROOT):
        print(f"--reuse: nothing at {ROOT} to re-read. Run with --spend first.")
        return 0
    os.makedirs(ROOT, exist_ok=True)  # never wiped: a bought session is reused

    print(f"reopen spike — {CC_BUILD} / {CX_BUILD}")
    print(f"scratch: {ROOT}   spend: {spend}")

    if args.only != "cx":
        if os.path.isfile(CC_VENDOR):
            run_claude_code(os.path.join(ROOT, "cc"), spend)
        else:
            print(f"\nSKIP claude-code — no vendor binary at {CC_VENDOR}")
            SKIPPED.append("claude-code")
    if args.only != "cc":
        if os.path.isfile(CX_VENDOR):
            run_codex(os.path.join(ROOT, "cx"), spend)
        else:
            print(f"\nSKIP codex — no vendor binary at {CX_VENDOR}")
            SKIPPED.append("codex")

    print()
    if FAILURES:
        print(f"{len(FAILURES)} arm(s) failed: {', '.join(FAILURES)}")
    else:
        print("all arms passed")
    if SKIPPED:
        print(f"skipped: {', '.join(SKIPPED)}")
    return len(FAILURES)


if __name__ == "__main__":
    sys.exit(main())
