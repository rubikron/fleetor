// The navigation spine. Four views now that the fleet is five terminals rather
// than a board: the terminals themselves, the message record, the topology, and
// the activity log that carries the backend's notices. The rest stays the roomy
// scaffold the handoff calls for, disabled until their phases land.

export type View = "fleet" | "messages" | "topology" | "activity";

interface SidebarProps {
  view: View;
  onSelect: (view: View) => void;
  messageCount: number;
}

const WORKSPACE: { view: View; label: string }[] = [
  { view: "fleet", label: "Terminals" },
  { view: "messages", label: "Messages" },
  { view: "topology", label: "Topology" },
  { view: "activity", label: "Activity" },
];

const SOON = ["Diffs", "Knowledge", "Profiles", "Gate", "Models", "Settings"];

export function Sidebar({ view, onSelect, messageCount }: SidebarProps) {
  return (
    <nav className="sidebar">
      <div className="sidebar__group" role="tablist" aria-label="Workspace views">
        {WORKSPACE.map((item) => (
          <button
            key={item.view}
            role="tab"
            aria-selected={view === item.view}
            className={`nav ${view === item.view ? "nav--active" : ""}`}
            onClick={() => onSelect(item.view)}
          >
            <span>{item.label}</span>
            {item.view === "messages" && messageCount > 0 && (
              <span className="badge">{messageCount}</span>
            )}
          </button>
        ))}
      </div>
      <div className="sidebar__label label">Configure</div>
      <div className="sidebar__group">
        {SOON.map((label) => (
          <button key={label} className="nav nav--soon" disabled>
            {label}
          </button>
        ))}
      </div>
    </nav>
  );
}
