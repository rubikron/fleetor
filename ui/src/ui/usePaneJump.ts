// Cmd+1..5 pane jumps: Cmd+1 is the orchestrator, Cmd+2..5 are worker-1..4 —
// the same left-to-right order as `ROSTER` in fleet/types.ts, so the digit
// simply indexes into it.
//
// Registered on `window` in the **capture phase**, exactly like
// useZoom.ts / useSidebarCollapse.ts: a terminal holds keyboard focus almost
// always and would otherwise swallow the digit before it bubbles anywhere.
// Digit1-Digit5 don't collide with the zoom shortcuts (Equal/Minus/Digit0 —
// see useZoom.ts's isZoomInKey/isZoomOutKey/isZoomResetKey) or with Cmd+B
// (sidebar — useSidebarCollapse.ts's isSidebarToggleKey).
//
// This hook only recognizes the keystroke and reports which pane was asked
// for; it does not own any navigation state itself (App.tsx already does,
// via usePersistedNav) — the caller decides what "jump to this pane" means
// (switch view, select the worker tab, move terminal focus).

import { useEffect } from "react";
import { ROSTER, type PaneId } from "../fleet/types";

const DIGIT_CODES = ["Digit1", "Digit2", "Digit3", "Digit4", "Digit5"];
const NUMPAD_CODES = ["Numpad1", "Numpad2", "Numpad3", "Numpad4", "Numpad5"];
const DIGIT_KEYS = ["1", "2", "3", "4", "5"];

function rosterIndexForKey(e: KeyboardEvent): number {
  const byCode = DIGIT_CODES.indexOf(e.code);
  if (byCode !== -1) return byCode;
  const byNumpad = NUMPAD_CODES.indexOf(e.code);
  if (byNumpad !== -1) return byNumpad;
  return DIGIT_KEYS.indexOf(e.key);
}

export function usePaneJump(onJump: (pane: PaneId) => void): void {
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (!e.metaKey && !e.ctrlKey) return;
      const idx = rosterIndexForKey(e);
      if (idx === -1) return;
      const pane = ROSTER[idx];
      if (!pane) return;
      e.preventDefault();
      e.stopPropagation();
      onJump(pane);
    };
    // `true` = capture phase, so this runs before xterm's own keydown
    // handler on the terminal's DOM node — same as useZoom.ts.
    window.addEventListener("keydown", handler, true);
    return () => window.removeEventListener("keydown", handler, true);
  }, [onJump]);
}
