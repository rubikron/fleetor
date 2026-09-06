#!/usr/bin/env python3
"""Every vendor-behaviour claim in issue #13, re-measurable in one command.

    python3 examples/codex-spike/probe.py

Measured against `codex-cli 0.153.4`. Spends **zero tokens** — that is the whole
point (C13). The model provider is a local capture server that answers every
request with `data: [DONE]`, so nothing ever reaches a real endpoint.

Six arms, each printing PASS/FAIL against what C3, C5, C7 and C22 recorded:

  1. brief-replaces  `model_instructions_file` replaces the system prompt (C3)
  2. brief-rejected  `base_instructions` in a custom catalog is NOT honoured (C3)
  3. agents-md       `AGENTS.md` arrives as a `user` message, in-band (C3)
  4. fence           a write outside the workspace is refused (C7)
  5. socket          refused by default; `network_access=true` lifts it (C5)
  6. typing          bracketed paste + CR submits, and needs timing (C22)

Arms 1-3 and 6 read the literal request body. Arms 4-5 use `codex sandbox`,
which runs a command under the real seatbelt with no model involved.

Exit code is the number of failed arms, so CI can gate on it. Skips cleanly
(exit 0, loud message) when `codex` is not on PATH — C13's stated rule.
"""

import http.server, json, os, pty, select, shutil, signal, subprocess, sys, tempfile, threading, time

BUILD = "codex-cli 0.153.4"
PORT = 8731
# AF_UNIX paths are capped near 104 bytes, so the scratch root cannot live under
# a long temp path. This is the one reason the probe does not use tempfile alone.
ROOT = "/tmp/codex-spike"
CAPTURED = os.path.join(ROOT, "captured.json")

FAILURES = []


def check(arm, ok, detail=""):
    print(f"  {'PASS' if ok else 'FAIL'}  {arm}{'  — ' + detail if detail else ''}")
    if not ok:
        FAILURES.append(arm)


# --- the capture server: a model provider that records and says nothing --------


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_POST(self):
        n = int(self.headers.get("Content-Length", "0"))
        open(CAPTURED, "wb").write(self.rfile.read(n))
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        self.wfile.write(b"data: [DONE]\n\n")


def serve():
    http.server.HTTPServer(("127.0.0.1", PORT), Handler).serve_forever()


# --- scratch installation -----------------------------------------------------


def build_scratch():
    """A CODEX_HOME pointing at the capture server, and a worktree with AGENTS.md.

    The catalog entry is cloned from the operator's own so every required field
    is present — a hand-written entry fails with `missing field shell_type`.
    """
    shutil.rmtree(ROOT, ignore_errors=True)
    home, wt = os.path.join(ROOT, "home"), os.path.join(ROOT, "wt")
    os.makedirs(home), os.makedirs(wt)

    catalog = json.loads(
        subprocess.run(["codex", "debug", "models"], capture_output=True, text=True).stdout
    )
    entry = dict(catalog["models"][0])
    entry.update(slug="probe-model", display_name="Probe")
    entry["base_instructions"] = "SENTINEL-BASEINSTR\nnot honoured"
    json.dump({"models": [entry]}, open(os.path.join(home, "models.json"), "w"))

    open(os.path.join(home, "config.toml"), "w").write(
        f'model = "probe-model"\n'
        f'model_provider = "probe"\n'
        f'model_catalog_json = "{home}/models.json"\n'
        f"\n[model_providers.probe]\n"
        f'name = "probe"\n'
        f'base_url = "http://127.0.0.1:{PORT}"\n'
        f'wire_api = "responses"\n'
        f'experimental_bearer_token = "sk-probe"\n'
    )
    open(os.path.join(home, "brief.md"), "w").write("SENTINEL-MODELINSTR\nYou are a FLEETOR worker pane.")
    open(os.path.join(wt, "AGENTS.md"), "w").write("SENTINEL-AGENTSMD\n")
    return home, wt


def turn(home, wt, extra=()):
    """One non-interactive turn. Returns the captured request body, or None."""
    if os.path.exists(CAPTURED):
        os.unlink(CAPTURED)
    env = {**os.environ, "CODEX_HOME": home}
    subprocess.run(
        ["codex", "exec", "--skip-git-repo-check", *extra],
        input="hi\n", text=True, cwd=wt, env=env,
        capture_output=True, timeout=60,
    )
    return json.load(open(CAPTURED)) if os.path.exists(CAPTURED) else None


# --- arms ---------------------------------------------------------------------


