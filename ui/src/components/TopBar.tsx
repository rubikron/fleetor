import { useState } from "react";
import type { FleetConfig } from "../fleet/types";

interface TopBarProps {
  config: FleetConfig | null;
  /// When the running fleet's run started; absent with no fleet.
  runStartedMs?: number;
  error: string | null;
  zoom: number;
}

const hhmm = (ms: number) => {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
};

export function TopBar({ config, runStartedMs, error, zoom }: TopBarProps) {
  const [dismissed, setDismissed] = useState(false);
  const zoomPercent = Math.round(zoom * 100);
  const showError = error && !dismissed;
  const title = [
    "fleetor",
    config?.target,
    runStartedMs !== undefined ? `run ${hhmm(runStartedMs)}` : undefined,
  ]
    .filter(Boolean)
    .join(" · ");

  return (
    <>
      <header className="topbar" data-tauri-drag-region="deep">
        <span className="topbar__title" title={config?.target_path}>
          {title}
        </span>
        <div className="topbar__spacer" />
        {zoomPercent !== 100 && <span className="topbar__zoom mono">{zoomPercent}%</span>}
      </header>
      {showError && (
        <div className="topbar-banner topbar-banner--error">
          <span>{error}</span>
          <button
            type="button"
            className="topbar-banner__dismiss"
            onClick={() => setDismissed(true)}
            aria-label="Dismiss"
          >
            ×
          </button>
        </div>
      )}
    </>
  );
}
