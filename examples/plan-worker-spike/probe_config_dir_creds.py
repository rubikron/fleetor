#!/usr/bin/env python3
"""WP-25 (#49) spike, part 3: can a fenced pane authenticate on the operator's
**existing** login, with no extra step asked of the operator?

`probe.py` (sibling, do not edit) measured that a private `HOME` removes the
operator's login keychain from the process's keychain search list, so a fenced
pane cannot reach the credential at all. `probe_env_token.py` (sibling, do not
edit) measured that handing the value in through `ANTHROPIC_AUTH_TOKEN` fails
twice over: that variable selects the metered path, and the access token it
would carry has under an hour of life.

This probe measures the two channels neither of those touched. Both use the
credential the operator already has — nothing here asks them to run
`claude setup-token`, or anything else.

  Q3  Does `claude` read a credential file out of `CLAUDE_CONFIG_DIR` on macOS,
      or is the keychain the only store it will look in? **This is #49's spike 1
      and it gates route 3.** If the file is never read, route 3 dies with route
      4 and the answer is defer. The vendor's bundle carries a file-backed store
      whose path is `<storage dir>/.credentials.json`; what is *not* established
      by reading is whether that store is reachable on darwin, or whether its
      storage dir is `CLAUDE_CONFIG_DIR`. Only running it says.

  Q4  Does `CLAUDE_CODE_OAUTH_TOKEN` — a *different* variable from the one
      `probe_env_token.py` measured — accept the operator's existing access
      token and put the pane on the plan? The vendor's own `byoc` runner sets it
      on a child `claude`, and `/login` warns when it is set, so it is an
      OAuth-aware override rather than the metered path. C71's blocker 1 was
      measured against `ANTHROPIC_AUTH_TOKEN` and does not speak to this one.
      Blocker 2 — the sub-hour lifetime — still stands here and is why this arm
      cannot be the whole answer even if it reads green.

  Q5  If the file *is* read, does the pane refresh it itself? The file carries
      the refresh token (17.9 d when `probe_env_token.py --arm expiry` measured
      it), so a pane that renews in place needs no machinery outside the Fence.
      This probe reports whether the file changed under the pane; it does not
      wait out an expiry, so a `no` here means "not within the watch window",
      never "cannot".

**Zero token spend by construction.** Every arm spawns an interactive `claude`
in a pty, watches the screen, and SIGKILLs. Nothing is ever submitted, so no
inference is requested and none is billed.

**The credential is never written outside a disposable arm directory, and never
printed.** Redaction is enforced in one place (`redact`); every print path goes
through it, and the pty transcripts are filtered before they land. The arm
directories live under `work/`, which is gitignored, and are deleted and
recreated on every run. The operator's own `~/.claude` and their keychain are
read only — `find-generic-password -w` does not modify the item.

**What this probe deliberately does not hand over.** The keychain item holds
more than Claude Code's own credential; on this machine it also carries
third-party MCP OAuth tokens with their own refresh tokens and a client secret.
Every arm here narrows to the `claudeAiOauth` object, never the item.

Usage:
    python3 probe_config_dir_creds.py --bin "$HOME/.local/bin/claude"
    python3 probe_config_dir_creds.py --bin "$HOME/.local/bin/claude" --arm fenced-creds-file

Exit code is the failure count, as the sibling probes do. A missing binary or a
missing keychain entry is a **loud skip**, not a failure.
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

KEYCHAIN_SERVICE = "Claude Code-credentials"
CREDENTIALS_FILE = ".credentials.json"

# ---------------------------------------------------------------------------
# Redaction. Single point of enforcement: nothing else in this file may print a
# value derived from the credential without passing through here first.
# ---------------------------------------------------------------------------

_SECRETS: list[str] = []


def register_secret(value: str) -> None:
    if value and len(value) >= 8:
        _SECRETS.append(value)


def redact(text: str) -> str:
    for s in sorted(_SECRETS, key=len, reverse=True):
        text = text.replace(s, "<REDACTED>")
    return text


def emit(line: str = "") -> None:
    print(redact(line))


def classify(token: str) -> str:
    """Describe a token without revealing it: length and family only."""
    for prefix, family in (
        ("sk-ant-oat", "OAuth access token"),
        ("sk-ant-ort", "OAuth refresh token"),
        ("sk-ant-api", "API key"),
        ("sk-ant-", "Anthropic credential, unrecognised family"),
    ):
        if token.startswith(prefix):
            return f"{family} ({len(token)} chars)"
    return f"unrecognised prefix ({len(token)} chars)"


# ---------------------------------------------------------------------------
# Reading the credential. Read-only, and it happens here — in an unfenced
# process, on the operator's own HOME — because that is the one place it can.
# ---------------------------------------------------------------------------


def read_credential():
    try:
        out = subprocess.run(
            ["security", "find-generic-password", "-s", KEYCHAIN_SERVICE, "-w"],
            capture_output=True,
            text=True,
            timeout=20,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    if out.returncode != 0:
        return None
    raw = out.stdout.strip()
    if not raw:
        return None
    try:
        doc = json.loads(raw)
    except json.JSONDecodeError:
        register_secret(raw)
        return {"__bare__": raw}
    oauth = doc.get("claudeAiOauth")
    if isinstance(oauth, dict):
        for key in ("accessToken", "refreshToken"):
            if isinstance(oauth.get(key), str):
                register_secret(oauth[key])
    for entry in (doc.get("mcpOAuth") or {}).values():
        if isinstance(entry, dict):
            for key in ("accessToken", "refreshToken", "clientSecret"):
                if isinstance(entry.get(key), str):
                    register_secret(entry[key])
    return doc


# ---------------------------------------------------------------------------
# Screen markers. Lifted from the sibling probes so all three are comparable.
# ---------------------------------------------------------------------------

ONBOARDING_MARKERS = [
    (r"choosethetextstyle|darkmode|lightmode", "theme picker"),
    (r"trustthefiles|doyoutrust|proceedwiththefiles", "trust dialog"),
    (r"usethisapikey|detectedacustomapikey|customapikey", "api-key approval"),
    (r"letsgetstarted|let'sgetstarted|securitynotes|presstocontinue", "welcome/intro"),
    (r"termsofservice|usagepolicy", "terms"),
]
LOGIN_MARKERS = [
    (r"signinwith|loginwith|invalidapikey|notloggedin|pleaserun/login|claude\.ai/login",
     "LOGIN REQUIRED"),
]
PROMPT_MARKERS = [
    (r"\?forshortcuts|/helpforhelp|tryedit<filepath>", "input box ready"),
]
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


# ---------------------------------------------------------------------------
# Arm setup. `seed_config_dir` and `seed_worker_home` mirror FLEETOR's own
# `spawn::` functions exactly, so an arm differs from a real pane only in the
# one thing it is measuring.
# ---------------------------------------------------------------------------


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


def plant_credentials(config_dir: Path, doc) -> dict:
    """Write the `claudeAiOauth` object as the pane's own credential file.

    This is the write route 3 would perform at spawn, in the one place a fenced
    pane can still reach. Mode 0600, matching what the vendor's own file store
    chmods its writes to. **Only `claudeAiOauth`** — the keychain item's
    third-party MCP tokens are not this pane's business and never leave here.
    """
    oauth = doc.get("claudeAiOauth")
    if not isinstance(oauth, dict):
        return {"planted": False, "why": "keychain item carries no claudeAiOauth object"}
    path = config_dir / CREDENTIALS_FILE
    path.write_text(json.dumps({"claudeAiOauth": oauth}))
    os.chmod(path, 0o600)
    stat = path.stat()
    return {
        "planted": True,
        "path": str(path),
        "mode": oct(stat.st_mode & 0o777),
        "bytes": stat.st_size,
        "mtime_ns": stat.st_mtime_ns,
        "digest_before": _digest(path),
    }


def _digest(path: Path) -> str:
    """A content fingerprint that is not the content — for change detection only."""
    import hashlib

    return hashlib.sha256(path.read_bytes()).hexdigest()[:16]


def worker_path(existing: str) -> str:
    """`spawn::worker_augmented_path_from` with no fleet bin and no cargo home."""
    return f"/opt/homebrew/bin:/usr/local/bin:{existing}"


def base_env() -> dict:
    env = dict(os.environ)
    env["TERM"] = "xterm-256color"
    env["COLORTERM"] = "truecolor"
    env.pop("CLAUDE_CODE_CHILD_SESSION", None)
    # This probe runs *inside* a Claude Code session; none of its wiring may
    # reach the child or the arms measure the parent rather than the pane.
    for name in list(env):
        if name.startswith("CLAUDE_CODE_") or name.startswith("CMUX_"):
            env.pop(name, None)
    env.pop("CLAUDE_SECURESTORAGE_CONFIG_DIR", None)
    env.pop("CLAUDE_CONFIG_DIR", None)
    env.pop("ANTHROPIC_API_KEY", None)
    env.pop("ANTHROPIC_AUTH_TOKEN", None)
    return env


def arm_env(name: str, config_dir: Path, home: Path, doc) -> dict:
    env = base_env()
    env["FLEETOR_PANE"] = "orch" if name == "control-unfenced" else "worker-1"
    env["CLAUDE_CONFIG_DIR"] = str(config_dir)

    if name == "control-unfenced":
        # `spawn::orch_command_with`: the operator's own HOME and PATH, the
        # config dir, the credential namespace. No Fence.
        env["CLAUDE_SECURESTORAGE_CONFIG_DIR"] = ""
        return env

    # Every remaining arm is fenced, exactly as `worker_command_with` builds it.
    env["HOME"] = str(home)
    env["PATH"] = worker_path(env.get("PATH", ""))
    env.pop("CLAUDE_SECURESTORAGE_CONFIG_DIR", None)

    if name in ("control-fenced", "fenced-creds-file"):
        # `fenced-creds-file` differs from its control by the planted file
        # alone — nothing is added to its environment, which is the point.
        pass
    elif name == "fenced-oauth-env":
        oauth = doc.get("claudeAiOauth") or {}
        env["CLAUDE_CODE_OAUTH_TOKEN"] = oauth.get("accessToken") or doc.get("__bare__", "")
    else:
        raise SystemExit(f"unknown arm {name}")
    return env


def spawn_and_watch(cwd: Path, env, seconds, rows, cols, binary: str) -> bytes:
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


def run_pty_arm(name: str, cwd: Path, work: Path, args, doc) -> dict:
    config_dir = work / f"cfgcred-config-{name}"
    if config_dir.exists():
        shutil.rmtree(config_dir)
    config_dir.mkdir(parents=True)
    seed_config_dir(config_dir, cwd)

    home = work / f"cfgcred-home-{name}"
    if home.exists():
        shutil.rmtree(home)
    seed_worker_home(home)

    planted = {"planted": False}
    if name == "fenced-creds-file":
        planted = plant_credentials(config_dir, doc)

    env = arm_env(name, config_dir, home, doc)
    raw = spawn_and_watch(cwd, env, args.seconds, args.rows, args.cols, args.bin)
    plain = strip_ansi(raw.decode("utf-8", errors="replace"))
    (work / f"cfgcred-plain-{name}.log").write_text(redact(plain))

    banner = [redact(l.strip()) for l in plain.splitlines()
              if "Claude Code v" in l or "·" in l]
    plan = scan(plain, PLAN_MARKERS)
    login = scan(plain, LOGIN_MARKERS)
    prompt = scan(plain, PROMPT_MARKERS)

    notes = []

    # Q5 — did the pane rewrite the file under itself? Only meaningful on the
    # arm that planted one, and a `no` is bounded by the watch window.
    cred_path = config_dir / CREDENTIALS_FILE
    if planted.get("planted"):
        if not cred_path.is_file():
            notes.append("credential file GONE after the run — the pane deleted it")
        else:
            after = _digest(cred_path)
            if after != planted["digest_before"]:
                notes.append(
                    f"credential file REWRITTEN by the pane within {args.seconds:.0f}s "
                    "— it holds the store itself, so it can self-refresh"
                )
            else:
                notes.append(
                    f"credential file unchanged within {args.seconds:.0f}s — bounded by "
                    "the watch window, NOT evidence the pane cannot refresh"
                )
    elif cred_path.is_file():
        # Any arm that did not plant one and has one now wrote its own.
        notes.append("pane WROTE a credential file into its own config dir unprompted")

    if name == "control-unfenced":
        ok = bool(plan) and not login
        expectation = "must read a plan — proves the keychain entry is live now"
    elif name == "control-fenced":
        ok = bool(login) and not plan
        expectation = "must read logged out — the negative control"
    elif name == "fenced-creds-file":
        ok = bool(plan) and not login
        expectation = "reads a plan if CLAUDE_CONFIG_DIR is a store claude will read (Q3)"
    else:
        ok = bool(plan) and not login
        expectation = "reads a plan if CLAUDE_CODE_OAUTH_TOKEN takes this credential (Q4)"

    handed = "<nothing handed in>"
    if "CLAUDE_CODE_OAUTH_TOKEN" in env:
        handed = f"CLAUDE_CODE_OAUTH_TOKEN = {classify(env['CLAUDE_CODE_OAUTH_TOKEN'])}"
    elif planted.get("planted"):
        handed = (
            f"{CREDENTIALS_FILE} in CLAUDE_CONFIG_DIR "
            f"({planted['bytes']} bytes, mode {planted['mode']})"
        )
    elif planted.get("why"):
        handed = f"<not planted: {planted['why']}>"

    return {
        "arm": name,
        "ok": ok,
        "expectation": expectation,
        "home": "<operator's own>" if name == "control-unfenced" else str(env["HOME"]),
        "handed_in": handed,
        "bytes": len(raw),
        "prompt": prompt or ["NOT REACHED"],
        "onboarding": scan(plain, ONBOARDING_MARKERS) or ["none"],
        "login": login or ["none"],
        "plan": plan or ["none"],
        "metered": scan(plain, METERED_MARKERS) or ["none"],
        "notes": notes,
        "banner": banner[:5],
    }


ARMS = ["control-unfenced", "control-fenced", "fenced-creds-file", "fenced-oauth-env"]


def main():
    work = Path(__file__).resolve().parent / "work"
    work.mkdir(parents=True, exist_ok=True)

    p = argparse.ArgumentParser()
    p.add_argument("--cwd", type=Path, default=Path.home() / ".fleetor" / "testbed")
    p.add_argument("--arm", default="all", choices=["all", *ARMS])
    p.add_argument("--bin", default=str(Path.home() / ".local" / "bin" / "claude"))
    p.add_argument("--seconds", type=float, default=12.0)
    p.add_argument("--rows", type=int, default=40)
    p.add_argument("--cols", type=int, default=120)
    args = p.parse_args()

    wanted = ARMS if args.arm == "all" else [args.arm]

    doc = read_credential()
    if doc is None:
        emit("SKIP: no `%s` entry in the login keychain." % KEYCHAIN_SERVICE)
        emit("      Nothing to hand in; this probe measures a credential it did not create.")
        return 0

    binary = Path(args.bin).expanduser()
    if not binary.is_file():
        emit(f"SKIP: no `claude` binary at {binary}.")
        emit("      Pass --bin to point at the real binary (the PATH one may be a shim, C47).")
        return 0

    cwd = args.cwd.expanduser()
    cwd.mkdir(parents=True, exist_ok=True)
    cwd = cwd.resolve()
    args.bin = str(binary.resolve())

    failures = 0
    for name in wanted:
        print(f"\n[probe] arm={name} cwd={cwd} bin={args.bin}", file=sys.stderr)
        r = run_pty_arm(name, cwd, work, args, doc)

        emit("=" * 76)
        emit(f"{'PASS' if r.get('ok') else 'FAIL'}  arm: {r['arm']}")
        emit(f"      expected      : {r['expectation']}")
        for key in ("home", "handed_in", "bytes"):
            emit(f"      {key:<14}: {r[key]}")
        for key in ("prompt", "onboarding", "login", "plan", "metered"):
            emit(f"      {key:<14}: {r[key]}")
        for note in r.get("notes", []):
            emit(f"      · {note}")
        if r.get("banner"):
            emit("      banner        :")
            for line in r["banner"]:
                emit(f"          {line}")
        emit("=" * 76)
        if not r.get("ok"):
            failures += 1

    emit(f"\n{len(wanted) - failures}/{len(wanted)} arms passed.")
    return failures


if __name__ == "__main__":
    sys.exit(main())
