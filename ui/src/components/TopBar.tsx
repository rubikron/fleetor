// The top status bar: a plain terminal-style status line showing the *real*
// fleet posture — the target repo, its branch, the worker backend, and the
// quality gate — plus the live connection status. Values come from the
// backend's fleet_config (the resolved target repo — the operator's own, or
// the seeded testbed), not placeholders.
//
// UI-polish pass: the former pill chips (bordered, pill-radius, uppercase
// labels) read as decoration on content that is really just key/value data.
// Replaced with plain mono key:value pairs separated by hairlines, the way a
// terminal status line reads.
//
// UI-polish pass 2: the window now uses an overlay title bar (see
// src-tauri/tauri.conf.json), so this row *is* the title bar — it carries
// `data-tauri-drag-region` so the window is still draggable, and styles.css
// reserves fixed px space on the left so the brand mark clears the macOS
// traffic lights. None of this row's content is interactive (no buttons),
// so nothing here risks having its clicks swallowed by the drag region.

import type { FleetConfig } from "../fleet/types";

interface StatusItem {
  label: string;
  value: string;
  tone: "neutral" | "gold" | "green";
}

function statusItems(config: FleetConfig | null): StatusItem[] {
  if (!config) {
    return [
      { label: "target", value: "…", tone: "neutral" },
      { label: "workers", value: "…", tone: "neutral" },
    ];
  }
  return [
    { label: "target", value: config.target, tone: "neutral" },
    { label: "branch", value: config.branch, tone: "neutral" },
    { label: "workers", value: config.worker_backend, tone: config.worker_backend === "none" ? "neutral" : "gold" },
    { label: "gate", value: config.gate, tone: "green" },
  ];
}

function valueClass(tone: StatusItem["tone"]): string {
  if (tone === "gold") return "status-line__value status-line__value--gold";
  if (tone === "green") return "status-line__value status-line__value--green";
  return "status-line__value";
}

interface TopBarProps {
  status: string;
  config: FleetConfig | null;
  /// The current app zoom factor (1.0 = 100%). Shown only when it departs
  /// from the default — this is a quiet aside, not a persistent readout.
  zoom: number;
}

export function TopBar({ status, config, zoom }: TopBarProps) {
  const zoomPercent = Math.round(zoom * 100);
  return (
    <header className="topbar" data-tauri-drag-region>
      <span className="brand">FLEETOR</span>
      <div className="status-line">
        {statusItems(config).map((item) => (
          <span key={item.label} className="status-line__item">
            <span className="status-line__label">{item.label}</span>
            <span className={valueClass(item.tone)}>{item.value}</span>
          </span>
        ))}
      </div>
      <div className="topbar__spacer" />
      {zoomPercent !== 100 && <span className="topbar__zoom mono">{zoomPercent}%</span>}
      <span className="topbar__status mono">{status}</span>
    </header>
  );
}
