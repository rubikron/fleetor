// Application zoom: a single factor drives both the CSS type-scale pivot
// (styles.css :root font-size) and every xterm pane's fontSize option. Kept
// as a discrete ladder rather than free-float continuous zoom so the operator
// always lands on a value both scales render cleanly at, and so the ends can
// be clamped to something still usable.

export const ZOOM_STEPS = [0.8, 0.9, 1.0, 1.15, 1.3, 1.5, 1.75, 2.0] as const;
export type ZoomStep = (typeof ZOOM_STEPS)[number];

export const ZOOM_DEFAULT: ZoomStep = 1.0;
export const ZOOM_MIN: ZoomStep = ZOOM_STEPS[0];
export const ZOOM_MAX: ZoomStep = ZOOM_STEPS[ZOOM_STEPS.length - 1];

// Base sizes the zoom factor scales from — the CSS pivot (see the "ZOOM
// PIVOT" comment in styles.css) and each xterm instance's own fontSize.
//
// Rebased one ladder step up (16→18, 13→15 — the old 1.15 rung) because the
// original baseline read too small in the app's real window. This is a rebase,
// not a new default zoom: 100% now *is* what used to be 115%, so the readout
// stays honest and the operator still has the full ladder in both directions
// from where they actually sit. Anything already in localStorage multiplies
// against the new base, so a stored 1.15 comes back visibly bigger once —
// acceptable for a value the operator re-sets with one keystroke.
//
// styles.css's :root font-size must match ROOT_FONT_SIZE_PX: it is the
// pre-JS default, and a mismatch shows up as a flash of the wrong scale on
// every launch.
export const ROOT_FONT_SIZE_PX = 18;
export const TERMINAL_FONT_SIZE_PX = 15;

export const ZOOM_STORAGE_KEY = "fleetor:zoom";

/// Guards a value read back from localStorage. A corrupted or out-of-range
/// stored value must fall back to the default, never reach the UI as-is.
export function isValidZoom(value: unknown): value is ZoomStep {
  return typeof value === "number" && (ZOOM_STEPS as readonly number[]).includes(value);
}

export function zoomStepIndex(zoom: number): number {
  const idx = (ZOOM_STEPS as readonly number[]).indexOf(zoom);
  return idx === -1 ? ZOOM_STEPS.indexOf(ZOOM_DEFAULT) : idx;
}
