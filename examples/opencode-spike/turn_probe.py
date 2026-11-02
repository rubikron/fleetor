# SPENDS the operator's plan: drives a seat-shaped opencode pane through real turns and reports what each arm did.
# usage: python3 -I examples/opencode-spike/turn_probe.py <empty dir> main|resume|baseline
#   main     — brief sentinel, hook refusal, external_directory, inside write, socket, /compact, /new
#   resume   — relaunch with --session <main's session id> and ask for the remembered word (run after main)
#   baseline — one "ok" turn on the fleetor agent and one on opencode's default agent, for input-token comparison
import os, pty, sys, time, select, subprocess, re, json, sqlite3, socket, threading

root = os.path.abspath(sys.argv[1]); mode = sys.argv[2]
work = os.path.join(root, "repo"); cfg = os.path.join(root, "cfg"); outside = os.path.join(root, "outside")
hooklog = os.path.join(root, "hook.log"); model = os.environ.get("PROBE_MODEL", "opencode-go/deepseek-v4.1-flash")
db = os.path.expanduser("~/.local/share/opencode/opencode.db")
for d in (work, outside, os.path.join(cfg, "plugins")): os.makedirs(d, exist_ok=True)
if not os.path.isdir(os.path.join(work, ".git")): subprocess.run(["git", "init", "-q", work])
brief = os.path.join(cfg, "brief.md")
open(brief, "w").write("You are a probe in a test harness. Your sentinel is TANGERINE-4417. Do exactly what each message asks, using the tool it names.\n")
open(os.path.join(cfg, "plugins", "fleetor-probe.js"), "w").write(f"""import {{ appendFileSync }} from 'node:fs'
const show = (v) => {{ try {{ return JSON.stringify(v) }} catch {{ return JSON.stringify(Object.keys(v ?? {{}})) }} }}
export default {{ id: 'fleetor-probe', async setup(ctx) {{
  ctx.tool.hook('execute.before', async (ev) => {{
    const s = show(ev); appendFileSync({json.dumps(hooklog)}, s + '\\n')
    if (s.includes('BLOCKME')) throw new Error('fleetor guardrail: refused (probe)')
  }})
}} }}
""")

def seat(agent=True):
    c = {"model": model, "permission": {"external_directory": "deny"}}
    if agent:
        c["default_agent"] = "fleetor"
        c["agent"] = {"fleetor": {"description": "fleet seat", "mode": "primary", "prompt": "{file:" + brief + "}"}}
    return json.dumps(c)

def rows():
    c = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    try:
        return c.execute("select id, agent, model, tokens_input, tokens_output, tokens_cache_read, time_created from session_v2 "
                         "where directory = ? order by time_created", (work,)).fetchall() or \
               c.execute("select id, agent, model, tokens_input, tokens_output, tokens_cache_read, time_created from session "
                         "where directory = ? order by time_created", (work,)).fetchall()
    finally:
        c.close()

class Pane:
    def __init__(self, args, agent=True):
        env = dict(os.environ, TERM="xterm-256color", COLORTERM="truecolor", OPENCODE_CONFIG_DIR=cfg,
                   OPENCODE_CONFIG=os.path.expanduser("~/.config/opencode/opencode.json"), OPENCODE_CONFIG_CONTENT=seat(agent))
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.chdir(work); os.execvpe("opencode", ["opencode", "--standalone", "--auto", *args], env)
        self.buf = b""; self.pump(12)
    def pump(self, secs, quiet=None):
        end = time.time() + secs; last = time.time()
        while time.time() < end:
            r, _, _ = select.select([self.fd], [], [], 0.2)
            if r:
                try: chunk = os.read(self.fd, 65536)
                except OSError: return
                self.buf += chunk; last = time.time()
            elif quiet and time.time() - last > quiet: return
    def mark(self): return len(self.buf)
    def send(self, text, wait=120):
        m = self.mark()
        os.write(self.fd, b"\x1b[200~" + text.encode() + b"\x1b[201~"); time.sleep(0.4); os.write(self.fd, b"\r")
        self.pump(6); self.pump(wait, quiet=6)
        return text_of(self.buf[m:])
    def close(self):
        try: os.killpg(os.getpgid(self.pid), 9)
        except ProcessLookupError: pass

