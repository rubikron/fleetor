// The navigation spine: the fleet's four real views — the terminals
// themselves, the message record, the topology, and the activity log.
//
// UI-polish pass: dropped the six disabled "Diffs / Knowledge / Profiles /
// Gate / Models / Settings" placeholders. A roadmap spelled out in
// permanently-disabled buttons doesn't earn a whole panel's worth of chrome;
// it just pads the sidebar out and makes four live destinations look like a
// third of what's on offer.
//
// UI-polish pass 3 — the rail is *secondary chrome and must read as such*.
// The pass before this styled it like primary content: body-sized type, weight
// 600, a "›" marker, and a solid gold badge all firing at once for a single
// active row. Every one of those is gone. What replaces it is what the tools
// this is measured against (Claude desktop, ChatGPT, Linear) actually do:
//
//  - small type (--t-small), one uniform row height, tight gaps
//  - an ICON per view, so the collapsed rail is a real icon rail rather than
//    two-letter codes ("TE" / "MS") nobody can read at a glance
//  - active = a quiet surface step + brighter foreground. No weight change, no
//    marker glyph, no accent bar (coral stays reserved for .line--rail)
//  - the unread count as plain gold digits, not a filled pill; collapsed, it
//    degrades to a dot on the icon instead of clipping out of the rail
//
// Icons are hand-written inline SVG on purpose: no icon library is installed
// and none is worth a dependency for four glyphs. They inherit currentColor,
// so they follow the row's active/hover state for free.

import type { ReactNode } from "react";

export type View = "fleet" | "messages" | "topology" | "activity";

interface SidebarProps {
  view: View;
  onSelect: (view: View) => void;
  messageCount: number;
  collapsed: boolean;
  onToggleCollapse: () => void;
}

/// 16×16, stroke-only, `currentColor`. Distinct silhouettes matter more than
/// detail — at this size the shape is the whole signal.
const ICON_PROPS = {
  viewBox: "0 0 16 16",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.4,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
  "aria-hidden": true,
};

const ICONS: Record<View, ReactNode> = {
  // a terminal: window frame + prompt caret + input line
  fleet: (
    <svg {...ICON_PROPS}>
      <rect x="1.6" y="2.6" width="12.8" height="10.8" rx="1.6" />
      <polyline points="4.6,6.6 6.5,8 4.6,9.4" />
      <line x1="8.4" y1="10" x2="11.4" y2="10" />
    </svg>
  ),
  // an envelope: the message record
  messages: (
    <svg {...ICON_PROPS}>
      <rect x="1.6" y="3.4" width="12.8" height="9.2" rx="1.6" />
      <polyline points="2.2,4.6 8,8.8 13.8,4.6" />
    </svg>
  ),
  // the fleet graph: one lead node fanning out to three
  topology: (
    <svg {...ICON_PROPS}>
      <circle cx="3.4" cy="8" r="1.9" />
      <circle cx="12.6" cy="3.6" r="1.5" />
      <circle cx="12.6" cy="8" r="1.5" />
      <circle cx="12.6" cy="12.4" r="1.5" />
      <line x1="5.2" y1="7.2" x2="11.1" y2="4.1" />
      <line x1="5.3" y1="8" x2="11.1" y2="8" />
      <line x1="5.2" y1="8.8" x2="11.1" y2="11.9" />
    </svg>
  ),
  // a pulse trace: the activity log
  activity: (
    <svg {...ICON_PROPS}>
      <polyline points="1.6,8 4.4,8 6.4,3.6 9.6,12.4 11.6,8 14.4,8" />
    </svg>
  ),
};

const WORKSPACE: { view: View; label: string }[] = [
  { view: "fleet", label: "Terminals" },
  { view: "messages", label: "Messages" },
  { view: "topology", label: "Topology" },
  { view: "activity", label: "Activity" },
];

export function Sidebar({
  view,
  onSelect,
  messageCount,
  collapsed,
  onToggleCollapse,
}: SidebarProps) {
  return (
    <nav className={`sidebar ${collapsed ? "sidebar--collapsed" : ""}`}>
      <div className="sidebar__nav" role="tablist" aria-label="Workspace views">
        {WORKSPACE.map((item) => {
          const unread = item.view === "messages" && messageCount > 0;
          return (
            <button
              key={item.view}
              role="tab"
              aria-selected={view === item.view}
              className={`nav ${view === item.view ? "nav--active" : ""}`}
              onClick={() => onSelect(item.view)}
              // Collapsed, the label is the only thing naming the icon, so it
              // has to survive as a tooltip and as the accessible name.
              title={collapsed ? item.label : undefined}
              aria-label={collapsed ? item.label : undefined}
            >
              <span className="nav__icon">{ICONS[item.view]}</span>
              <span className="nav__label">{item.label}</span>
              {unread && (
                <>
                  <span className="nav__count">{messageCount}</span>
                  {/* collapsed fallback: a dot, because the digits clip out
                      of an icon-width rail */}
                  <span className="nav__dot" aria-hidden="true" />
                </>
              )}
            </button>
          );
        })}
      </div>

      <div className="sidebar__footer">
        <button
          type="button"
          className="sidebar__toggle"
          onClick={onToggleCollapse}
          aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
          title={`${collapsed ? "Expand" : "Collapse"} sidebar (⌘B)`}
        >
          <span className="nav__icon">
            <svg {...ICON_PROPS}>
              {collapsed ? (
                <polyline points="6.4,3.8 10.4,8 6.4,12.2" />
              ) : (
                <polyline points="9.6,3.8 5.6,8 9.6,12.2" />
              )}
            </svg>
          </span>
          <span className="nav__label">Collapse</span>
        </button>
      </div>
    </nav>
  );
}
