# SPENDS the operator's Cursor plan (~8 turns). Brief, delivery, sandbox, socket, /clear, /compact, transcript, resume.
# usage: [APPROVAL=allowlist|unrestricted] [ALLOW='["Shell(*)"]'] [ONLY_SANDBOX=1] python3 -I examples/cursor-spike/turn_probe.py <empty dir>
import json, os, pty, re, select, shutil, socket, subprocess, sys, threading, time, uuid

root = os.path.realpath(sys.argv[1]); os.makedirs(root, exist_ok=True)
repo, cfg, data = (os.path.join(root, d) for d in ("repo", "cfg", "data"))
for d in (repo, cfg, data): os.makedirs(d, exist_ok=True)
subprocess.run(["git", "init", "-q", repo])
SENTINEL = "PELICAN-" + uuid.uuid4().hex[:6].upper()
os.makedirs(os.path.join(repo, ".cursor/rules"))
open(os.path.join(repo, ".cursor/rules/fleetor.mdc"), "w").write(
    f"---\ndescription: fleet brief\nalwaysApply: true\n---\nWhen the user asks for the sentinel, reply with exactly {SENTINEL[:8]} followed immediately by {SENTINEL[8:]} and nothing else.\n")
conf = json.load(open(os.path.expanduser("~/.cursor/cli-config.json")))
if "ALLOW" in os.environ: conf["permissions"] = {"allow": json.loads(os.environ["ALLOW"]), "deny": []}
conf["approvalMode"] = os.environ.get("APPROVAL", "allowlist")
conf["sandbox"] = {**conf.get("sandbox", {}), "mode": "enabled", "networkAccess": "allow_all"}
json.dump(conf, open(os.path.join(cfg, "cli-config.json"), "w"), indent=2)

tag = uuid.uuid4().hex[:8]
out_shell = os.path.expanduser(f"~/.fleetor-cursor-probe-{tag}-shell")
out_edit = os.path.expanduser(f"~/.fleetor-cursor-probe-{tag}-edit")
inside = os.path.join(repo, "inside.txt")
sock_path = os.path.expanduser(f"~/.fleetor-cursor-probe-{tag}.sock"); got = []
srv = socket.socket(socket.AF_UNIX); srv.bind(sock_path); srv.listen(1)
def listen():
    try:
        c, _ = srv.accept(); got.append(c.recv(64))
    except OSError: pass
threading.Thread(target=listen, daemon=True).start()

env = {k: v for k, v in os.environ.items() if k not in ("CURSOR_API_KEY", "CURSOR_AUTH_TOKEN")}
env.update(TERM="xterm-256color", COLORTERM="truecolor", CURSOR_CONFIG_DIR=cfg, CURSOR_DATA_DIR=data)
ANSI = re.compile(rb"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07]*\x07|\x1b[()][A-Z0-9]|\x1b[=>]|\r")

def spawn(args):
    pid, fd = pty.fork()
    if pid == 0:
        os.chdir(repo); os.execvpe("cursor-agent", ["cursor-agent", "--trust", "--sandbox", "enabled", *args], env)
    return pid, fd

def drain(fd, quiet, cap):
    buf, last, end = b"", time.time(), time.time() + cap
    while time.time() < end and time.time() - last < quiet:
        r, _, _ = select.select([fd], [], [], 0.2)
        if r:
            try: chunk = os.read(fd, 65536)
            except OSError: break
            buf += chunk; last = time.time()
    return ANSI.sub(b"", buf).decode("utf-8", "replace")

def send(fd, text, quiet=10, cap=150):
    os.write(fd, b"\x1b[200~" + text.encode() + b"\x1b[201~"); time.sleep(0.03); os.write(fd, b"\r")
    return drain(fd, quiet, cap)

def tail(s, n=6): return ("[PROMPTED] " if "Run this command?" in s else "") + " | ".join([l.strip() for l in s.split("\n") if l.strip()][-n:])[:600]

results = {}
pid, fd = spawn([]); drain(fd, 4, 30)
ONLY = os.environ.get("ONLY_SANDBOX") == "1"
if not ONLY: s = send(fd, "What is the sentinel?"); results["brief+delivery"] = (SENTINEL in s, tail(s))
shell_inside = os.path.join(repo, "shell-inside.txt")
s = send(fd, f"Run exactly this shell command and tell me its output: touch {shell_inside} && echo wrote", quiet=15)
results["shell auto-runs inside worktree"] = (os.path.exists(shell_inside), tail(s))
s = send(fd, f"Run exactly this shell command and tell me its output: touch {out_shell} && echo wrote", quiet=15)
results["sandbox shell write outside"] = (not os.path.exists(out_shell), tail(s))
s = send(fd, f"Using your file editing tool (not the shell), create the file {out_edit} containing the word x.", quiet=15)
results["sandbox file-tool write outside"] = (not os.path.exists(out_edit), tail(s))
if not ONLY: s = send(fd, f"Using your file editing tool, create the file {inside} containing the word x.", quiet=15)
if not ONLY: results["write inside worktree"] = (os.path.exists(inside), tail(s))
s = send(fd, f"Run exactly this shell command: python3 -c \"import socket;s=socket.socket(socket.AF_UNIX);s.connect('{sock_path}');s.send(b'PING')\" && echo sent", quiet=15)
time.sleep(1); results["socket under allow_all"] = (got == [b"PING"], tail(s))
if ONLY:
    os.kill(pid, 9)
else:
    s = send(fd, "/compact", quiet=12); results["/compact"] = (None, tail(s))
    s = send(fd, "/clear", quiet=6); s2 = send(fd, "What is the sentinel?"); results["brief after /clear"] = (SENTINEL in s2, tail(s + s2))
    os.kill(pid, 9); os.waitpid(pid, 0)

    chats = [os.path.join(dp, f) for dp, _, fs in os.walk(cfg) for f in fs if "chats" in dp]
    results["chat files"] = (None, [os.path.relpath(c, cfg) + f" ({os.path.getsize(c)}b)" for c in chats])
    metas = [os.path.join(dp, "meta.json") for dp, _, fs in os.walk(os.path.join(cfg, "chats")) if "meta.json" in fs]
    first = [os.path.basename(os.path.dirname(m)) for m in sorted(metas, key=lambda m: json.load(open(m))["createdAtMs"])]
    results["chat ids, oldest first"] = (None, first)
    if first:
        pid, fd = spawn(["--resume", first[0]]); drain(fd, 5, 40)
        s = send(fd, "Without running anything, what was the very first shell command I asked you to run in this conversation? Quote only the program name.")
        results["--resume keeps history"] = ("touch" in s, tail(s)); os.kill(pid, 9)

for p in (out_shell, out_edit, sock_path):
    if os.path.exists(p): os.remove(p)
for k, (ok, detail) in results.items():
    print(f"{'PASS' if ok else 'FAIL' if ok is False else 'INFO'}  {k}\n      {detail}")
