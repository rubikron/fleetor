#!/usr/bin/env python3
"""Does a codex `PreToolUse` hook fire *untrusted* — and does `-c` really clobber one?

    python3 examples/codex-spike/hook_probe.py

The WP-25 P2 measurement issue #45 was gated on, and a **sibling** of `probe.py` and
`trust_probe.py` rather than an edit to either (C13). Measured against
`codex-cli 0.153.4`. Spends **zero tokens**.

## Why this needed a new instrument

`trust_probe.py` could answer its question by watching the first-run gate render into a
pty — no turn ever completed. This question cannot be answered that way: a `PreToolUse`
hook only fires when a **tool call actually happens**, and a tool call only happens when
a model emits one. Watching the pty is not enough and neither is a capture server that
answers `data: [DONE]`.

So this probe **is** the model. The fabricated `[model_providers.probe]` points at a
loopback server that answers the first request of every turn with a canned
`response.output_item.done` carrying an `exec_command` function call whose `cmd` writes a
file **outside the pane's cwd**, and answers every later request with a plain assistant
message so the turn ends. Codex executes it for real. Nothing is ever sent anywhere, and
no model is ever consulted.

Two consequences make this a better instrument than a transcript scrape:

  * **The witness is the filesystem, not prose.** Each arm reports two independent facts:
    whether the hook process ran at all (it appends its raw stdin to a witness file), and
    whether the out-of-worktree file exists afterwards. "Installed but silent" and
    "fired and denied" cannot be confused.
  * **The tool call is identical in every arm**, so the only variable is how the hook was
    delivered and whether it was trusted.

The cloned catalog entry matters and is not decoration. Against this build's *default*
model the shell tool is a custom JavaScript `exec`; against a clone of the operator's own
first catalog entry (`shell_type = "unified_exec"`, `apply_patch_tool_type = "freeform"`)
the tools are `exec_command` and `apply_patch` — which is the world #31 measured and the
world `HarnessSpec::guardrail.write_tools` is written against. `probe.py` clones the entry
for an unrelated reason; this one clones it so the arms are about the right tools.

## What the arms establish

**The headline, and it is the branch issue #45 hoped against: a `-c`-delivered hook does
not fire untrusted either.** Hook trust in this build is a property of *the hook*, not of
*how the hook arrived*. `arm 3` and `arm 4` differ only in delivery channel and both are
skipped silently while the write lands.

**The hint that suggested otherwise is explained, and it was the wrong injection.** cmux's
PATH shim does ship its hooks via `-c` and they do run — because the same shim also injects
`--dangerously-bypass-hook-trust`. `examples/codex-spike/hook_probe.py` reads the shim's
own arg stream (`cmux hooks codex inject-args`) and prints it, so the attribution is
visible rather than argued.

**A second finding, which falsifies half of C46: `-c` does not clobber a `config.toml`
hook.** `arm 5` puts our hook in `config.toml` and a decoy in `-c hooks.PreToolUse=[…]`,
and ours still fires — the two sources **compose**. What actually replaces is one `-c` by a
*later* `-c` for the same key (`arm 6`), which is the behaviour #31 saw and generalised one
step too far. So the cmux shim never erased anything; the guardrail was skipped for the one
reason trust, and the installer's existing placement in `config.toml` needs no change.

Exit code is the number of failed arms, so CI can gate on it. Skips cleanly (exit 0, loud
message) when `codex` is not on PATH — C13's rule. The shim arms skip loudly on their own
when this is not running inside cmux, which is not the same event.

**This is the only file that *runs* `--dangerously-bypass-hook-trust`.** Elsewhere in the
repository it appears only as prose explaining why it was not reached for. It is the
probe's *positive control* — arms 1 and 2 exist so that a `False` in arms 3 and 4 cannot
quietly mean "the hook script was broken". C46 refuses the flag as a *shipped* mechanism;
using it to prove an instrument is the opposite of relying on it.

Set `CODEX_BIN` to drive a specific binary. Per C47, the arms that matter run against the
absolute vendor path and the shim separately, and both readings are reported.
"""

