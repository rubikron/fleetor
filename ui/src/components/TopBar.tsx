import { useState } from "react";
import type { FleetConfig } from "../fleet/types";

interface TopBarProps {
  config: FleetConfig | null;
  error: string | null;
  zoom: number;
}

export function TopBar({ config, error, zoom }: TopBarProps) {
  const [dismissed, setDismissed] = useState(false);
  const zoomPercent = Math.round(zoom * 100);
  const showError = error && !dismissed;

  return (
    <>
      <header className="topbar" data-tauri-drag-region="deep">
        <span className="brand">FLEETOR</span>
        {config && (
          <span className="topbar__repo mono" title={config.target}>
            {config.target}
          </span>
        )}
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
