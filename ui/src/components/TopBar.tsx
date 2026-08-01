// The top status bar: repo · branch · gate · session cost, as quiet pill chips
// (handoff §11). Values are static placeholders in 4e-1 — the real repo/gate/cost
// wiring rides in with the live orchestrator (4e-2).

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

export function TopBar() {
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
    </header>
  );
}