import http.server
import json
import os
import shutil
import subprocess
import sys
import threading
import uuid

BUILD = "codex-cli 0.153.4"

# Short, under /tmp: codex opens `app-server-control.sock` inside CODEX_HOME and AF_UNIX
# paths are capped near 104 bytes. `trust_probe.py` records the same constraint.
ROOT = f"/tmp/codex-hook-probe-{os.getpid()}"
HOME = ROOT + "/home"
WT = ROOT + "/wt"
OUTSIDE = ROOT + "/outside/loot.txt"
WITNESS = ROOT + "/witness.jsonl"

# Assigned by the OS in `serve`, not chosen here. A fixed port is what makes two
# concurrent runs of this probe — or of the vendor tier that drives it as a model —
# collide, with the second one's server dying on `EADDRINUSE` and its arm reporting
# that the model was never reached (#43).
PORT = 0
VENDOR = os.environ.get("CODEX_BIN", "/opt/homebrew/bin/codex")

FAILURES = []


def check(arm, ok, detail=""):
    print(f"  {'PASS' if ok else 'FAIL'}  {arm}{'  — ' + detail if detail else ''}")
    if not ok:
        FAILURES.append(arm)


# --- the model: a provider that answers with one real tool call ---------------


def sse(*events):
    return b"".join(f"event: {n}\ndata: {json.dumps(o)}\n\n".encode() for n, o in events)


