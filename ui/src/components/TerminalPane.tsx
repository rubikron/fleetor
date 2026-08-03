// One pane: a real `claude` TUI on the far end of a pty, rendered by xterm.
//
// The component is driven entirely by props — which pane it is, how much
// scrollback it keeps, and whether the fleet has been started — so the same code
// renders the operator's own orchestrator and each of the four workers.
//
// **It is never unmounted.** Hidden worker tabs stay in the DOM behind
// `.is-hidden`, because there is no screen-replay mechanism in the backend:
// unmount an xterm and its buffer is destroyed, `pty_spawn` no-ops on the still
// running child, and the pane comes back blank until something repaints it (L7).
//
// **Nothing here queues, delays or rate-limits.** The injection apparatus this
// file used to carry — a 1500 ms operator-idle guard and a 400 ms one-message
// flusher — is gone with the headless fleet, and delivery now goes straight from
// the hub to the pty with nothing in between (D-034).

import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { warmTheme } from "../theme";
import { onPaneExit, onPaneOutput, resizePane, spawnPane, writePane } from "../fleet/api";
import type { PaneId, PaneStatus } from "../fleet/types";

// Raw pty bytes arrive base64-encoded so escape sequences and multibyte UTF-8
// never split across a chunk boundary.
function decodeBase64(b64: string): Uint8Array {
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return bytes;
}

const DIM = "\x1b[38;2;138;134;124m";
const RESET = "\x1b[0m";

interface TerminalPaneProps {
  pane: PaneId;
  label: string;
  /// Workers keep less history than the orchestrator: five xterm buffers at
  /// 10 000 lines each is memory spent on scrollback nobody reads.
  scrollback: number;
  /// The operator has started the fleet — this pane may spend tokens.
  started: boolean;
  onStatus: (pane: PaneId, status: PaneStatus) => void;
  /// Rendered in the pane head; the per-pane restart when one wedges.
  onRestart?: () => void;
}

export function TerminalPane({
  pane,
  label,
  scrollback,
  started,
  onStatus,
  onRestart,
}: TerminalPaneProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  // Held in a ref so the mount effect never re-runs when App re-renders: tearing
  // the xterm down to swap a callback would destroy the buffer (L7).
  const onStatusRef = useRef(onStatus);
  onStatusRef.current = onStatus;

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
      scrollback,
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
    const keep = (un: () => void) => (disposed ? un() : disposers.push(un));

    void onPaneOutput(pane, (payload) => {
      term.write(decodeBase64(payload));
      onStatusRef.current(pane, "live");
    }).then(keep);
    void onPaneExit(pane, () => {
      term.write(`\r\n${DIM}[${label} exited]${RESET}\r\n`);
      onStatusRef.current(pane, "dead");
    }).then(keep);

    // A rejected write must be visible. Both call sites used to be bare
    // `void invoke(...)`, so a failure vanished into an unhandled promise — no
    // event, no log, no console.
    const onData = term.onData((data) => {
      void writePane(pane, data).catch((e) => term.write(`\r\n${DIM}[input refused: ${e}]${RESET}\r\n`));
    });
    const onResize = term.onResize(({ rows, cols }) => void resizePane(pane, rows, cols).catch(() => {}));

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
      observer.disconnect();
      window.removeEventListener("resize", refit);
      onData.dispose();
      onResize.dispose();
      disposers.forEach((un) => un());
      term.dispose();
    };
  }, [pane, label, scrollback]);

  // Spawn once the operator has started the fleet. The command is idempotent, so
  // a re-run after a spurious flip is harmless; a failure is written into the
  // pane itself, because a pane that silently never starts is the worst outcome.
  useEffect(() => {
    if (!started) return;
    const term = termRef.current;
    if (!term) return;
    void spawnPane(pane, term.rows, term.cols).catch((e) => {
      term.write(`\r\n${DIM}[${label} could not start — ${e}]${RESET}\r\n`);
      onStatusRef.current(pane, "dead");
    });
  }, [started, pane, label]);

  return (
    <div className="terminal-pane">
      <div className="pane__head">
        <span className="mono">{label}</span>
        <span className="grow" style={{ flex: "1 1 auto" }} />
        {started && onRestart && (
          <button className="pane__ctl" onClick={onRestart}>
            Restart
          </button>
        )}
      </div>
      <div className="terminal-wrap">
        <div ref={hostRef} className="terminal-host" />
      </div>
    </div>
  );
}
