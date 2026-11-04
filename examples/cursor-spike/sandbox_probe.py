# SPENDS ~2 turns. Does cursorsandbox wrap the agent's shell commands? Polls the pane's process tree during a command.
# usage: [ALLOW='["Shell(*)"]'] python3 -I examples/cursor-spike/sandbox_probe.py <empty dir>
import json, os, pty, select, subprocess, sys, time, uuid
root = os.path.realpath(sys.argv[1]); repo, cfg, data = (os.path.join(root, d) for d in ("repo", "cfg", "data"))
for d in (repo, cfg, data): os.makedirs(d, exist_ok=True)
subprocess.run(["git", "init", "-q", repo])
conf = json.load(open(os.path.expanduser("~/.cursor/cli-config.json")))
conf["sandbox"] = {**conf.get("sandbox", {}), "mode": "enabled", "networkAccess": "allow_all"}
conf["approvalMode"] = "allowlist"
if "ALLOW" in os.environ: conf["permissions"] = {"allow": json.loads(os.environ["ALLOW"]), "deny": []}
json.dump(conf, open(os.path.join(cfg, "cli-config.json"), "w"), indent=2)
outside = os.path.expanduser(f"~/.fleetor-cursor-probe-{uuid.uuid4().hex[:8]}")
env = {k: v for k, v in os.environ.items() if k not in ("CURSOR_API_KEY", "CURSOR_AUTH_TOKEN")}
env.update(TERM="xterm-256color", CURSOR_CONFIG_DIR=cfg, CURSOR_DATA_DIR=data)
pid, fd = pty.fork()
if pid == 0:
    os.chdir(repo); os.execvpe("cursor-agent", ["cursor-agent", "--trust", "--sandbox", "enabled"], env)
def pump(sec):
    end = time.time() + sec; buf = b""
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], 0.2)
        if r:
            try: buf += os.read(fd, 65536)
            except OSError: break
    return buf
pump(6)
msg = f"Run exactly this one shell command, then report its full output: sleep 20; touch {outside}; echo exit=$?"
os.write(fd, b"\x1b[200~" + msg.encode() + b"\x1b[201~"); time.sleep(0.03); os.write(fd, b"\r")
seen, end, screen = set(), time.time() + 60, b""
while time.time() < end:
    screen += pump(1)
    ps = subprocess.run(["ps", "-A", "-o", "pid=,ppid=,comm="], capture_output=True, text=True).stdout
    kids, tree = {}, {pid}
    for line in ps.splitlines():
        p, pp, comm = line.split(None, 2); kids.setdefault(int(pp), []).append((int(p), comm))
    stack = [pid]
    while stack:
        for c, comm in kids.get(stack.pop(), []):
            tree.add(c); stack.append(c); seen.add(os.path.basename(comm))
os.kill(pid, 9)
print("prompted:", b"Run this command?" in screen)
print("descendant process names:", sorted(seen))
print("outside write happened:", os.path.exists(outside))
if os.path.exists(outside): os.remove(outside)
