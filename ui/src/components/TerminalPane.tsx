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

import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import "@xterm/xterm/css/xterm.css";
import { warmTheme, warmThemeLight } from "../theme";
import { onPaneExit, onPaneOutput, resizePane, spawnPane, writePane } from "../fleet/api";
import { statusTone, STATUS_LABEL } from "../lib/statusTone";
import { PaneGauge } from "./PaneGauge";
import type { GaugeReading, PaneId, PaneStatus } from "../fleet/types";
import type { Theme } from "../ui/useTheme";

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

/// How long the pane waits for a resize burst to settle before refitting.
/// Long enough to swallow a whole sidebar-collapse animation (~140ms of
/// per-frame ResizeObserver callbacks) into a single fit, short enough that a
/// window drag still feels immediate. Trailing edge only.
const REFIT_SETTLE_MS = 60;

interface TerminalPaneProps {
  pane: PaneId;
  label: string;
  /// Workers keep less history than the orchestrator: five xterm buffers at
  /// 10 000 lines each is memory spent on scrollback nobody reads.
  scrollback: number;
  /// The operator has started the fleet — this pane may spend tokens.
  started: boolean;
  /// This pane's live status, shown as a dot + word in its own head — the
  /// detail the dashboard band used to duplicate now lives only here and in
  /// the worker tab strip.
  status: PaneStatus;
  /// The model running this pane, shown alongside the label when known.
  model?: string;
  /// What this pane's rail says about its context (WP-04, #48) — a reading,
  /// not a gauge. Never rendered as 0%, and an absent figure is no longer
  /// rendered as nothing: `unavailable` says the word, `pending` says the
  /// pane has not been asked yet, and only the orchestrator — out of scope by
  /// construction — shows nothing at all. `undefined` means `pending`.
  gauge?: GaugeReading;
  /// xterm's fontSize in px, driven by the app-wide zoom factor
  /// (TERMINAL_FONT_SIZE_PX * zoom — see ui/useZoom.ts). Read once as the
  /// initial value at mount; changes afterward are applied by a separate
  /// effect below, never by re-running the mount effect (L8).
  fontSize: number;
  /// The app-wide light/dark preference (ui/useTheme.ts). Same treatment as
  /// fontSize: read once at mount for the Terminal's initial theme, then
  /// re-applied by its own effect below on every change — never folded into
  /// the mount effect's deps (L8).
  theme: Theme;
  onStatus: (pane: PaneId, status: PaneStatus) => void;
  /// Rendered in the pane head; the per-pane restart when one wedges.
  onRestart?: () => void;
  /// Called once, after mount, with a function that moves keyboard focus
  /// into this pane's terminal. App.tsx keeps one of these per pane so its
  /// Cmd+1..5 pane-jump shortcut (ui/usePaneJump.ts) can focus the terminal
  /// it just switched to.
  onFocusReady?: (focus: () => void) => void;
}

