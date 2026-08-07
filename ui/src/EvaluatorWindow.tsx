// The evaluator's window (WP-15) — a second Tauri window holding exactly one
// terminal.
//
// **Its own root rather than a view inside `App`**, and the reason is not
// layout. `App` owns `fleet_bootstrap`, the event follower, the dashboard band
// and the start gate; mounting it here would give the app a second webview
// racing the first for the same fleet. So `main.tsx` branches on the window
// label before either root mounts, and this window never learns that any of
// that exists. What it shares with the main window is the parts that are
// genuinely shared: `TerminalPane`, the theme, and `styles.css`.
//
// **§7 rule 5, twice over.** The terminal below is mounted unconditionally for
// the life of the window — there is no `started` gate, no conditional render and
// nothing to switch away to, because unmounting an xterm destroys its buffer and
// there is no screen replay behind a pty. The window itself honours the same
// rule from the Rust side: closing it is intercepted and turned into a hide, so
// the webview and this xterm survive to be shown again (`lib.rs`).
//
// The pane is spawned by `TerminalPane`'s own `started` effect through the same
// `pty_spawn` every other terminal uses, which is idempotent for a pane that is
// already running — so React StrictMode's double mount costs nothing here
// either.

import { useState } from "react";

import { TerminalPane } from "./components/TerminalPane";
import { EVALUATOR, type PaneStatus } from "./fleet/types";
import { useTheme } from "./ui/useTheme";

// Deeper than a worker's 2,000 and deeper than orch's 10,000: this pane reads a
// whole run's archive and prints a retro at the end of it, and the operator
// scrolls back through the reasoning rather than through the last few turns.
const SCROLLBACK = 20000;

export function EvaluatorWindow() {
  const theme = useTheme();
  const [status, setStatus] = useState<PaneStatus>("idle");

  return (
    <div className="evaluator-window">
      <TerminalPane
        pane={EVALUATOR}
        label="review"
        scrollback={SCROLLBACK}
        started
        status={status}
        fontSize={13}
        theme={theme.theme}
        onStatus={(_, next) => setStatus(next)}
      />
    </div>
  );
}
