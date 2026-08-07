// Dev mode, as the UI sees it (WP-16).
//
// Deliberately *not* the useTheme.ts / usePersistedNav.ts shape. Those read and
// write localStorage, because a theme is a webview preference and nothing on the
// Rust side ever asks what it is. Dev mode is asked about by code that has no
// webview — WP-15's evaluator window, WP-17's fence — so the flag lives in
// ~/.fleetor/config.json and this hook is a *view* of it, never a second copy.
//
// Three states, and the third is why `enabled` is not a bare boolean at mount:
// on, off, and not-yet-answered. Rendering "off" while the answer is in flight
// would flash the app out of a posture it is actually in, which is the one thing
// the banner exists to prevent — so the banner waits for a real answer instead.

import { useCallback, useEffect, useState } from "react";
import { fetchDevMode, setDevMode } from "../fleet/api";

export interface DevModeControls {
  /// `null` until the backend has answered — neither on nor off yet.
  enabled: boolean | null;
  /// Persist the opposite of the current state. A failure leaves the switch
  /// where it was and puts a sentence in `error`; it never optimistically
  /// renders a mode that did not persist.
  toggle: () => void;
  /// Why the last toggle did not take, if it did not.
  error: string | null;
}

function why(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : "dev mode could not be saved";
}

export function useDevMode(): DevModeControls {
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    fetchDevMode()
      .then((value) => {
        if (live) setEnabled(value);
      })
      .catch((e: unknown) => {
        if (!live) return;
        // An unreadable config is off on the Rust side too — but say why, or a
        // switch that will not move looks like a broken switch.
        setEnabled(false);
        setError(why(e));
      });
    return () => {
      live = false;
    };
  }, []);

  // No optimistic flip: the switch moves when the write lands, not when it is
  // requested. A mode that persists is the whole feature, so a switch that shows
  // "on" over a config that says otherwise would be showing the wrong app.
  const toggle = useCallback(() => {
    if (enabled === null) return; // nothing to flip yet
    setError(null);
    setDevMode(!enabled)
      .then((stored) => setEnabled(stored))
      .catch((e: unknown) => setError(why(e)));
  }, [enabled]);

  return { enabled, toggle, error };
}
