// Persists the operator's light/dark preference and applies it as
// `data-theme` on the document root, where styles.css's `:root[data-theme=
// "light"]` block picks it up. Same validated-read pattern as
// useSidebarCollapse.ts: a corrupt or missing stored
// value must fall back to the default and never throw or wedge the app.

import { useCallback, useEffect, useState } from "react";
import { THEME_MORPH_MS } from "../theme";

const STORAGE_KEY = "fleetor:theme";

export type Theme = "dark" | "light";

// Dark stays the default — it's the palette every other design decision in
// styles.css was tuned against; light is an opt-in the operator reaches for.
const DEFAULT_THEME: Theme = "dark";

function isValidTheme(value: unknown): value is Theme {
  return value === "dark" || value === "light";
}

function readStoredTheme(): Theme {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (raw === null) return DEFAULT_THEME;
    const parsed: unknown = JSON.parse(raw);
    return isValidTheme(parsed) ? parsed : DEFAULT_THEME;
  } catch {
    return DEFAULT_THEME;
  }
}

function writeStoredTheme(theme: Theme): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(theme));
  } catch {
    /* best-effort persistence only; the theme still applies for the session */
  }
}

// Marks the root as mid-switch for the length of the morph, so styles.css's
// `:root[data-theme-morph]` transition runs for a switch the operator makes and
// never for the stored theme applied at launch — which would otherwise morph in
// from dark on every start. Called before the state change, which is what puts
// it on the root before `data-theme` flips. Reduced motion skips it, and the
// switch snaps as it always did.
let morphTimer: number | undefined;
function beginMorph(): void {
  if (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) return;
  const root = document.documentElement;
  root.dataset.themeMorph = "";
  window.clearTimeout(morphTimer);
  morphTimer = window.setTimeout(() => {
    delete root.dataset.themeMorph;
  }, THEME_MORPH_MS + 50);
}

export interface ThemeControls {
  theme: Theme;
  setTheme: (theme: Theme) => void;
  toggle: () => void;
}

export function useTheme(): ThemeControls {
  const [theme, setThemeState] = useState<Theme>(readStoredTheme);

  useEffect(() => {
    writeStoredTheme(theme);
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  const setTheme = useCallback((next: Theme) => {
    beginMorph();
    setThemeState(next);
  }, []);
  const toggle = useCallback(() => {
    beginMorph();
    setThemeState((prev) => (prev === "dark" ? "light" : "dark"));
  }, []);

  return { theme, setTheme, toggle };
}
