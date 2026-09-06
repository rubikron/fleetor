#!/usr/bin/env python3
"""WP-25 (#49) spike, part 2: can the operator's plan credential be *handed in*
rather than copied to disk?

`probe.py` (sibling, do not edit) measured that a fenced pane cannot reach the
operator's login keychain at all: a private `HOME` removes the login keychain
from the process's keychain search list, so `CLAUDE_SECURESTORAGE_CONFIG_DIR`
has nothing to select from. Its §4 listed three routes out, all of which give
up something the Fence exists to hold. The narrowest of them planted a
credential *file* in the pane's config dir — a token on disk inside a worktree,
which is what D-062 forbids and what #29's byte-scan exists to catch.

The operator asked a better question: why copy it at all — can it not be read
once, **before the sandbox exists**, and handed to the pane in an environment
variable? Nothing is written, nothing survives the process, and the plumbing
already exists: `Credentials::token_env` is `ANTHROPIC_AUTH_TOKEN`, which is
exactly how a fenced worker receives the fleet's metered key today
(`spawn::worker_command_with`).

Two unknowns, and this probe settles both.

  Q1  Is the keychain value usable as a bearer token at all? OAuth credentials
      and API keys authenticate over different paths, so `ANTHROPIC_AUTH_TOKEN`
      accepting one says nothing about the other.
  Q2  How long is it good for? A token read at spawn goes stale mid-run, and a
      fenced pane cannot reach the keychain to refresh it. A pane that dies an
      hour in — looking healthy right until it does not — is D-062's failure
      mode delayed rather than fixed.

**Zero token spend by construction.** The pty arms never submit: they spawn an
interactive `claude`, watch the screen, and SIGKILL, exactly as `probe.py` does.
The one network arm sends a **deliberately malformed** `/v1/messages` body and
reads only the HTTP status — 401 means the credential was rejected, 400 means it
was accepted and the *body* was rejected. No completion is ever requested, so no
inference is billed.

**The credential is never written or printed.** Every arm reports lengths,
classes and outcomes. Nothing in this file's output, its logs, or its exit path
carries token material; the redaction is enforced in one place (`redact`) and the
env-dump the pty arms write is filtered through it.

The comparison set is `probe.py`'s measured arms, on the same machine:

    unfenced-plan   reads  `Opus 5 (1M context) · Claude Max`   (auth works)
    fenced-plan     reads  `API Usage Billing` + `Not logged in · Run /login`

Arms here:

    control-unfenced    the operator's posture, no token handed in. Re-proves the
                        keychain entry is live *right now*, which is what makes
                        every other arm mean anything. Must read a plan.
    control-fenced      the Fence with no token. `probe.py`'s `fenced-plan`
                        restated as this probe's negative control: must come back
                        logged out, or a green token arm proves nothing.
    fenced-oauth-access `ANTHROPIC_AUTH_TOKEN` = `claudeAiOauth.accessToken`, the
                        access token **alone**. The spelling the route needs.
    fenced-oauth-blob   `ANTHROPIC_AUTH_TOKEN` = the `claudeAiOauth` object as
                        JSON. The operator's question read literally — "hand in
                        the keychain value". Note this is the claudeAiOauth
                        object, NOT the whole keychain item: that item also holds
                        unrelated third-party MCP OAuth tokens (this machine has
                        two), and handing those to a fenced pane would leak
                        credentials that have nothing to do with FLEETOR.
    http-401-or-400     the access token against `api.anthropic.com` with a
                        malformed body. Zero-token, and it separates "the pane
                        stopped complaining" from "the server accepts this",
                        which the mode line alone cannot.

Usage:
    python3 probe_env_token.py --bin "$HOME/.local/bin/claude"
    python3 probe_env_token.py --arm expiry          # Q2 alone, no pty, instant
    python3 probe_env_token.py --arm http-401-or-400 # Q1 corroboration, instant

Exit code is the failure count, as the sibling tier probes do. A missing binary
or a missing keychain entry is a **loud skip**, not a failure.
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

# ---------------------------------------------------------------------------
# Redaction. Single point of enforcement: nothing else in this file may print a
# value derived from the credential without passing through here first.
# ---------------------------------------------------------------------------

_SECRETS: list[str] = []


def register_secret(value: str) -> None:
    """Every string that must never appear in output is registered here."""
    if value and len(value) >= 8:
        _SECRETS.append(value)


def redact(text: str) -> str:
    for s in sorted(_SECRETS, key=len, reverse=True):
        text = text.replace(s, "<REDACTED>")
    return text


def emit(line: str = "") -> None:
    print(redact(line))


def classify(token: str) -> str:
    """Describe a token without revealing it: length and family only.

    The family is read from the vendor's documented prefix grammar rather than
    from the token body — `sk-ant-oat*` is an OAuth access token, `sk-ant-api*`
    an API key — so the description is a *category*, never a fragment.
    """
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
# Reading the credential. Read-only: `find-generic-password -w` does not modify
# the item, and nothing here writes to the keychain or to ~/.claude.
# ---------------------------------------------------------------------------


def read_credential():
    """Return the parsed keychain item, or None if it is absent.

    This is the step that must happen **before the sandbox exists** for the
    route to work at all — an unfenced process, the operator's own HOME, their
    own login keychain on the search list.
    """
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
        # A bare string, not a blob. Register it and report the shape.
        register_secret(raw)
        return {"__bare__": raw}
    oauth = doc.get("claudeAiOauth")
    if isinstance(oauth, dict):
        for key in ("accessToken", "refreshToken"):
            if isinstance(oauth.get(key), str):
                register_secret(oauth[key])
    # Third-party MCP OAuth tokens share the item. Register them so that even an
    # accidental dump cannot spill them, and never hand them to a pane.
    for entry in (doc.get("mcpOAuth") or {}).values():
        if isinstance(entry, dict):
            for key in ("accessToken", "refreshToken", "clientSecret"):
                if isinstance(entry.get(key), str):
                    register_secret(entry[key])
    return doc


# ---------------------------------------------------------------------------
# Q2 — lifetime.
# ---------------------------------------------------------------------------


def arm_expiry(doc) -> dict:
    """Report the credential's shape and remaining lifetime. No values."""
    if "__bare__" in doc:
        return {
            "arm": "expiry",
            "ok": True,
            "shape": "bare string, not a blob",
            "notes": ["no expiry carried; nothing to compute"],
        }

    oauth = doc.get("claudeAiOauth")
    if not isinstance(oauth, dict):
        return {"arm": "expiry", "ok": False, "shape": "no claudeAiOauth key", "notes": []}

    now = time.time()
    notes = []
    fields = sorted(oauth.keys())
    access = oauth.get("accessToken", "")
    refresh = oauth.get("refreshToken", "")

    notes.append(f"fields: {', '.join(fields)}")
    notes.append(f"accessToken: {classify(access)}")
    notes.append(
        f"refreshToken: {'present — ' + classify(refresh) if refresh else 'ABSENT'}"
    )
    if oauth.get("subscriptionType"):
        notes.append(f"subscriptionType: {oauth['subscriptionType']}")
    if oauth.get("scopes"):
        notes.append(f"scopes: {', '.join(oauth['scopes'])}")

    lifetimes = {}
    for key, label in (
        ("expiresAt", "access token"),
        ("refreshTokenExpiresAt", "refresh token"),
    ):
        raw = oauth.get(key)
        if not isinstance(raw, (int, float)):
            notes.append(f"{key}: absent")
            continue
        # Epoch milliseconds; the vendor writes ms and a seconds reading would be
        # off by three orders of magnitude, which is the kind of error that reads
        # as "expired in 1970" rather than as a bug.
        when = raw / 1000.0
        remaining = when - now
        lifetimes[key] = remaining
        stamp = time.strftime("%Y-%m-%d %H:%M:%S %Z", time.localtime(when))
        if remaining < 0:
            notes.append(f"{key}: {stamp} — EXPIRED {abs(remaining) / 3600:.2f} h ago")
        else:
            notes.append(
                f"{key}: {stamp} — {remaining / 3600:.2f} h "
                f"({remaining / 86400:.2f} d) remaining ({label})"
            )

    return {
        "arm": "expiry",
        "ok": True,
        "shape": "JSON blob (OAuth credential), not a bare token",
        "notes": notes,
        "lifetimes": lifetimes,
    }


