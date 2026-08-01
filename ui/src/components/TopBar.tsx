// The top status bar: repo · branch · gate · session cost as quiet pill chips,
// plus the demoted dev toolbar (Run demo / Assign) on the right — dev controls,
// deliberately kept out of the monitoring content (handoff §11 / redesign notes).
// Repo/gate/cost values are static placeholders in 4e-1; real wiring rides in
// with the live orchestrator (4e-2).

interface Pill {
  label: string;
  value: string;
  tone?: "neutral" | "gold" | "green";
}

const PILLS: Pill[] = [
  { label: "repo", value: "fleetor" },
  { label: "branch", value: "phase-4e-shell" },
  { label: "gate", value: "cargo test · vite build", tone: "green" },
  { label: "session", value: "$0.00", tone: "gold" },
];

interface TopBarProps {
  ready: boolean;
  status: string;
  onDemo: () => void;
  onAssign: () => void;
}

export function TopBar({ ready, status, onDemo, onAssign }: TopBarProps) {
  return (
    <header className="topbar">
      <span className="brand">FLEETOR</span>
      <div className="pills">
        {PILLS.map((p) => (
          <span key={p.label} className={`pill pill--${p.tone ?? "neutral"}`}>
            <span className="pill__label">{p.label}</span>
            <span className="pill__value">{p.value}</span>
          </span>
        ))}
      </div>
      <div className="topbar__spacer" />
      <span className="devbar__status">{status}</span>
      <div className="devbar">
        <span className="devbar__label">dev</span>
        <button className="devbtn" onClick={onDemo} disabled={!ready}>
          Run demo lifecycle
        </button>
        <button className="devbtn" onClick={onAssign} disabled={!ready}>
          Assign a ticket
        </button>
      </div>
    </header>
  );
}