def text_of(b):
    return re.sub(rb"\x1b\[[0-9;?<>=]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)|\x1b[()][A-Z0-9]|\x1b[=>]|\r", b"", b).decode("utf-8", "replace")
def has(t, needle): return needle in re.sub(r"\s+", "", t)
def report(name, ok, extra=""): print(f"== {name}: {ok}" + (f" — {extra}" if extra else ""))
def tail(t, n=6): return " | ".join([l.strip() for l in t.split("\n") if l.strip()][-n:])[-500:]

if mode == "main":
    sock = socket.socket(socket.AF_UNIX); here = os.getcwd(); os.chdir(outside); sock.bind("hub.sock"); os.chdir(here); sock.listen(1)
    accepted = []; threading.Thread(target=lambda: accepted.append(sock.accept()), daemon=True).start()
    p = Pane([])
    t = p.send("Reply with your sentinel, and remember the word PLUM-88 for later."); report("brief sentinel reaches the model", has(t, "TANGERINE-4417"), tail(t))
    t = p.send(f"Use the bash tool to run exactly: touch {outside}/BLOCKME-bash.txt"); report("hook throw refuses bash (file absent)", not os.path.exists(f"{outside}/BLOCKME-bash.txt"), tail(t))
    t = p.send(f"Use the write tool (not bash) to create {outside}/BLOCKME-write.txt containing hi"); report("hook throw refuses write tool (file absent)", not os.path.exists(f"{outside}/BLOCKME-write.txt"), tail(t))
    t = p.send(f"Use the write tool (not bash) to create {outside}/ext-write.txt containing hi"); report("external_directory deny refuses write tool (file absent)", not os.path.exists(f"{outside}/ext-write.txt"), tail(t))
    t = p.send(f"Use the bash tool to run exactly: touch {outside}/ext-bash.txt"); report("external_directory deny refuses bash (file absent)", not os.path.exists(f"{outside}/ext-bash.txt"), tail(t))
    t = p.send("Use the bash tool to run exactly: echo ok > inside.txt"); report("--auto runs an inside write unprompted (file present)", os.path.exists(f"{work}/inside.txt"), tail(t))
    t = p.send("Use the bash tool to run exactly: python3 -c \"import socket;s=socket.socket(socket.AF_UNIX);s.connect('../outside/hub.sock');print('connected')\"")
    time.sleep(1); report("unix socket outside the worktree reachable", bool(accepted), tail(t))
    print("== sessions after turns:", rows())
    t = p.send("/compact", wait=120); report("/compact", "see tail", tail(t, 8)); print("== sessions after /compact:", rows())
    t = p.send("/new", wait=20); report("/new", "see tail", tail(t, 8))
    t = p.send("Reply with your sentinel."); report("brief survives /new", has(t, "TANGERINE-4417"), tail(t))
    print("== sessions after /new:", rows())
    p.close()
    print("== hook calls logged:", sum(1 for _ in open(hooklog)) if os.path.exists(hooklog) else 0)
    if os.path.exists(hooklog): print("== first hook event:", open(hooklog).readline()[:800])
elif mode == "resume":
    first = rows()[0][0]
    p = Pane(["--session", first])
    t = p.send("What word did I ask you to remember? Reply with just the word."); report(f"--session {first} keeps the conversation", has(t, "PLUM-88"), tail(t))
    p.close(); print("== sessions:", rows())
elif mode == "baseline":
    for agent in (True, False):
        p = Pane([], agent=agent); t = p.send("Reply with just: ok"); p.close()
        report(f"agent={'fleetor' if agent else 'default'}", "done", tail(t, 3))
    print("== sessions (compare tokens_input of the last two):", rows())
