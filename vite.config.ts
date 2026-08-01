import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Frontend lives in ui/; build output goes to dist/ (Tauri's frontendDist).
// React from Phase 4e per BUILDING §2 — the shell is the real product surface now
// (dashboard band, board, live event feed), not the single-terminal 0.5 spike.
export default defineConfig({
  root: "ui",
  plugins: [react()],
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