class Handler(http.server.BaseHTTPRequestHandler):
    """First request of a turn: call `exec_command`. Every later one: end the turn."""

    n = 0

    def log_message(self, *a):
        pass

    def do_POST(self):
        self.rfile.read(int(self.headers.get("Content-Length", "0")))
        Handler.n += 1
        rid = "resp_" + uuid.uuid4().hex[:12]
        if Handler.n == 1:
            item = {
                "type": "function_call",
                "id": "fc_1",
                "call_id": "call_1",
                "name": "exec_command",
                "arguments": json.dumps({"cmd": f"echo pwned > {OUTSIDE}", "workdir": WT}),
            }
        else:
            item = {
                "type": "message",
                "id": "msg_1",
                "role": "assistant",
                "status": "completed",
                "content": [{"type": "output_text", "text": "end"}],
            }
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        self.wfile.write(
            sse(
                ("response.created", {"type": "response.created", "response": {
                    "id": rid, "object": "response", "status": "in_progress", "output": []}}),
                ("response.output_item.done", {"type": "response.output_item.done",
                                               "output_index": 0, "item": item}),
                ("response.completed", {"type": "response.completed", "response": {
                    "id": rid, "object": "response", "status": "completed", "output": [item],
                    "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}}}),
            )
        )


def serve():
    """Start the canned model on a port the OS picks, and return it.

    Binding here rather than inside the thread is what lets a caller write the
    port into a config it composes on the next line.
    """
    global PORT
    server = http.server.HTTPServer(("127.0.0.1", 0), Handler)
    PORT = server.server_address[1]
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return PORT


# --- scratch installation -----------------------------------------------------


def build_scratch():
    """A CODEX_HOME at the canned provider, a worktree, and a target outside it.

    `sandbox_mode = "danger-full-access"` on purpose: C7's seatbelt would refuse the write
    on its own, and then a *skipped* hook would look exactly like a firing one. The
    sandbox is deliberately out of the way so this probe measures the guardrail alone.
    """
    shutil.rmtree(ROOT, ignore_errors=True)
    for d in (HOME, WT, os.path.dirname(OUTSIDE)):
        os.makedirs(d)

    catalog = json.loads(
        subprocess.run([VENDOR, "debug", "models"], capture_output=True, text=True).stdout
    )
    entry = dict(catalog["models"][0])
    entry.update(slug="probe-model", display_name="Probe")
    json.dump({"models": [entry]}, open(HOME + "/models.json", "w"))

    hook = ROOT + "/witness_hook.py"
    open(hook, "w").write(
        "#!/usr/bin/env python3\n"
        "import sys, json\n"
        "raw = sys.stdin.read()\n"
        f"open({WITNESS!r}, 'a').write(raw.strip() + '\\n')\n"
        "d = json.loads(raw) if raw.strip() else {}\n"
        "print(json.dumps({'hookSpecificOutput': {\n"
        "    'hookEventName': d.get('hook_event_name', 'PreToolUse'),\n"
        "    'permissionDecision': 'deny',\n"
        "    'permissionDecisionReason': 'FLEETOR-PROBE-DENY'}}))\n"
    )
    os.chmod(hook, 0o755)
    return f"/usr/bin/env python3 {hook}"


def base_config():
    """The seeded `config.toml`, composed after `serve` has a port to name."""
    return (
        'model = "probe-model"\n'
        'model_provider = "probe"\n'
        "features.hooks = true\n"
        f'model_catalog_json = "{HOME}/models.json"\n'
        'sandbox_mode = "danger-full-access"\n'
        'approval_policy = "never"\n'
        "\n[model_providers.probe]\n"
        'name = "probe"\n'
        f'base_url = "http://127.0.0.1:{PORT}"\n'
        'wire_api = "responses"\n'
        'experimental_bearer_token = "sk-probe"\n'
    )


def entry_toml(command):
    """One `hooks.<event>` array in codex's shape, as `install_guardrail` writes it."""
    return '[{hooks=[{type="command",command="%s",timeout=30}]}]' % command


def config_block(command):
    return '\n[[hooks.PreToolUse]]\nhooks=[{type="command",command="%s",timeout=30}]\n' % command


# --- the measurement ----------------------------------------------------------


def turn(binary, config_extra="", args=()):
    """One turn. Returns (the hook ran, the out-of-worktree file exists, calls)."""
    Handler.n = 0
    for p in (WITNESS, OUTSIDE):
        if os.path.exists(p):
            os.unlink(p)
    open(HOME + "/config.toml", "w").write(base_config() + config_extra)
    env = {**os.environ, "CODEX_HOME": HOME}
    try:
        subprocess.run(
            [binary, "exec", "--skip-git-repo-check", *args, "go"],
            input="", text=True, cwd=WT, env=env, capture_output=True, timeout=120,
        )
    except subprocess.TimeoutExpired:
        pass
    lines = open(WITNESS).read().strip().splitlines() if os.path.exists(WITNESS) else []
    return bool(lines), os.path.exists(OUTSIDE), len(lines)


def arm(name, binary, expect_fired, expect_wrote, config_extra="", args=()):
    fired, wrote, calls = turn(binary, config_extra, args)
    check(name, fired == expect_fired and wrote == expect_wrote,
          f"hook_fired={fired} (calls={calls}) wrote_outside={wrote}")
    return fired, wrote


BYPASS = "--dangerously-bypass-hook-trust"  # probe-only positive control; never shipped


def main():
    if not os.path.exists(VENDOR):
        print(f"SKIP — no codex at {VENDOR}. Set CODEX_BIN to point at one.")
        return 0
    running = subprocess.run([VENDOR, "--version"], capture_output=True, text=True).stdout.strip()
    if running != BUILD:
        print(f"NOTE — recorded against {BUILD}, running {running}. Findings may have drifted.")

    serve()
    ours = build_scratch()
    decoy = "/usr/bin/true"

    print("\n-- positive controls: the instrument fires and a deny really stops the tool --")
    arm("control-bypass-config-toml", VENDOR, True, False,
        config_extra=config_block(ours), args=(BYPASS,))
    arm("control-bypass-c-flag", VENDOR, True, False,
        args=(BYPASS, "-c", "hooks.PreToolUse=" + entry_toml(ours)))

    print("\n-- the question #45 was gated on, against the vendor binary (C47) --")
    arm("config-toml-untrusted-is-skipped", VENDOR, False, True,
        config_extra=config_block(ours))
    arm("c-flag-untrusted-is-skipped-too", VENDOR, False, True,
        args=("-c", "hooks.PreToolUse=" + entry_toml(ours)))

    print("\n-- what `-c` actually replaces (C46's second cause, falsified) --")
    arm("c-flag-does-not-clobber-config-toml", VENDOR, True, False,
        config_extra=config_block(ours),
        args=(BYPASS, "-c", "hooks.PreToolUse=" + entry_toml(decoy)))
    arm("a-later-c-flag-does-replace-an-earlier-one", VENDOR, False, True,
        args=(BYPASS, "-c", "hooks.PreToolUse=" + entry_toml(ours),
              "-c", "hooks.PreToolUse=" + entry_toml(decoy)))

    print("\n-- the same two readings through the PATH `codex` (C47) --")
    shim = next((p for p in subprocess.run(
        ["bash", "-c", "type -a -p codex"], capture_output=True, text=True,
        env=os.environ).stdout.split() if "cmux-cli-shims" in p), None)
    if not shim:
        print("  SKIP — no cmux shim on PATH; the shim arms need cmux to be running.")
    else:
        print(f"  shim: {shim}")
        cmux = os.environ.get("CMUX_CLAUDE_HOOK_CMUX_BIN", "")
        injected = []
        if cmux and os.path.exists(cmux):
            sock = os.environ.get("CMUX_SOCKET_PATH", "")
            cmd = [cmux] + (["--socket", sock] if sock else []) + ["hooks", "codex", "inject-args"]
            injected = subprocess.run(cmd, capture_output=True, text=True).stdout.split("\0")
        check("the-shim-injects-the-bypass-flag", BYPASS in injected,
              "this, not the -c channel, is why cmux's own hooks run")
        arm("config-toml-fires-under-the-shim", shim, True, False,
            config_extra=config_block(ours))
        arm("c-flag-fires-under-the-shim", shim, True, False,
            args=("-c", "hooks.PreToolUse=" + entry_toml(ours)))

    print("\n-- the real guardrail, with trust held out of the way --")
    real_arm()

    print(f"\n{len(FAILURES)} failed" + (f": {', '.join(FAILURES)}" if FAILURES else ""))
    return len(FAILURES)


# --- the real script, not the witness -----------------------------------------

#: `guardrail::hook_command`'s argument order, reproduced. If this drifts, the arm
#: below stops proving anything about the shipped guardrail — which is why it
#: asserts on the *journal line*, a thing only the real script writes.
def real_guardrail_command(pane, roots, policy, journal):
    script = ROOT + "/write-guardrail.py"
    shutil.copyfile(
        os.path.join(os.path.dirname(os.path.abspath(__file__)),
                     "..", "..", "src-tauri", "src", "write_guardrail.py"),
        script,
    )
    parts = ["/usr/bin/python3", script, "--pane", pane]
    for tool in ("Bash", "Write", "Edit", "MultiEdit", "NotebookEdit", "apply_patch"):
        parts += ["--tool", tool]
    parts += ["--event", "PreToolUse"]
    for r in roots:
        parts += ["--root", r]
    parts += ["--deny", policy, "--journal", journal]
    return " ".join("'%s'" % p.replace("'", r"'\''") for p in parts)


def real_arm():
    """Everything except trust: the shipped script, a real codex `exec_command`.

    This is the arm that says what is *not* broken. `write_guardrail.py` is covered by
    `src-tauri/tests/write_guardrail.rs` against a synthesised payload; nothing anywhere
    had driven it from a real codex tool call, through a real hook invocation, with the
    exact command line `install_guardrail` composes. The bypass flag stands in for the
    trust record so the arm measures the script rather than the gate.
    """
    journal = ROOT + "/guardrail.jsonl"
    policy = HOME + "/pane-config"
    for p in (journal, OUTSIDE):
        if os.path.exists(p):
            os.unlink(p)
    command = real_guardrail_command("worker-1", [WT], policy, journal)

    Handler.n = 0
    open(HOME + "/config.toml", "w").write(base_config() + config_block(command))
    env = {**os.environ, "CODEX_HOME": HOME}
    try:
        subprocess.run([VENDOR, "exec", "--skip-git-repo-check", BYPASS, "go"],
                       input="", text=True, cwd=WT, env=env, capture_output=True, timeout=120)
    except subprocess.TimeoutExpired:
        pass

    wrote = os.path.exists(OUTSIDE)
    lines = open(journal).read().strip().splitlines() if os.path.exists(journal) else []
    check("the-shipped-script-refuses-a-real-codex-tool-call", not wrote and bool(lines),
          f"wrote_outside={wrote} journal_lines={len(lines)}")
    if lines:
        rec = json.loads(lines[0])
        check("the-refusal-reaches-the-activity-feed-journal",
              rec.get("pane") == "worker-1" and rec.get("tool") == "Bash",
              f"pane={rec.get('pane')!r} tool={rec.get('tool')!r} paths={rec.get('paths')!r}")


# --- the same instrument, pointed at a CODEX_HOME FLEETOR seeded (#46, C49) ------


def fleet_seeded(argv):
    """Drive a **fleet-seeded** pane: the model is still this file, the witness is still
    the filesystem, and everything else is production's.

        hook_probe.py --fleet-seeded --home H --cwd W --outside O --binary B [--arg A]...

    `src-tauri/tests/vendor_binary_tier.rs` seeds `H` through `Harness::seed_config_dir`
    and `Harness::install_guardrail`, and passes the argv `Harness::command_args` composed
    — so the hook table, the guardrail's command line and the hook-trust bypass all come
    from production code rather than from anything typed here. This function supplies the
    two things a Rust test cannot: a model that emits a real `exec_command`, and a target
    outside the worktree to aim it at.

    **The seeded `config.toml` is never edited.** The provider redirect and the sandbox
    relaxation are `-c` session flags, which are a *higher* layer, so the document under
    test is used byte for byte as FLEETOR wrote it. `sandbox_mode` is relaxed for
    `build_scratch`'s reason — C7's seatbelt would refuse the write on its own, and a
    skipped hook would then look exactly like a firing one. The fence itself is measured
    by its own arm; this one measures the guardrail alone.

    Prints one JSON object and exits 0. The verdict is the Rust test's to reach.
    """
    global OUTSIDE, WT
    opts = {}
    args = []
    i = 0
    while i < len(argv):
        if argv[i] == "--arg":
            args.append(argv[i + 1])
        else:
            opts[argv[i].lstrip("-")] = argv[i + 1]
        i += 2

    home, WT, OUTSIDE, binary = opts["home"], opts["cwd"], opts["outside"], opts["binary"]
    if os.path.exists(OUTSIDE):
        os.unlink(OUTSIDE)

    catalog = json.loads(
        subprocess.run([binary, "debug", "models"], capture_output=True, text=True).stdout
    )
    entry = dict(catalog["models"][0])
    entry.update(slug="probe-model", display_name="Probe")
    json.dump({"models": [entry]}, open(home + "/models.json", "w"))

    Handler.n = 0
    serve()
    overrides = [
        'model="probe-model"',
        'model_provider="probe"',
        'model_catalog_json="%s/models.json"' % home,
        'sandbox_mode="danger-full-access"',
        'model_providers.probe.name="probe"',
        'model_providers.probe.base_url="http://127.0.0.1:%d"' % PORT,
        'model_providers.probe.wire_api="responses"',
        'model_providers.probe.experimental_bearer_token="sk-probe"',
    ]
    cmd = [binary, "exec", "--skip-git-repo-check", *args]
    for over in overrides:
        cmd += ["-c", over]
    cmd.append("go")
    timed_out = False
    try:
        subprocess.run(cmd, input="", text=True, cwd=WT,
                       env={**os.environ, "CODEX_HOME": home},
                       capture_output=True, timeout=180)
    except subprocess.TimeoutExpired:
        timed_out = True
    print(json.dumps({
        "wrote_outside": os.path.exists(OUTSIDE),
        "model_was_asked": Handler.n > 0,
        "timed_out": timed_out,
        "binary": binary,
    }))
    return 0


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--fleet-seeded":
        sys.exit(fleet_seeded(sys.argv[2:]))
    sys.exit(main())