def arm_brief(home, wt):
    """C3: the carrier replaces the prompt; the two rejected carriers behave as recorded."""
    body = turn(home, wt, ["-c", f"model_instructions_file={home}/brief.md"])
    if body is None:
        return check("brief-replaces", False, "no request reached the provider")
    ins, whole = body.get("instructions", ""), json.dumps(body)
    check("brief-replaces", "SENTINEL-MODELINSTR" in ins and "You are Codex" not in ins,
          f"instructions={len(ins)} chars")
    check("brief-rejected", "SENTINEL-BASEINSTR" not in whole,
          "base_instructions stayed a descriptor")
    # AGENTS.md is in-band on the SAME request — that is the finding, not a separate run.
    roles = [i.get("role") for i in body.get("input", []) if isinstance(i, dict)]
    check("agents-md", "SENTINEL-AGENTSMD" in whole and "user" in roles,
          f"input roles={roles}")


def arm_fence(wt):
    """C7: workspace-write refuses a write outside the workspace."""
    r = subprocess.run(
        ["codex", "sandbox", "-c", "sandbox_mode=workspace-write",
         "sh", "-c", f'echo x > {os.path.expanduser("~")}/.codex-spike-escape'],
        cwd=wt, capture_output=True, text=True, timeout=60,
    )
    escaped = os.path.exists(os.path.expanduser("~/.codex-spike-escape"))
    if escaped:
        os.unlink(os.path.expanduser("~/.codex-spike-escape"))
    check("fence", not escaped, (r.stderr or "").strip().splitlines()[-1:] and
          (r.stderr).strip().splitlines()[-1] or "no file created")


def arm_socket(wt):
    """C5: the socket is refused by default and reachable with one key — both halves."""
    sock, conn = os.path.join(ROOT, "f.sock"), os.path.join(ROOT, "connect.py")
    open(conn, "w").write(
        "import socket,sys\n"
        "s=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)\n"
        "try:\n s.connect(sys.argv[1]); s.sendall(b'HI'); print('CONNECTED')\n"
        "except Exception as e: print('REFUSED',e)\n"
    )

    def listen():
        import socket
        try:
            os.unlink(sock)
        except FileNotFoundError:
            pass
        srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        srv.bind(sock), srv.listen(1)
        srv.accept()[0].recv(16)

    for label, extra, want in (
        ("socket-refused-by-default", [], "REFUSED"),
        ("socket-lifted-by-network-access",
         ["-c", "sandbox_workspace_write.network_access=true"], "CONNECTED"),
    ):
        threading.Thread(target=listen, daemon=True).start()
        time.sleep(0.5)
        out = subprocess.run(
            ["codex", "sandbox", "-c", "sandbox_mode=workspace-write", *extra,
             "python3", conn, sock],
            cwd=wt, capture_output=True, text=True, timeout=60,
        ).stdout
        check(label, want in out, out.strip())


def arm_typing(home, wt):
    """C22: a bracketed paste + CR submits — and the timing is load-bearing.

    Both timings are run deliberately. The fast one FAILING is the finding: it is
    what makes codex the first harness that owes M6 a real profile. Fixed sleeps
    are what the phase-2 spike replaces with readiness detection.
    """
    for label, startup, delay, expect in (
        ("typing-fast-does-not-submit", 6.0, 0.6, False),
        ("typing-tuned-submits", 12.0, 1.5, True),
    ):
        if os.path.exists(CAPTURED):
            os.unlink(CAPTURED)
        pid, fd = pty.fork()
        if pid == 0:
            os.chdir(wt)
            os.execve(shutil.which("codex"), ["codex", "--no-alt-screen"],
                      {**os.environ, "CODEX_HOME": home,
                       "TERM": "xterm-256color", "COLORTERM": "truecolor"})

        def drain(t):
            end = time.time() + t
            while time.time() < end:
                if select.select([fd], [], [], 0.2)[0]:
                    try:
                        os.read(fd, 65536)
                    except OSError:
                        return

        drain(startup)
        os.write(fd, b"\x1b[200~PTY-SENTINEL say ok\x1b[201~")
        time.sleep(delay)
        os.write(fd, b"\r")
        drain(8.0)
        os.kill(pid, signal.SIGTERM)
        os.waitpid(pid, 0)
        submitted = os.path.exists(CAPTURED)
        check(label, submitted == expect, "submitted" if submitted else "not submitted")


def main():
    if shutil.which("codex") is None:
        print(f"SKIP: codex is not on PATH. This tier is measured against {BUILD}.")
        return 0
    actual = subprocess.run(["codex", "--version"], capture_output=True, text=True).stdout.strip()
    print(f"codex-spike — recorded against {BUILD}, running against {actual}\n")
    if actual != BUILD:
        print(f"  NOTE: build differs from the recorded one. A failure below may be drift,\n"
              f"        not a regression — re-record rather than patching around it.\n")

    threading.Thread(target=serve, daemon=True).start()
    home, wt = build_scratch()
    time.sleep(0.5)

    arm_brief(home, wt)
    arm_fence(wt)
    arm_socket(wt)
    arm_typing(home, wt)

    print(f"\n{len(FAILURES)} failed" + (f": {', '.join(FAILURES)}" if FAILURES else ""))
    return len(FAILURES)


if __name__ == "__main__":
    sys.exit(main())
