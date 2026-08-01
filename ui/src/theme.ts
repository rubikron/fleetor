import type { ITheme } from "@xterm/xterm";

// Warm 16-color palette from BUILDING §7 / §176. The app has NO blue anywhere:
// ANSI blue -> gold (#c69a4e), ANSI cyan -> tan (#a98f5f), so claude's TUI sits
// on-theme. Exit-test caveat (§176): if CC's own color semantics suffer, favor
// legibility over theme — flagged for the visual check.
export const warmTheme: ITheme = {
  background: "#1f1e1b",
  foreground: "#e8e6e1",
  cursor: "#d97757",
  cursorAccent: "#1f1e1b",
  selectionBackground: "#45423d",
  selectionForeground: "#e8e6e1",

  black: "#2d2b27",
  red: "#c25d4f",
  green: "#7fa36a",
  yellow: "#c69a4e",
  blue: "#c69a4e", // remapped from blue -> gold
  magenta: "#b98a6a",
  cyan: "#a98f5f", // remapped from cyan -> tan
  white: "#b8b4ab",

  brightBlack: "#8a867c",
  brightRed: "#d06a5b",
  brightGreen: "#8fb37a",
  brightYellow: "#d6ab5e",
  brightBlue: "#d6ab5e", // remapped
  brightMagenta: "#c99a7a",
  brightCyan: "#b99f6f", // remapped
  brightWhite: "#e8e6e1",
};