# ---------------------------------------------------------------------------
# Q1, corroboration — does the server accept it as a bearer token?
# ---------------------------------------------------------------------------


def arm_http(doc) -> dict:
    """Zero-token acceptance check.

    A **malformed** `/v1/messages` body: no model, no messages, nothing that
    could be billed. The status separates the two answers the mode line cannot:

        401  the credential was rejected — the route does not work
        400  the credential was accepted and the *body* was rejected — it does

    Sent to `api.anthropic.com`, the credential's own issuer, and nowhere else.
    The token is passed through the environment so it never appears in the
    process table or in this file's output.
    """
    oauth = doc.get("claudeAiOauth") or {}
    access = oauth.get("accessToken") or doc.get("__bare__")
    if not access:
        return {"arm": "http-401-or-400", "ok": False, "notes": ["no access token to test"]}

    env = dict(os.environ)
    env["PROBE_TOKEN"] = access
    results = []
    # Two spellings, because OAuth and API keys ride different headers. If the
    # bearer spelling is the one that works, that is the spelling
    # `ANTHROPIC_AUTH_TOKEN` already uses, which is the whole point.
    for label, header in (
        ("Authorization: Bearer (what ANTHROPIC_AUTH_TOKEN sets)", "Authorization: Bearer $PROBE_TOKEN"),
        ("x-api-key (what ANTHROPIC_API_KEY sets)", "x-api-key: $PROBE_TOKEN"),
    ):
        cmd = [
            "curl", "-s", "-o", "/dev/null", "-w", "%{http_code}",
            "-X", "POST", "https://api.anthropic.com/v1/messages",
            "-H", header,
            "-H", "anthropic-version: 2023-06-01",
            "-H", "anthropic-beta: oauth-2025-04-20",
            "-H", "content-type: application/json",
            # Deliberately invalid: no model, no messages, no max_tokens.
            "--data", '{"probe":"malformed-on-purpose"}',
            "--max-time", "20",
        ]
        try:
            out = subprocess.run(cmd, capture_output=True, text=True, env=env, timeout=30)
            code = out.stdout.strip() or "no-response"
        except (OSError, subprocess.TimeoutExpired):
            code = "network-error"
        verdict = {
            "401": "REJECTED — credential not accepted on this path",
            "403": "REJECTED — accepted but forbidden for this use",
            "400": "ACCEPTED — auth passed, body rejected as designed",
        }.get(code, "inconclusive")
        results.append(f"{label}: HTTP {code} — {verdict}")

    accepted = any(" 400 " in f" {r} " or "HTTP 400" in r for r in results)
    return {
        "arm": "http-401-or-400",
        "ok": accepted,
        "notes": results,
        "accepted": accepted,
    }


