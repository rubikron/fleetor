// Cmd+ / Cmd- / Cmd+0 application zoom. One factor, read from and persisted
// to localStorage, drives both the CSS pivot (document.documentElement's
// font-size — see styles.css) and every xterm pane's fontSize, the latter via
// the `terminalFontSize` this hook returns and callers pass down as a prop.
//
// The shortcut is registered on `window` in the **capture phase**. This is a
// terminal app — the operator's keyboard focus is almost always inside an
// xterm instance, which attaches its own keydown handling on its own DOM
// node and would otherwise swallow the combo before it bubbles anywhere.
// Capture fires root-to-target, ahead of xterm's own listener, and
// stopPropagation() here during capture keeps the event from ever reaching
// it. preventDefault() also stops the WebView's native page zoom, which
// would otherwise double-apply on top of this one.

import { useCallback, useEffect, useState } from "react";
import {
  ROOT_FONT_SIZE_PX,
  TERMINAL_FONT_SIZE_PX,
  ZOOM_DEFAULT,
  ZOOM_STEPS,
  ZOOM_STORAGE_KEY,
  isValidZoom,
  zoomStepIndex,
  type ZoomStep,
} from "./zoomConstants";

function readStoredZoom(): ZoomStep {
  try {
    const raw = window.localStorage.getItem(ZOOM_STORAGE_KEY);
    if (raw === null) return ZOOM_DEFAULT;
    const parsed: unknown = JSON.parse(raw);
    return isValidZoom(parsed) ? parsed : ZOOM_DEFAULT;
  } catch {
    // Malformed JSON, a non-numeric value, or a storage access error must
    // never crash the shell — treat it exactly like "nothing stored".
    return ZOOM_DEFAULT;
  }
}

function writeStoredZoom(zoom: ZoomStep): void {
  try {
    window.localStorage.setItem(ZOOM_STORAGE_KEY, JSON.stringify(zoom));
  } catch {
    /* best-effort persistence only; zoom still works for the session */
  }
}

function isZoomInKey(e: KeyboardEvent): boolean {
  return e.code === "Equal" || e.code === "NumpadAdd" || e.key === "+" || e.key === "=";
}
function isZoomOutKey(e: KeyboardEvent): boolean {
  return e.code === "Minus" || e.code === "NumpadSubtract" || e.key === "-" || e.key === "_";
}
function isZoomResetKey(e: KeyboardEvent): boolean {
  return e.code === "Digit0" || e.code === "Numpad0" || e.key === "0";
}

export interface ZoomControls {
  /// The current step, e.g. 1.15 for 115%.
  zoom: ZoomStep;
  /// True at the 1.0 step — callers use this to hide zoom-level chrome.
  isDefault: boolean;
  /// ROOT_FONT_SIZE_PX * zoom, already applied to the document root; exposed
  /// for callers that want it (e.g. the top bar's percentage readout).
  rootFontSize: number;
  /// TERMINAL_FONT_SIZE_PX * zoom — pass straight into every TerminalPane.
  terminalFontSize: number;
}

export function useZoom(): ZoomControls {
  const [zoom, setZoom] = useState<ZoomStep>(readStoredZoom);

  // The CSS pivot: every rem-based size in styles.css tracks this one value.
  useEffect(() => {
    document.documentElement.style.fontSize = `${ROOT_FONT_SIZE_PX * zoom}px`;
    writeStoredZoom(zoom);
  }, [zoom]);

  const zoomIn = useCallback(() => {
    setZoom((prev) => ZOOM_STEPS[Math.min(zoomStepIndex(prev) + 1, ZOOM_STEPS.length - 1)]);
  }, []);
  const zoomOut = useCallback(() => {
    setZoom((prev) => ZOOM_STEPS[Math.max(zoomStepIndex(prev) - 1, 0)]);
  }, []);
  const zoomReset = useCallback(() => setZoom(ZOOM_DEFAULT), []);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (!e.metaKey && !e.ctrlKey) return;
      if (isZoomInKey(e)) {
        e.preventDefault();
        e.stopPropagation();
        zoomIn();
      } else if (isZoomOutKey(e)) {
        e.preventDefault();
        e.stopPropagation();
        zoomOut();
      } else if (isZoomResetKey(e)) {
        e.preventDefault();
        e.stopPropagation();
        zoomReset();
      }
    };
    // `true` = capture phase, so this runs before xterm's own keydown
    // handler on the terminal's DOM node.
    window.addEventListener("keydown", handler, true);
    return () => window.removeEventListener("keydown", handler, true);
  }, [zoomIn, zoomOut, zoomReset]);

  return {
    zoom,
    isDefault: zoom === ZOOM_DEFAULT,
    rootFontSize: ROOT_FONT_SIZE_PX * zoom,
    terminalFontSize: TERMINAL_FONT_SIZE_PX * zoom,
  };
}
