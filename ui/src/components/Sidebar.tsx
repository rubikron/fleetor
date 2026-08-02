// The navigation spine (handoff §11): workspace views up top, a Configure group
// below. 4e-1 wired Tickets + Event log; the redesign pass adds the live Fleet
// topology view. The rest stays the roomy, future-proof scaffold the handoff
// calls for (disabled until their phases land).

export type View = "board" | "events" | "fleet" | "workers";

interface SidebarProps {
  view: View;
  onSelect: (view: View) => void;
  unread: number;
}

const WORKSPACE: { view: View; label: string; isNew?: boolean }[] = [
  { view: "board", label: "Tickets" },
  { view: "events", label: "Event log" },
  { view: "fleet", label: "Fleet", isNew: true },
  { view: "workers", label: "Workers", isNew: true },
];

const SOON = ["Diffs", "Knowledge", "Profiles", "Gate", "Models", "Settings"];

export function Sidebar({ view, onSelect, unread }: SidebarProps) {
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
            {item.view === "events" && unread > 0 && <span className="badge">{unread}</span>}
            {item.isNew && <span className="nav__new">new</span>}
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
