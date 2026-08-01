// The top status bar (Phase 4e-2): quiet pill chips showing the *real* fleet
// posture — the target repo, its branch, the worker backend, and the quality
// gate — plus the live connection status. Values come from the backend's
// fleet_config (the scratch repo and resolved worker backend), not placeholders.
// The former dev toolbar (demo / manual assign) is gone: real dispatch happens
// when the lead calls `assign` in the orchestrator TUI, so those buttons would
// only mislead.

import type { FleetConfig } from "../fleet/types";

interface Pill {
  label: string;
  value: string;
  tone: "neutral" | "gold" | "green";
}

function pills(config: FleetConfig | null): Pill[] {
  if (!config) {
    return [
      { label: "target", value: "…", tone: "neutral" },
      { label: "workers", value: "…", tone: "neutral" },
    ];
  }
  return [
    { label: "target", value: config.target, tone: "neutral" },
    { label: "branch", value: config.branch, tone: "neutral" },
    { label: "workers", value: config.worker_backend, tone: config.worker_backend === "flash" ? "gold" : "neutral" },
    { label: "gate", value: config.gate, tone: "green" },
  ];
}

interface TopBarProps {
  status: string;
  config: FleetConfig | null;
}

export function TopBar({ status, config }: TopBarProps) {
  return (
    <header className="topbar">
      <span className="brand">FLEETOR</span>
      <div className="pills">
        {pills(config).map((p) => (
          <span key={p.label} className={`pill pill--${p.tone}`}>
            <span className="pill__label">{p.label}</span>
            <span className="pill__value">{p.value}</span>
          </span>
        ))}
      </div>
      <div className="topbar__spacer" />
      <span className="devbar__status">{status}</span>
    </header>
  );
}
