import { defineConfig, type Plugin, type ViteDevServer } from "vite";
import react from "@vitejs/plugin-react";

/// Where the client posts frontend failures. Dev-only; the plugin below is
/// `apply: "serve"`, so this endpoint does not exist in a production build.
const DEV_LOG_ROUTE = "/__fleetor_dev_log";

/// Cap on a single report's body, so a runaway loop cannot flood the terminal
/// or hold an unbounded string in the dev server.
const MAX_REPORT_BYTES = 16_000;

interface DevReport {
  level?: string;
  message?: string;
  detail?: string;
}

/// Pipes frontend errors into the terminal running `tauri dev`.
///
/// The alternative — an on-screen panel — cannot help in the failure it exists
/// to diagnose. A render that throws unmounts the React root, and this app now
/// starts its window hidden until the frontend reveals it, so the two worst
/// cases are a blank window and no window. An in-app reporter is invisible in
/// both. The terminal is already open, survives the webview dying, and keeps
/// scrollback.
function devLogBridge(): Plugin {
  return {
    name: "fleetor-dev-log",
    apply: "serve",
    configureServer(server: ViteDevServer) {
      server.middlewares.use(DEV_LOG_ROUTE, (req, res) => {
        if (req.method !== "POST") {
          res.statusCode = 405;
          res.end();
          return;
        }

        let body = "";
        let aborted = false;
        req.on("data", (chunk: Buffer) => {
          if (aborted) return;
          body += chunk.toString();
          if (body.length > MAX_REPORT_BYTES) {
            aborted = true;
            body = body.slice(0, MAX_REPORT_BYTES);
          }
        });

        req.on("end", () => {
          // Always 204, even on a malformed body. A logging endpoint that can
          // fail the thing it is logging for would be worse than no endpoint.
          res.statusCode = 204;
          res.end();

          let report: DevReport = {};
          try {
            report = JSON.parse(body) as DevReport;
          } catch {
            report = { level: "error", message: "(unparseable dev report)", detail: body };
          }

          const tag = "\x1b[35m[ui]\x1b[0m"; // magenta — distinct from vite's own output
          const line = `${tag} ${report.message ?? "(no message)"}`;
          const detail = report.detail ? `\n${report.detail}` : "";
          if (report.level === "warn") server.config.logger.warn(line + detail);
          else server.config.logger.error(line + detail);
        });
      });
    },
  };
}

// Frontend lives in ui/; build output goes to dist/ (Tauri's frontendDist).
// React from Phase 4e per BUILDING §2 — the shell is the real product surface now
// (dashboard band, board, live event feed), not the single-terminal 0.5 spike.
export default defineConfig({
  root: "ui",
  plugins: [react(), devLogBridge()],
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