export function TerminalPane({
  pane,
  label,
  scrollback,
  started,
  status,
  model,
  gauge,
  fontSize,
  theme,
  onStatus,
  onRestart,
  onFocusReady,
}: TerminalPaneProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  // Held in a ref so the mount effect never re-runs when App re-renders: tearing
  // the xterm down to swap a callback would destroy the buffer (L7).
  const onStatusRef = useRef(onStatus);
  onStatusRef.current = onStatus;
  // Same reasoning, for the zoom-driven fontSize: the mount effect below reads
  // this only once, as the terminal's initial size, and must never re-run when
  // it changes (L8) — a separate effect further down handles updates.
  const fontSizeRef = useRef(fontSize);
  fontSizeRef.current = fontSize;
  const themeRef = useRef(theme);
  themeRef.current = theme;
  const onFocusReadyRef = useRef(onFocusReady);
  onFocusReadyRef.current = onFocusReady;

  // Whether this pane's terminal currently owns keyboard focus — drives the
  // always-on focused-pane frame (five live terminals share one keyboard;
  // nothing else shows which one is listening).
  const [isFocused, setIsFocused] = useState(false);
  // Whether the operator has scrolled up in this pane's scrollback — drives
  // the "jump to bottom" affordance (see the onScroll listener below).
  const [scrolledUp, setScrolledUp] = useState(false);

  // Mount the xterm view once. No pty is spawned here — that waits for `started`.
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const term = new Terminal({
      theme: themeRef.current === "light" ? warmThemeLight : warmTheme,
      fontFamily: 'ui-monospace, "SF Mono", Menlo, monospace',
      fontSize: fontSizeRef.current,
      lineHeight: 1.2,
      cursorBlink: true,
      scrollback,
      allowProposedApi: true,
      macOptionIsMeta: true,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host);

    // Render on the GPU. Without this addon xterm falls back to its DOM
    // renderer — a <span> per styled run, per row, per terminal — and this app
    // has five of them live at once. That cost is paid on every repaint, so it
    // shows up as sluggish typing and scrolling, and it is why any layout
    // animation over the panes (the sidebar rail, a divider drag, a window
    // resize) drops frames: the browser re-lays-out and repaints that whole
    // DOM tree every frame.
    //
    // Must be loaded AFTER term.open() — the addon needs a rendered element to
    // attach its canvas to.
    let webgl: WebglAddon | null = null;
    try {
      webgl = new WebglAddon();
      // A lost GPU context leaves the terminal permanently blank if we hold on
      // to a dead addon. Disposing it drops xterm back to the DOM renderer —
      // slower, but visible, which is the only thing that matters here.
      webgl.onContextLoss(() => {
        webgl?.dispose();
        webgl = null;
      });
      term.loadAddon(webgl);
    } catch {
      // No WebGL (software rendering, a driver blocklist, an exhausted context
      // pool). The DOM renderer still works; this is a performance
      // optimization, never a requirement.
      webgl = null;
    }

    fit.fit();
    termRef.current = term;
    fitRef.current = fit;

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

    // xterm already preserves scroll position on new output on its own: it
    // only re-pins the viewport to the bottom when the viewport was already
    // there (see @xterm/xterm's BufferService — `isUserScrolling` gates
    // whether a write advances `ydisp`). `onScroll` fires on every such
    // adjustment — both from a manual scroll and from a write while
    // scrolled up — so comparing viewportY to baseY here is enough to drive
    // a quiet "jump to bottom" affordance; no extra scroll-locking is needed.
    const onScroll = term.onScroll(() => {
      const buf = term.buffer.active;
      setScrolledUp(buf.viewportY < buf.baseY);
    });

    // A resize is a burst, not an event. The sidebar's collapse animates over
    // ~140ms and a divider drag runs for as long as the mouse is held, so a
    // raw ResizeObserver fires once per frame throughout. Every fit() resizes
    // the pty, and `claude` repaints its entire TUI on each one — a dozen
    // full repaints inside one collapse animation is exactly the flicker this
    // coalescing exists to prevent. Trailing-only, so the geometry that lands
    // is the settled one rather than an intermediate frame.
    let refitTimer: number | undefined;
    const refit = () => {
      window.clearTimeout(refitTimer);
      refitTimer = window.setTimeout(() => {
        // While the pane is on a hidden tab its box is 0×0; fitting then would
        // collapse claude's grid. Only refit when it actually has a size.
        if (host.clientWidth === 0 || host.clientHeight === 0) return;
        try {
          fit.fit();
        } catch {
          /* not measurable yet */
        }
      }, REFIT_SETTLE_MS);
    };
    const observer = new ResizeObserver(refit);
    observer.observe(host);
    window.addEventListener("resize", refit);

    return () => {
      disposed = true;
      termRef.current = null;
      fitRef.current = null;
      observer.disconnect();
      window.clearTimeout(refitTimer);
      window.removeEventListener("resize", refit);
      onData.dispose();
      onResize.dispose();
      onScroll.dispose();
      disposers.forEach((un) => un());
      // Before term.dispose(), so the GPU context is released rather than
      // leaked — a browser only allows so many live WebGL contexts, and this
      // app opens five.
      webgl?.dispose();
      term.dispose();
    };
  }, [pane, label, scrollback]);

  // Focused-pane frame: a new, separate effect from the mount effect above.
  // xterm's own focus target is its internal textarea, created during
  // `term.open()` there, so this effect (which only runs once, after that
  // mount effect has already run) reads it from `termRef.current` rather
  // than adding anything to the mount effect itself — attaching listeners
  // here can never risk re-running that effect's deps and tearing the
  // buffer down (L7/L8).
  useEffect(() => {
    const textarea = termRef.current?.textarea;
    if (!textarea) return;
    const handleFocus = () => setIsFocused(true);
    const handleBlur = () => setIsFocused(false);
    textarea.addEventListener("focus", handleFocus);
    textarea.addEventListener("blur", handleBlur);
    return () => {
      textarea.removeEventListener("focus", handleFocus);
      textarea.removeEventListener("blur", handleBlur);
    };
  }, []);

  // Registers a `focus()` callback for App.tsx's Cmd+1..5 pane-jump shortcut
  // (ui/usePaneJump.ts). Also a separate, mount-once effect, for the same
  // L7/L8 reason as above — it must never join the mount effect's deps.
  useEffect(() => {
    onFocusReadyRef.current?.(() => termRef.current?.focus());
  }, []);

  // Zoom: a separate, additional effect from the mount effect above. It only
  // mutates the existing xterm instance's fontSize and refits — it must never
  // be folded into the mount effect's deps, because that effect constructs
  // and disposes the Terminal, and re-running it on every zoom change would
  // tear down and recreate the xterm, wiping the scrollback buffer (L7/L8).
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    term.options.fontSize = fontSize;
    const host = hostRef.current;
    // Same 0×0 guard as `refit` above: a hidden worker pane can't be fit
    // while it measures zero, and forcing it would collapse the grid. The
    // fontSize is still applied, so when the pane next becomes visible —
    // App.tsx's view/selectedWorker effect dispatches a `resize` event on
    // exactly that transition — the mount effect's own `resize` listener
    // calls `fit.fit()` again, this time against the already-updated size.
    if (!host || host.clientWidth === 0 || host.clientHeight === 0) return;
    try {
      fitRef.current?.fit();
    } catch {
      /* not measurable yet — a later resize/refit will catch up */
    }
  }, [fontSize]);

  // Theme: same shape as the fontSize effect above and for the same reason —
  // it only mutates the live xterm instance's `theme` option, never the
  // mount effect's deps, so switching light/dark never tears down and
  // recreates the terminal (L7/L8). xterm's options object applies a new
  // theme immediately, no refit needed.
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    term.options.theme = theme === "light" ? warmThemeLight : warmTheme;
  }, [theme]);

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

  // Clicking anywhere in the pane — its head, its padding, its frame, the
  // xterm surface itself — moves keyboard focus into the terminal, except
  // the Restart button, which needs its own click uncontested. `onClick`
  // (not `onMouseDown`) so a text-selection drag inside xterm finishes
  // before this fires; xterm tracks selection itself rather than relying on
  // the browser's native selection, so re-focusing afterward doesn't clear it.
  const focusTerminal = (e: React.MouseEvent) => {
    const target = e.target as HTMLElement;
    if (target.closest(".pane__ctl")) return;
    termRef.current?.focus();
  };

  return (
    <div
      className={`terminal-pane ${isFocused ? "terminal-pane--focused" : ""}`}
      onClick={focusTerminal}
    >
      <div className="pane__head">
        <span className={`dot dot--${statusTone(status)}`} />
        <span className="mono pane__title">{label}</span>
        {model && <span className="mono pane__meta">{model}</span>}
        <PaneGauge reading={gauge} block="pane__gauge" className="mono pane__meta" />
        <span className="grow" style={{ flex: "1 1 auto" }} />
        <span className="pane__status">{STATUS_LABEL[status]}</span>
        {started && onRestart && (
          <button className="pane__ctl" onClick={onRestart}>
            Restart
          </button>
        )}
      </div>
      <div className="terminal-wrap">
        <div ref={hostRef} className="terminal-host" />
        {scrolledUp && (
          <button
            className="terminal-jump"
            onClick={() => termRef.current?.scrollToBottom()}
          >
            ↓ jump to latest
          </button>
        )}
      </div>
    </div>
  );
}
