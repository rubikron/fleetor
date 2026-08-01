import { defineConfig } from "vite";

// Spike frontend lives in ui/; build output goes to dist/ (consumed by Tauri's
// frontendDist). Plain TS + xterm.js — no React here by intent: the spike is a
// single full-window terminal, so a framework would be dead weight (KISS). The
// real shell (Phase 4) adopts React per BUILDING §2.
export default defineConfig({
  root: "ui",
  build: {
    outDir: "../dist",
    emptyOutDir: true,
  },
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
});
