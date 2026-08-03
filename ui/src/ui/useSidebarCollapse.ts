// Sidebar collapse: a persisted boolean, toggled by the sidebar's own control
// or Cmd+B, that narrows the nav rail down to a glyph strip. Collapsing is a
// layout change on a flex sibling of the workspace, which is exactly the
// shape of change that resizes the visible terminal pane(s) — App.tsx nudges
// a `resize` event on this state the same way it already does for view and
// worker-tab changes (see the comment there).
//
// The shortcut is registered on `window` in the **capture phase**, same
// reasoning as useZoom.ts's Cmd+/Cmd-/Cmd+0: a terminal almost always holds
// keyboard focus and would otherwise swallow the combo before it bubbles
// anywhere. Cmd+B does not collide with any zoom shortcut (Equal/Minus/Digit0).

import { useCallback, useEffect, useState } from "react";

const STORAGE_KEY = "fleetor:sidebar-collapsed";

function readStoredCollapsed(): boolean {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (raw === null) return false;
    const parsed: unknown = JSON.parse(raw);
    return typeof parsed === "boolean" ? parsed : false;
  } catch {
    // Corrupt JSON, a non-boolean value, or a storage access error must never
    // crash the shell — treat it exactly like "nothing stored" (expanded).
    return false;
  }
}

function writeStoredCollapsed(collapsed: boolean): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(collapsed));
  } catch {
    /* best-effort persistence only; collapse still works for the session */
  }
}

function isSidebarToggleKey(e: KeyboardEvent): boolean {
  return e.code === "KeyB" || e.key === "b" || e.key === "B";
}

export interface SidebarCollapseControls {
  collapsed: boolean;
  toggle: () => void;
}

export function useSidebarCollapse(): SidebarCollapseControls {
  const [collapsed, setCollapsed] = useState<boolean>(readStoredCollapsed);

  useEffect(() => {
    writeStoredCollapsed(collapsed);
  }, [collapsed]);

  const toggle = useCallback(() => setCollapsed((prev) => !prev), []);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (!e.metaKey && !e.ctrlKey) return;
      if (!isSidebarToggleKey(e)) return;
      e.preventDefault();
      e.stopPropagation();
      toggle();
    };
    // `true` = capture phase, so this runs before xterm's own keydown handler
    // on the terminal's DOM node — same as useZoom.ts.
    window.addEventListener("keydown", handler, true);
    return () => window.removeEventListener("keydown", handler, true);
  }, [toggle]);

  return { collapsed, toggle };
}
