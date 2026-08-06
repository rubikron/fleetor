import type { ITheme } from "@xterm/xterm";

// Warm 16-color palette from BUILDING §7 / §176. The app has NO blue anywhere:
// ANSI blue -> gold (#c69a4e), ANSI cyan -> tan (#a98f5f), so claude's TUI sits
// on-theme. Exit-test caveat (§176): if CC's own color semantics suffer, favor
// legibility over theme — flagged for the visual check.
// background/cursorAccent are kept in lockstep with --term-bg in styles.css:
// xterm paints its own canvas from this hardcoded value rather than the CSS
// custom property, and .terminal-host's padding shows the pane's real
// --term-bg around that canvas — if the two drift, every pane gets a visible
// seam where the container color meets the terminal's. --term-bg is
// redeclared per theme (see styles.css's :root and :root[data-theme="light"]
// blocks) specifically so it can track whichever of warmTheme/warmThemeLight
// TerminalPane.tsx currently hands to xterm.
export const warmTheme: ITheme = {
  background: "#201d18",
  foreground: "#e9e7e2",
  cursor: "#d97757",
  cursorAccent: "#201d18",
  selectionBackground: "#45423d",
  selectionForeground: "#e9e7e2",

  black: "#2d2b27",
  red: "#d66f66",
  green: "#7fa36a",
  yellow: "#c69a4e",
  blue: "#c69a4e", // remapped from blue -> gold
  magenta: "#b98a6a",
  cyan: "#a98f5f", // remapped from cyan -> tan
  white: "#c2baae",

  brightBlack: "#8a867c",
  brightRed: "#d97a70",
  brightGreen: "#8fb37a",
  brightYellow: "#d6ab5e",
  brightBlue: "#d6ab5e", // remapped
  brightMagenta: "#c99a7a",
  brightCyan: "#b99f6f", // remapped
  brightWhite: "#e9e7e2",
};

// Light counterpart. Not a straight inversion of warmTheme's hex values —
// same approach as styles.css's light-mode CSS variables: the hue colors are
// deepened so they hold up as text on a cream background instead of near-
// black, and the neutral roles (background/foreground/black/white) are
// re-picked for the new lightness direction rather than swapped 1:1. Bright
// variants stay a *more saturated* step from their normal counterpart, not a
// lighter one — literally lightening them toward the cream background would
// make bold/bright text less legible, the opposite of what "bright" is for.
// brightWhite = foreground (darkest, most prominent), mirroring warmTheme's
// own brightWhite = foreground pattern.
export const warmThemeLight: ITheme = {
  background: "#f7f2ea",
  foreground: "#2b2318",
  cursor: "#a84c2a",
  cursorAccent: "#f7f2ea",
  selectionBackground: "#ded1b6",
  selectionForeground: "#2b2318",

  black: "#4a4032",
  red: "#b23f39",
  green: "#4f7a3d",
  yellow: "#93691f",
  blue: "#93691f", // remapped from blue -> gold
  magenta: "#8f5833",
  cyan: "#7d6226", // remapped from cyan -> tan
  white: "#6b6152",

  brightBlack: "#8c8168",
  brightRed: "#c65950",
  brightGreen: "#628f4e",
  brightYellow: "#a67b2e",
  brightBlue: "#a67b2e", // remapped
  brightMagenta: "#a8683f",
  brightCyan: "#8f7638", // remapped
  brightWhite: "#2b2318",
};
