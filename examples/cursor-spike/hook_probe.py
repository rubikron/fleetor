# SPENDS ~2-4 turns. What does cursor's preToolUse hook receive, and does a hooks.json in CURSOR_CONFIG_DIR fire?
# usage: [GUARD=1 [CLAUDE_LOCAL=1]] python3 -I examples/cursor-spike/hook_probe.py <empty dir>
#   default: a logging hook allows everything; GUARD=1: the fleet's write_guardrail.py is the hook.
import json, os, pty, select, shutil, subprocess, sys, time, uuid
here = os.path.dirname(os.path.realpath(__file__))
root = os.path.realpath(sys.argv[1]); repo, cfg, data = (os.path.join(root, d) for d in ("repo", "cfg", "data"))
for d in (repo, cfg, data): os.makedirs(d, exist_ok=True)
subprocess.run(["git", "init", "-q", repo])
conf = json.load(open(os.path.expanduser("~/.cursor/cli-config.json")))
conf["approvalMode"] = "allowlist"; conf["permissions"] = {"allow": ["Shell(*)"], "deny": []}
json.dump(conf, open(os.path.join(cfg, "cli-config.json"), "w"), indent=2)
log = os.path.join(root, "hook-log.jsonl")
logger = os.path.join(root, "logger.py")
open(logger, "w").write(f"import sys\nraw=sys.stdin.read()\nopen({log!r},'a').write(raw.strip()+'\\n')\nprint('{{}}')\n")
if os.environ.get("GUARD") == "1":
    guard = os.path.join(cfg, "write-guardrail.py"); shutil.copy(os.path.join(here, "../../src-tauri/src/write_guardrail.py"), guard)
    tools = os.environ.get("TOOLS", "Shell Write Delete StrReplace Edit MultiEdit EditNotebook").split()
    cmd = f"tee -a {log} | /usr/bin/python3 {guard} --pane w2 " + " ".join(f"--tool {t}" for t in tools) + \
          f" --event {'PreToolUse' if os.environ.get('CLAUDE_LOCAL') == '1' else 'preToolUse'} --root {repo} --journal {root}/journal.jsonl"
else:
    cmd = f"/usr/bin/python3 {logger}"
if os.environ.get("CLAUDE_LOCAL") == "1":
    os.makedirs(os.path.join(repo, ".claude"), exist_ok=True)
    cmd = cmd.replace("tee -a", "(cat; echo) | tee -a")
    json.dump({"hooks": {"PreToolUse": [{"matcher": "|".join(tools), "hooks": [{"type": "command", "command": cmd}]}]}},
              open(os.path.join(repo, ".claude", "settings.local.json"), "w"))
else:
    os.makedirs(os.path.join(repo, ".cursor"), exist_ok=True)
    json.dump({"version": 1, "hooks": {"preToolUse": [{"command": cmd}]}}, open(os.path.join(repo, ".cursor", "hooks.json"), "w"))
outside = os.path.expanduser(f"~/.fleetor-cursor-probe-{uuid.uuid4().hex[:8]}")
env = {k: v for k, v in os.environ.items() if k not in ("CURSOR_API_KEY", "CURSOR_AUTH_TOKEN")}
env.update(TERM="xterm-256color", CURSOR_CONFIG_DIR=cfg, CURSOR_DATA_DIR=data)
pid, fd = pty.fork()
if pid == 0:
    os.chdir(repo); os.execvpe("cursor-agent", ["cursor-agent", "--trust"], env)
def pump(quiet, cap):
    buf, last, end = b"", time.time(), time.time() + cap
    while time.time() < end and time.time() - last < quiet:
        r, _, _ = select.select([fd], [], [], 0.2)
        if r:
            try: buf += os.read(fd, 65536); last = time.time()
            except OSError: break
    return buf
def send(t):
    os.write(fd, b"\x1b[200~" + t.encode() + b"\x1b[201~"); time.sleep(0.03); os.write(fd, b"\r"); return pump(15, 150)
pump(4, 30)
res = {}
if os.environ.get("GUARD") == "1":
    s = send(f"Run exactly this shell command: touch {outside}.shell && echo wrote")
    res["shell outside refused"] = (not os.path.exists(outside + ".shell"), b"Run this command?" in s)
    s = send(f"Using your file editing tool (not the shell), create the file {outside}.edit containing x.")
    res["file-tool outside refused"] = (not os.path.exists(outside + ".edit"), b"Write to this file?" in s)
    open(outside + ".existing", "w").write("alpha\n")
    s = send(f"Using your file editing tool (not the shell), change the word alpha to beta in {outside}.existing.")
    res["file-tool edit of an existing outside file refused"] = (open(outside + ".existing").read() == "alpha\n", b"Write to this file?" in s)
    s = send(f"Using your file deletion tool (not the shell), delete the file {outside}.existing.")
    res["file-tool delete outside refused"] = (os.path.exists(outside + ".existing"), b"Delete" in s and b"(y)" in s)
s = send(f"Run exactly this shell command: touch {repo}/a.txt && echo wrote"); res["shell inside works"] = (os.path.exists(f"{repo}/a.txt"), b"Run this command?" in s)
s = send(f"Using your file editing tool (not the shell), create the file {repo}/b.txt containing x."); res["file-tool inside works"] = (os.path.exists(f"{repo}/b.txt"), b"Write to this file?" in s)
os.kill(pid, 9)
for p in (outside + ".shell", outside + ".edit", outside + ".existing"):
    if os.path.exists(p): os.remove(p)
for k, (ok, prompted) in res.items(): print(f"{'PASS' if ok else 'FAIL'}  {k}{'  [PROMPTED]' if prompted else ''}")
print("--- hook payloads")
for line in open(log) if os.path.exists(log) else []:
    try: e = json.loads(line)
    except ValueError: print("unparsed:", line[:200]); continue
    print(json.dumps({k: (v if k != "tool_input" else {a: str(b)[:80] for a, b in v.items()}) for k, v in e.items() if k in ("hook_event_name", "tool_name", "tool_input", "cwd", "workspace_roots")})[:400])
pass  # print("payload keys:", sorted(json.loads(open(log).readline()).keys()) if os.path.exists(log) else "NO HOOK FIRED")
if os.path.exists(f"{root}/journal.jsonl"): print("--- journal"); print(open(f"{root}/journal.jsonl").read()[:600])
