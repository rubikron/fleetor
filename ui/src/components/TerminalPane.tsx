// The orchestrator pane: the real `claude` TUI through the Phase 0.5 pty bridge,
// unchanged, now hosted inside the React shell. In 4e-1 it runs a plain idle
// session in a scratch cwd (the retained spike); 4e-2 points it at the lead seat
// wired to the hub. If `claude` isn't on PATH the pane simply reports it exited —
// the rest of the shell stays fully usable.

import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "@xterm/xterm/css/xterm.css";
import { warmTheme } from "../theme";

// Raw pty bytes arrive base64-encoded so escape sequences and multibyte UTF-8
// never split across a chunk boundary.
function decodeBase64(b64: string): Uint8Array {
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return bytes;
}

export function TerminalPane() {
  const hostRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const term = new Terminal({
      theme: warmTheme,
      fontFamily: 'ui-monospace, "SF Mono", Menlo, monospace',
      fontSize: 13,
      lineHeight: 1.2,
      cursorBlink: true,
      scrollback: 10000,
      allowProposedApi: true,
      macOptionIsMeta: true,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host);
    fit.fit();

    const disposers: Array<() => void> = [];
    let disposed = false;

    void listen<string>("pty://output", (e) => term.write(decodeBase64(e.payload))).then((un) => {
      if (disposed) un();
      else disposers.push(un);
    });
    void listen("pty://exit", () => {
      term.write("\r\n\x1b[38;2;138;134;124m[claude exited — orchestrator seat idle]\x1b[0m\r\n");
    }).then((un) => {
      if (disposed) un();
      else disposers.push(un);
    });

    const onData = term.onData((data) => void invoke("pty_write", { data }));
    const onResize = term.onResize(({ rows, cols }) => void invoke("pty_resize", { rows, cols }));

    const refit = () => {
      try {
        fit.fit();
      } catch {
        /* not measurable yet */
      }
    };
    const observer = new ResizeObserver(refit);
    observer.observe(host);
    window.addEventListener("resize", refit);

    void invoke("pty_spawn", { rows: term.rows, cols: term.cols });

    return () => {
      disposed = true;
      observer.disconnect();
      window.removeEventListener("resize", refit);
      onData.dispose();
      onResize.dispose();
      disposers.forEach((un) => un());
      term.dispose();
    };
  }, []);

  return (
    <div className="terminal-pane">
      <div className="pane__head">
        <span className="mono">orchestrator · claude</span>
      </div>
      <div ref={hostRef} className="terminal-host" />
    </div>
  );
}
