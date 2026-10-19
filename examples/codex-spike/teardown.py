"""codex 0.163's app-server daemons detach from the probe (own group, ppid 1) and outlive
it, so a probe kills whatever still runs out of its scratch home when it is done."""
import os
import signal
import subprocess


def kill_from(home):
    root = os.path.realpath(home) + os.sep
    try:
        ps = subprocess.run(["ps", "-axo", "pid=,command="], capture_output=True, text=True).stdout
    except OSError:
        return
    for line in ps.splitlines():
        pid, _, command = line.strip().partition(" ")
        if command.startswith(root) and int(pid) != os.getpid():
            try:
                os.kill(int(pid), signal.SIGKILL)
            except (ProcessLookupError, PermissionError):
                pass
