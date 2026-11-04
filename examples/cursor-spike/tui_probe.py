# Zero tokens: launches cursor-agent in a pty in a fresh repo, submits nothing, prints the screen and process tree after 12s.
# usage: python3 -I examples/cursor-spike/tui_probe.py <empty dir> [cursor-agent args...]
import os, pty, sys, time, select, subprocess, re, json
root = sys.argv[1]; extra = sys.argv[2:]
work = os.path.join(root, "repo"); os.makedirs(work, exist_ok=True)
subprocess.run(["git", "init", "-q", work])
env = dict(os.environ, TERM="xterm-256color", COLORTERM="truecolor",
           CURSOR_CONFIG_DIR=os.path.join(root, "cfg"), CURSOR_DATA_DIR=os.path.join(root, "data"))
pid, fd = pty.fork()
if pid == 0:
    os.chdir(work); os.execvpe("cursor-agent", ["cursor-agent", *extra], env)
buf = b""; end = time.time() + 12
while time.time() < end:
    r, _, _ = select.select([fd], [], [], 0.2)
    if r:
        try: buf += os.read(fd, 65536)
        except OSError: break
tree = subprocess.run(["ps", "-o", "pid,ppid,comm,args", "-g", str(os.getpgid(pid))], capture_output=True, text=True).stdout
os.kill(pid, 9)
text = re.sub(rb"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b[()][A-Z0-9]|\x1b[=>]|\r", b"", buf).decode("utf-8", "replace")
lines = [l.strip() for l in text.split("\n") if l.strip()]
print("== screen (last 25 lines)"); print("\n".join(lines[-25:]))
print("== process tree"); print(tree)
print("== modes requested:", sorted(set(re.findall(rb"\x1b\[\?(\d+)[hl]", buf))))
