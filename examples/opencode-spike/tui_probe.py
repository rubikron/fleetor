# Zero tokens: launches opencode in a pty in a fresh repo with a seat-shaped config, submits nothing,
# and reports the screen, process tree, whether the seat's plugin/agent/model loaded, and what it wrote.
# usage: python3 -I examples/opencode-spike/tui_probe.py <empty dir> [opencode args...]
import os, pty, sys, time, select, subprocess, re, json, sqlite3

root = os.path.abspath(sys.argv[1]); extra = sys.argv[2:]
home = os.path.expanduser("~")
work = os.path.join(root, "repo"); cfg = os.path.join(root, "cfg"); marker = os.path.join(root, "plugin-loaded")
os.makedirs(work, exist_ok=True); os.makedirs(os.path.join(cfg, "plugins"), exist_ok=True)
subprocess.run(["git", "init", "-q", work])
brief = os.path.join(cfg, "brief.md")
open(brief, "w").write("You are a probe. If asked for the sentinel, answer TANGERINE-4417.\n")
# v2 plugin shape (v2.2.5 refuses v1's named async export): a default export with an id and setup.
open(os.path.join(cfg, "plugins", "fleetor-probe.js"), "w").write(
    "import { writeFileSync } from 'node:fs'\n"
    "export default { id: 'fleetor-probe', async setup(ctx) {\n"
    f"  writeFileSync({json.dumps(marker)}, JSON.stringify({{ctx: Object.keys(ctx), tool: Object.keys(ctx.tool), permission: Object.keys(ctx.permission), hook: String(ctx.tool.hook).slice(0, 600)}}))\n"
    "} }\n")
seat = {
    "model": os.environ.get("PROBE_MODEL", "opencode-go/deepseek-v4.1-flash"),
    "default_agent": "fleetor",
    "agent": {"fleetor": {"description": "fleet seat", "mode": "primary", "prompt": "{file:" + brief + "}"}},
    "permission": {"external_directory": "deny"},
}
env = dict(os.environ, TERM="xterm-256color", COLORTERM="truecolor",
           OPENCODE_CONFIG_DIR=cfg, OPENCODE_CONFIG_CONTENT=json.dumps(seat))

operator_cfg = os.path.join(home, ".config/opencode/opencode.json")
db = os.path.join(home, ".local/share/opencode/opencode.db")
def sessions():
    c = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    try:
        return {t: c.execute(f"select id, agent, model from {t} where directory like ?", (root + "%",)).fetchall()
                for t in ("session", "session_v2")}
    finally:
        c.close()
mtime = os.path.getmtime(operator_cfg)

def debug(*args):
    r = subprocess.run(["opencode", "debug", *args, *[a for a in extra if a == "--standalone"]], cwd=work, env=env, capture_output=True, text=True, timeout=60)
    return (r.stdout + r.stderr).strip()
print("== debug agents (fleetor present?):", "fleetor" in debug("agents"))
cfg_out = debug("config")
print("== debug config sources:", re.findall(r'"path":\s*"([^"]+)"', cfg_out) or cfg_out[:400])

pid, fd = pty.fork()
if pid == 0:
    os.chdir(work); os.execvpe("opencode", ["opencode", *extra], env)
buf = b""; end = time.time() + 15
while time.time() < end:
    r, _, _ = select.select([fd], [], [], 0.2)
    if r:
        try: buf += os.read(fd, 65536)
        except OSError: break
try:
    pgid = os.getpgid(pid)
    tree = subprocess.run(["ps", "-o", "pid,ppid,comm,args", "-g", str(pgid)], capture_output=True, text=True).stdout
    os.killpg(pgid, 9)
except ProcessLookupError:
    tree = "exited before 15s, status %s" % (os.waitpid(pid, 0)[1],)
text = re.sub(rb"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07]*\x07|\x1b[()][A-Z0-9]|\x1b[=>]|\r", b"", buf).decode("utf-8", "replace")
lines = [l.strip() for l in text.split("\n") if l.strip()]
print("== screen (last 25 lines)"); print("\n".join(lines[-25:]))
print("== process tree"); print(tree)
print("== modes requested:", sorted(set(re.findall(rb"\x1b\[\?(\d+)[hl]", buf))))
print("== plugin marker:", open(marker).read() if os.path.exists(marker) else "absent")
print("== sessions for this dir:", sessions())
print("== operator opencode.json untouched:", os.path.getmtime(operator_cfg) == mtime)
print("== files written under cfg:", sorted(os.path.relpath(os.path.join(d, f), cfg) for d, _, fs in os.walk(cfg) for f in fs))
