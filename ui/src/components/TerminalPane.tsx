// The orchestrator pane: the operator's real `claude` TUI through the pty bridge,
// now running as the fleet **lead** wired to the hub (Phase 4e-2). The xterm view
// mounts immediately, but the lead process is NOT spawned until the operator
// explicitly starts the session — spawning it runs their Opus and spends tokens,
// so a start gate (with the target + worker backend spelled out) sits over the
// pane until then. The Claude Code TUI itself is untouched.

import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "@xterm/xterm/css/xterm.css";
import { warmTheme } from "../theme";
import { spawnLead } from "../fleet/api";
import type { FleetConfig } from "../fleet/types";

// Raw pty bytes arrive base64-encoded so escape sequences and multibyte UTF-8
// never split across a chunk boundary.
function decodeBase64(b64: string): Uint8Array {
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return bytes;
}

// The lead inbox: relayed worker→lead messages are typed into the TUI only after a
// quiet gap, so an injection never clobbers a line the operator is composing.
const INJECT_IDLE_MS = 1500;
const INJECT_FLUSH_MS = 400;

interface TerminalPaneProps {
  started: boolean;
  onStart: () => void;
  config: FleetConfig | null;
}

export function TerminalPane({ started, onStart, config }: TerminalPaneProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  // Relayed worker→lead messages waiting to be typed into the TUI, plus the last
  // time the operator typed — the idle guard the flush loop keys off of.
  const injectQueue = useRef<string[]>([]);
  const lastInputAt = useRef(0);
  const startedRef = useRef(started);
  useEffect(() => {
    startedRef.current = started;
  }, [started]);

  // Mount the xterm view once. No pty is spawned here — that waits for `started`.
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
    termRef.current = term;

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
    // Worker→lead traffic the backend pump relays — queued, typed in when idle.
    void listen<string>("lead://inject", (e) => injectQueue.current.push(e.payload)).then((un) => {
      if (disposed) un();
      else disposers.push(un);
    });

    const onData = term.onData((data) => {
      lastInputAt.current = Date.now();
      void invoke("pty_write", { data });
    });
    const onResize = term.onResize(({ rows, cols }) => void invoke("pty_resize", { rows, cols }));

    // Type one queued worker message per tick — but only once the session is live
    // and the operator has been quiet for a beat, so a relayed message becomes its
    // own submitted turn (the trailing "\r") without garbling anything mid-compose.
    const flush = setInterval(() => {
      if (!startedRef.current || injectQueue.current.length === 0) return;
      if (Date.now() - lastInputAt.current < INJECT_IDLE_MS) return;
      const next = injectQueue.current.shift();
      if (next != null) void invoke("pty_write", { data: `${next}\r` });
    }, INJECT_FLUSH_MS);

    const refit = () => {
      // While the pane is on a hidden tab its box is 0×0; fitting then would
      // collapse claude's grid. Only refit when it actually has a size.
      if (host.clientWidth === 0 || host.clientHeight === 0) return;
      try {
        fit.fit();
      } catch {
        /* not measurable yet */
      }
    };
    const observer = new ResizeObserver(refit);
    observer.observe(host);
    window.addEventListener("resize", refit);

    return () => {
      disposed = true;
      termRef.current = null;
      clearInterval(flush);
      observer.disconnect();
      window.removeEventListener("resize", refit);
      onData.dispose();
      onResize.dispose();
      disposers.forEach((un) => un());
      term.dispose();
    };
  }, []);

  // Spawn the lead only once the operator has started the session (the gate). The
  // pty command is idempotent, so a re-run after a spurious flip is harmless.
  useEffect(() => {
    if (!started) return;
    const term = termRef.current;
    if (!term) return;
    void spawnLead(term.rows, term.cols);
  }, [started]);

  return (
    <div className="terminal-pane">
      <div className="pane__head">
        <span className="mono">orchestrator · claude</span>
        {started && <span className="pane__live">live</span>}
      </div>
      <div className="terminal-wrap">
        <div ref={hostRef} className="terminal-host" />
        {!started && <SessionGate onStart={onStart} config={config} />}
      </div>
    </div>
  );
}

/// The explicit spend gate: nothing runs until the operator starts the session.
function SessionGate({ onStart, config }: { onStart: () => void; config: FleetConfig | null }) {
  const target = config?.target ?? "the scratch repo";
  const backend = config?.worker_backend ?? "fake";
  const lead = config?.lead_model ?? "opus (operator)";
  return (
    <div className="pane-gate">
      <div className="pane-gate__card">
        <h3 className="pane-gate__title">Start orchestrator session</h3>
        <p className="pane-gate__body">
          Spawns your real <span className="mono">claude</span> ({lead}) as the fleet lead in{" "}
          <span className="mono">{target}</span>, wired to the hub. It will spend tokens.
        </p>
        <ul className="pane-gate__facts">
          <li>
            <span className="k">workers</span>
            <span className={`v ${backend === "flash" ? "v--gold" : ""}`}>
              {backend === "flash" ? "real DeepSeek Flash (costs tokens)" : "fake (free proof path)"}
            </span>
          </li>
          <li>
            <span className="k">target</span>
            <span className="v mono">{target}</span>
          </li>
        </ul>
        <button className="pane-gate__go" onClick={onStart}>
          Start session
        </button>
      </div>
    </div>
  );
}
