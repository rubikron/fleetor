import type { ITheme } from "@xterm/xterm";

// Warm 16-color palette from BUILDING §7 / §176. The app has NO blue anywhere:
// ANSI blue -> gold (#c69a4e), ANSI cyan -> tan (#a98f5f), so claude's TUI sits
// on-theme. Exit-test caveat (§176): if CC's own color semantics suffer, favor
// legibility over theme — flagged for the visual check.
// background/cursorAccent are kept in lockstep with --bg-1 in styles.css:
// xterm paints its own canvas from this hardcoded value rather than the CSS
// custom property, and .terminal-host's padding shows the pane's real
// --bg-1 around that canvas — if the two drift, every pane gets a visible
// seam where the container color meets the terminal's.
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