# ---------------------------------------------------------------------------
# Q1, primary — the pty arms. Lifted from `probe.py` so the two are comparable.
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
    env.pop("PROBE_TOKEN", None)
    return env


def arm_env(name: str, config_dir: Path, home: Path, doc) -> dict:
    env = base_env()
    env["FLEETOR_PANE"] = "orch" if name == "control-unfenced" else "worker-1"
    env["CLAUDE_CONFIG_DIR"] = str(config_dir)

    if name == "control-unfenced":
        # `spawn::orch_command_with`: the operator's own HOME and PATH, the
        # config dir, the credential namespace. No Fence, no token handed in.
        env["CLAUDE_SECURESTORAGE_CONFIG_DIR"] = ""
        return env

    # Every remaining arm is fenced, exactly as `worker_command_with` builds it.
    env["HOME"] = str(home)
    env["PATH"] = worker_path(env.get("PATH", ""))
    # The Fence scrubs the credential namespace; these arms keep that scrub,
    # because the whole point is that the credential arrives in the environment
    # instead of through the keychain.
    env.pop("CLAUDE_SECURESTORAGE_CONFIG_DIR", None)

    oauth = doc.get("claudeAiOauth") or {}
    if name == "control-fenced":
        pass
    elif name == "fenced-oauth-access":
        env["ANTHROPIC_AUTH_TOKEN"] = oauth.get("accessToken") or doc.get("__bare__", "")
    elif name == "fenced-oauth-blob":
        # The operator's question read literally, minus the third-party MCP
        # tokens that share the keychain item and must not reach a pane.
        env["ANTHROPIC_AUTH_TOKEN"] = json.dumps(oauth)
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
    config_dir = work / f"envtok-config-{name}"
    if config_dir.exists():
        shutil.rmtree(config_dir)
    config_dir.mkdir(parents=True)
    seed_config_dir(config_dir, cwd)

    home = work / f"envtok-home-{name}"
    if home.exists():
        shutil.rmtree(home)
    seed_worker_home(home)

    env = arm_env(name, config_dir, home, doc)
    raw = spawn_and_watch(cwd, env, args.seconds, args.rows, args.cols, args.bin)
    plain = strip_ansi(raw.decode("utf-8", errors="replace"))
    # The screen can echo an env var back; the log is redacted before it lands.
    (work / f"envtok-plain-{name}.log").write_text(redact(plain))

    banner = [redact(l.strip()) for l in plain.splitlines()
              if "Claude Code v" in l or "·" in l]
    plan = scan(plain, PLAN_MARKERS)
    login = scan(plain, LOGIN_MARKERS)
    prompt = scan(plain, PROMPT_MARKERS)

    # What "ok" means differs per arm, and saying so out loud is the point: a
    # control that passes by reading *logged out* is doing its job.
    if name == "control-unfenced":
        ok = bool(plan) and not login
        expectation = "must read a plan — proves the keychain entry is live now"
    elif name == "control-fenced":
        ok = bool(login) and not plan
        expectation = "must read logged out — the negative control"
    else:
        ok = bool(plan) and not login
        expectation = "reads a plan if the handed-in credential authenticates"

    token_desc = "<none handed in>"
    if "ANTHROPIC_AUTH_TOKEN" in env:
        val = env["ANTHROPIC_AUTH_TOKEN"]
        token_desc = (
            f"claudeAiOauth object as JSON ({len(val)} chars)"
            if val.startswith("{") else classify(val)
        )

    return {
        "arm": name,
        "ok": ok,
        "expectation": expectation,
        "home": "<operator's own>" if name == "control-unfenced" else str(env["HOME"]),
        "token": token_desc,
        "bytes": len(raw),
        "prompt": prompt or ["NOT REACHED"],
        "onboarding": scan(plain, ONBOARDING_MARKERS) or ["none"],
        "login": login or ["none"],
        "plan": plan or ["none"],
        "metered": scan(plain, METERED_MARKERS) or ["none"],
        "banner": banner[:5],
    }


