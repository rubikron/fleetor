// Frontend failures, reported to the terminal running `tauri dev` rather than
// to the screen.
//
// The on-screen version of this could not help in the failure it existed to
// diagnose. A render that throws unmounts the React root, and the window is now
// created hidden until the frontend reveals it (see useWindowState / lib.rs) —
// so the two worst outcomes are a blank window and no window at all, and an
// in-app reporter is invisible in both. The terminal is already open, it
// survives the webview dying, and it keeps scrollback.
//
// The receiving end is the `fleetor-dev-log` plugin in vite.config.ts, which is
// `apply: "serve"` — the endpoint does not exist in a production build, and
// every call here is behind `import.meta.env.DEV`, so none of this ships.

const DEV_LOG_ROUTE = "/__fleetor_dev_log";

export type DevLogLevel = "warn" | "error";

/// Fire-and-forget. Never awaited, never surfaced, never allowed to throw:
/// a reporter that can break the thing it is reporting on is worse than no
/// reporter. `keepalive` so a report survives the page being torn down, which
/// is exactly when the interesting failures happen.
export function devLog(level: DevLogLevel, message: string, detail?: unknown): void {
  if (!import.meta.env.DEV) return;
  try {
    void fetch(DEV_LOG_ROUTE, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ level, message, detail: formatDetail(detail) }),
      keepalive: true,
    }).catch(() => {
      /* the dev server is gone; nothing useful left to do */
    });
  } catch {
    /* serialisation or fetch construction failed — still not worth raising */
  }
}

function formatDetail(detail: unknown): string | undefined {
  if (detail === undefined || detail === null) return undefined;
  if (detail instanceof Error) return detail.stack ?? `${detail.name}: ${detail.message}`;
  if (typeof detail === "string") return detail;
  try {
    return JSON.stringify(detail, null, 2);
  } catch {
    return String(detail);
  }
}

/// Catches what a React error boundary cannot: async throws, rejected promises,
/// and anything raised outside the render cycle — an effect's callback, a Tauri
/// event handler, a pty listener.
export function installDevErrorReporting(): void {
  if (!import.meta.env.DEV) return;

  window.addEventListener("error", (e) => {
    devLog("error", `uncaught: ${e.message}`, e.error ?? `${e.filename}:${e.lineno}:${e.colno}`);
  });

  window.addEventListener("unhandledrejection", (e) => {
    devLog("error", "unhandled promise rejection", e.reason);
  });
}
