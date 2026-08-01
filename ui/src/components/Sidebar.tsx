// The navigation spine (handoff §11): workspace views up top, a CONFIGURE group
// below. 4e-1 wires the two views the observable shell actually has — Board and
// Event log — and shows the rest as the roomy, future-proof scaffold the handoff
// calls for (disabled until their phases land).

export type View = "board" | "events";

interface SidebarProps {
  view: View;
  onSelect: (view: View) => void;
  unread: number;
}

const WORKSPACE: { view: View; label: string }[] = [
  { view: "board", label: "Tickets" },
  { view: "events", label: "Event log" },
];

const SOON = ["Diffs", "Knowledge", "Profiles", "Gate", "Models", "Settings"];

export function Sidebar({ view, onSelect, unread }: SidebarProps) {
  return (
    <nav className="sidebar">
      <div className="sidebar__group">
        {WORKSPACE.map((item) => (
          <button
            key={item.view}
            className={`nav ${view === item.view ? "nav--active" : ""}`}
            onClick={() => onSelect(item.view)}
          >
            <span>{item.label}</span>
            {item.view === "events" && unread > 0 && <span className="badge">{unread}</span>}
          </button>
        ))}
      </div>
      <div className="sidebar__label">Configure</div>
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
