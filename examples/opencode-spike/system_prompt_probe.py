# Zero tokens: points a seat at a local stub provider that records the request body, so the system prompt
# each agent sends can be read directly. Answers whether agent.fleetor.prompt replaces or appends (D-145).
# usage: python3 -I examples/opencode-spike/system_prompt_probe.py <empty dir>
import os, sys, json, subprocess, threading
from http.server import BaseHTTPRequestHandler, HTTPServer

root = os.path.abspath(sys.argv[1]); work = os.path.join(root, "repo"); cfg = os.path.join(root, "cfg")
os.makedirs(work, exist_ok=True); os.makedirs(cfg, exist_ok=True); subprocess.run(["git", "init", "-q", work])
brief = os.path.join(cfg, "brief.md"); open(brief, "w").write("BRIEF-SENTINEL-5521: you are a probe.\n")
bodies = []

class Stub(BaseHTTPRequestHandler):
    def do_POST(self):
        bodies.append(json.loads(self.rfile.read(int(self.headers["Content-Length"]))))
        self.send_response(500); self.end_headers(); self.wfile.write(b'{"error":{"message":"stub"}}')
    def log_message(self, *a): pass
srv = HTTPServer(("127.0.0.1", 0), Stub); threading.Thread(target=srv.serve_forever, daemon=True).start()

conf = {"provider": {"stub": {"npm": "@ai-sdk/openai-compatible", "name": "stub",
                              "options": {"baseURL": f"http://127.0.0.1:{srv.server_port}/v1", "apiKey": "x"},
                              "models": {"m": {"name": "m"}}}},
        "model": "stub/m", "small_model": "stub/m",
        "agent": {"fleetor": {"description": "fleet seat", "mode": "primary", "prompt": "{file:" + brief + "}"}}}
env = dict(os.environ, OPENCODE_CONFIG_DIR=cfg, OPENCODE_CONFIG_CONTENT=json.dumps(conf))

def system_of(agent):
    bodies.clear()
    subprocess.run(["opencode", "run", "--standalone", "--agent", agent, "hi"], cwd=work, env=env, capture_output=True, timeout=120)
    chats = [b for b in bodies if any(m.get("role") == "user" and "hi" in json.dumps(m.get("content")) for m in b.get("messages", []))]
    sys_msgs = [m for b in chats[:1] for m in b["messages"] if m.get("role") == "system"]
    return "\n".join(m["content"] if isinstance(m["content"], str) else json.dumps(m["content"]) for m in sys_msgs)

for agent in ("fleetor", "build"):
    s = system_of(agent)
    print(f"== {agent}: {len(s)} chars, brief sentinel present: {'BRIEF-SENTINEL-5521' in s}, "
          f"mentions opencode: {'opencode' in s.lower()}")
    print("   first 300:", s[:300].replace("\n", " | "))
    print("   sections:", [l for l in s.split("\n") if l.startswith("# ")])