PTY_ARMS = ["control-unfenced", "control-fenced", "fenced-oauth-access", "fenced-oauth-blob"]
ARMS = ["expiry", "http-401-or-400", *PTY_ARMS]


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
    pty_wanted = [a for a in wanted if a in PTY_ARMS]
    if pty_wanted and not binary.is_file():
        emit(f"SKIP: no `claude` binary at {binary}; skipping {len(pty_wanted)} pty arm(s).")
        emit("      Pass --bin to point at the real binary (the PATH one may be a shim, C47).")
        wanted = [a for a in wanted if a not in PTY_ARMS]

    cwd = args.cwd.expanduser()
    cwd.mkdir(parents=True, exist_ok=True)
    cwd = cwd.resolve()
    args.bin = str(binary.resolve()) if binary.is_file() else args.bin

    failures = 0
    for name in wanted:
        if name == "expiry":
            r = arm_expiry(doc)
        elif name == "http-401-or-400":
            r = arm_http(doc)
        else:
            print(f"\n[probe] arm={name} cwd={cwd} bin={args.bin}", file=sys.stderr)
            r = run_pty_arm(name, cwd, work, args, doc)

        emit("=" * 76)
        emit(f"{'PASS' if r.get('ok') else 'FAIL'}  arm: {r['arm']}")
        if r.get("expectation"):
            emit(f"      expected      : {r['expectation']}")
        for key in ("shape", "home", "token", "bytes"):
            if key in r:
                emit(f"      {key:<14}: {r[key]}")
        for key in ("prompt", "onboarding", "login", "plan", "metered"):
            if key in r:
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
