# Zero tokens: a local stub provider makes the model call write/shell/edit, and the real PLUGIN template
# from placement/opencode.rs (with the real write_guardrail.py) is asked to refuse the ones outside the root.
# usage: python3 -I examples/opencode-spike/guardrail_probe.py <empty dir>
import os, re, sys, json, shlex, subprocess, threading
from http.server import BaseHTTPRequestHandler, HTTPServer

here = os.path.dirname(os.path.abspath(__file__)); repo_root = os.path.dirname(os.path.dirname(here))
root = os.path.abspath(sys.argv[1]); work = os.path.join(root, "repo"); cfg = os.path.join(root, "cfg"); outside = os.path.join(root, "outside")
for d in (work, outside, os.path.join(cfg, "plugins")): os.makedirs(d, exist_ok=True)
subprocess.run(["git", "init", "-q", work])
subprocess.run(["git", "-C", work, "-c", "user.email=p@p", "-c", "user.name=p", "commit", "-q", "--allow-empty", "-m", "first"])
work = os.path.realpath(work); outside = os.path.realpath(outside); journal = os.path.join(root, "journal.jsonl")

src = open(os.path.join(repo_root, "src-tauri/src/placement/opencode.rs")).read()
plugin = re.search(r'const PLUGIN: &str = r#"(.*?)"#;', src, re.S).group(1)
script = os.path.join(repo_root, "src-tauri/src/write_guardrail.py")
tools = ["shell", "write", "edit", "patch", "worktree_remove"]
command = " ".join([shlex.quote("/usr/bin/python3"), shlex.quote(script), "--pane", "'worker-1'"]
                   + [f"--tool {shlex.quote(t)}" for t in tools]
                   + ["--event 'execute.before'", f"--root {shlex.quote(work)}", f"--deny {shlex.quote(os.path.join(root, 'policy'))}",
                      f"--journal {shlex.quote(journal)}"])
open(os.path.join(cfg, "plugins", "fleetor-guardrail.js"), "w").write(
    plugin.replace("__COMMAND__", json.dumps(command)).replace("__EVENT__", json.dumps("execute.before")))

call = {}
class Stub(BaseHTTPRequestHandler):
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        replied = any(m.get("role") == "tool" for m in body.get("messages", []))
        if body.get("tools") and not replied:
            delta = {"role": "assistant", "tool_calls": [{"index": 0, "id": "call_1", "type": "function",
                     "function": {"name": call["name"], "arguments": json.dumps(call["args"])}}]}; finish = "tool_calls"
        else:
            delta = {"role": "assistant", "content": "done"}; finish = "stop"
        chunk = lambda d, f: {"id": "x", "object": "chat.completion.chunk", "created": 0, "model": "m", "choices": [{"index": 0, "delta": d, "finish_reason": f}]}
        self.send_response(200); self.send_header("Content-Type", "text/event-stream"); self.end_headers()
        for c in (chunk(delta, None), chunk({}, finish)): self.wfile.write(f"data: {json.dumps(c)}\n\n".encode())
        self.wfile.write(b"data: [DONE]\n\n")
    def log_message(self, *a): pass
srv = HTTPServer(("127.0.0.1", 0), Stub); threading.Thread(target=srv.serve_forever, daemon=True).start()

conf = {"provider": {"stub": {"npm": "@ai-sdk/openai-compatible", "name": "stub",
                              "options": {"baseURL": f"http://127.0.0.1:{srv.server_port}/v1", "apiKey": "x"},
                              "models": {"m": {"name": "m", "tool_call": True}}}},
        "model": "stub/m", "small_model": "stub/m", "default_agent": "fleetor",
        "agent": {"fleetor": {"description": "probe", "mode": "primary", "prompt": "probe"}},
        "permission": {"*": "allow", "external_directory": "deny"}}
env = dict(os.environ, PWD=work, OPENCODE_CONFIG_DIR=cfg, OPENCODE_CONFIG_CONTENT=json.dumps(conf))

def arm(name, args, path, want_written):
    call.update(name=name, args=args)
    try:
        out = subprocess.run(["opencode", "run", "--standalone", *(["--print-logs"] if os.environ.get("PROBE_VERBOSE") else []), "go"],
                             cwd=work, env=env, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=60)
    except subprocess.TimeoutExpired as e:
        out = subprocess.CompletedProcess(e.cmd, -1, (e.stdout or b"").decode(errors="replace") + " [TIMEOUT]", (e.stderr or b"").decode(errors="replace"))
    open(os.path.join(root, f"{name}.log"), "a").write(out.stderr)
    written = os.path.exists(path)
    print(f"== {name} -> {path}: {'written' if written else 'absent'} ({'pass' if written == want_written else 'FAIL'})")
    if written != want_written or os.environ.get("PROBE_VERBOSE"):
        print("   stdout:", out.stdout[-1500:].replace("\n", " | ")); print("   stderr:", out.stderr[-800:].replace("\n", " | "))

arm("write", {"path": f"{outside}/w.txt", "content": "hi"}, f"{outside}/w.txt", False)
arm("shell", {"command": f"touch {outside}/s.txt"}, f"{outside}/s.txt", False)
arm("shell", {"command": "touch s.txt", "workdir": outside}, f"{outside}/s.txt", False)
open(f"{outside}/e.txt", "w").write("old")
arm("edit", {"path": f"{outside}/e.txt", "oldString": "old", "newString": "new"}, f"{outside}/e.txt.never", False)
print("   edit outside left the file:", open(f"{outside}/e.txt").read())
arm("patch", {"patchText": f"*** Begin Patch\n*** Add File: {outside}/p.txt\n+hi\n*** End Patch"}, f"{outside}/p.txt", False)
arm("write", {"path": f"{work}/inside.txt", "content": "hi"}, f"{work}/inside.txt", True)
arm("shell", {"command": "touch inside-shell.txt"}, f"{work}/inside-shell.txt", True)
print("== journal refusals:", sum(1 for _ in open(journal)) if os.path.exists(journal) else 0)
