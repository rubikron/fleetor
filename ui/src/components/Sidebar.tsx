// The navigation spine: the fleet's three real views — the terminals
// themselves, the message record, and the activity log.
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
// and none is worth a dependency for five glyphs. They inherit currentColor,
// so they follow the row's active/hover state for free.
//
// Settings renders right after WORKSPACE, in the same list — not down in
// .sidebar__footer with Collapse, which would strand it behind the flex
// spacer at the bottom of a tall rail. It's appended after the .map() call
// rather than folded into the WORKSPACE array, so it stays the last row by
// construction as more workspace tabs are added, without needing a manual
// reorder each time. An earlier pass removed a *disabled* Settings
// placeholder alongside five other dead roadmap buttons — this is not that:
// it's a real, working destination.

import type { ReactNode } from "react";

// **This union is one half of a pair.** `ui/src/ui/usePersistedNav.ts` carries
// the other — the list of views it will restore on relaunch — and a view added
// here and forgotten there is a view the operator can select and never return
// to, silently, with no error. That had already happened to `history` before
// D-073 found it. `src-tauri/tests/views.rs` reads both lists and fails if they
// stop naming the same things.
export type View =
  | "fleet"
  | "messages"
  | "tasks"
  | "activity"
  | "history"
  | "critic"
  | "evaluator"
  | "settings";

interface SidebarProps {
  view: View;
  onSelect: (view: View) => void;
  messageCount: number;
  collapsed: boolean;
  onToggleCollapse: () => void;
  /// WP-16's mode, straight from `useDevMode`. `null` means the backend has not
  /// answered yet — the dev-only row stays absent until it says `true`, so it
  /// never flashes into the rail and back out on launch.
  devMode: boolean | null;
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
  // a checklist: the task board. Boxes with rules beside them, not ticks —
  // this shell never renders a claimed `done` as a checkmark (see TaskBoard).
  tasks: (
    <svg {...ICON_PROPS}>
      <rect x="1.8" y="2.6" width="5" height="5" rx="1.2" />
      <rect x="1.8" y="9.4" width="5" height="5" rx="1.2" />
      <line x1="9.2" y1="5.1" x2="14.2" y2="5.1" />
      <line x1="9.2" y1="11.9" x2="14.2" y2="11.9" />
    </svg>
  ),
  // a pulse trace: the activity log
  activity: (
    <svg {...ICON_PROPS}>
      <polyline points="1.6,8 4.4,8 6.4,3.6 9.6,12.4 11.6,8 14.4,8" />
    </svg>
  ),
  // a clock wound back: past runs. Deliberately not an archive box — the shape
  // that reads as "storage" also reads as "somewhere things go to be forgotten",
  // and this is the one view that is meant to be returned to.
  history: (
    <svg {...ICON_PROPS}>
      <path d="M2.4 8a5.6 5.6 0 1 0 1.7-4" />
      <polyline points="2.1,1.9 2.1,4.5 4.7,4.5" />
      <polyline points="8,4.9 8,8 10.3,9.4" />
    </svg>
  ),
  // a speech bubble over a rule: the run read back, and reported on.
  // Deliberately unlike the magnifier below — in dev mode the two rows sit
  // together and answer different questions, so they must not read as two
  // spellings of one thing.
  critic: (
    <svg {...ICON_PROPS}>
      <path d="M2.4 3.4h11.2v7.4H8.6L5.4 13.6v-2.8H2.4z" />
      <line x1="5" y1="6" x2="11" y2="6" />
      <line x1="5" y1="8.4" x2="9" y2="8.4" />
    </svg>
  ),
  // a magnifier over a rule: reading the run back. Deliberately not a clipboard
  // or a tick — this pane grades a finished mission, and both of those shapes
  // read as the task board next door.
  evaluator: (
    <svg {...ICON_PROPS}>
      <circle cx="6.8" cy="6.8" r="4.2" />
      <line x1="9.9" y1="9.9" x2="13.8" y2="13.8" />
      <line x1="5" y1="6.2" x2="8.6" y2="6.2" />
      <line x1="5" y1="8.4" x2="7.4" y2="8.4" />
    </svg>
  ),
  // a gear: preferences
  settings: (
    <svg {...ICON_PROPS}>
      <circle cx="8" cy="8" r="3.1" />
      <circle cx="8" cy="8" r="1.1" />
      <line x1="11.1" y1="8" x2="14.4" y2="8" />
      <line x1="10.2" y1="10.2" x2="12.5" y2="12.5" />
      <line x1="8" y1="11.1" x2="8" y2="14.4" />
      <line x1="5.8" y1="10.2" x2="3.5" y2="12.5" />
      <line x1="4.9" y1="8" x2="1.6" y2="8" />
      <line x1="5.8" y1="5.8" x2="3.5" y2="3.5" />
      <line x1="8" y1="4.9" x2="8" y2="1.6" />
      <line x1="10.2" y1="5.8" x2="12.5" y2="3.5" />
    </svg>
  ),
};

const WORKSPACE: { view: View; label: string; hint?: string; devOnly?: true }[] = [
  { view: "fleet", label: "Terminals" },
  { view: "messages", label: "Messages" },
  // Next to Messages on purpose: the board is what the fleet agreed on and the
  // record is what it then said about it, and they get read together.
  { view: "tasks", label: "Tasks" },
  { view: "activity", label: "Activity" },
  // Last of the always-present rows, and after the live views on purpose:
  // everything above is this run, this one is every run before it.
  { view: "history", label: "History" },
  // **The Critic** (WP-20, D-076). An ordinary row, present whether or not dev
  // mode is on, because it is a product feature rather than an instrument of an
  // experiment. It carries a `hint` for the same reason the row below it does:
  // in dev mode the two sit next to each other, and two panes that both read a
  // run have to say in the rail which question each answers.
  {
    view: "critic",
    label: "Critic",
    hint: "Critic — what the fleet did, cited from the run's own archive",
  },
  // **Last, and dev-only, and that ordering is the point** (D-073). A row that
  // appears and disappears with the mode has to sit at the end, or turning dev
  // mode on shifts every row below it and the rail the operator has learned
  // moves under them. It carries no start control: the evaluator wakes when
  // `orch` hands off, and the view says so — see App.tsx.
  {
    view: "evaluator",
    label: "Evaluator",
    hint: "Evaluator — whether the mission was met, against this mission's own key",
    devOnly: true,
  },
];

export function Sidebar({
  view,
  onSelect,
  messageCount,
  collapsed,
  onToggleCollapse,
  devMode,
}: SidebarProps) {
  // `=== true`, not truthy: `null` is "the backend has not answered yet", and it
  // must read as absent rather than as present-but-off.
  const rows = WORKSPACE.filter((item) => !item.devOnly || devMode === true);
  return (
    <nav className={`sidebar ${collapsed ? "sidebar--collapsed" : ""}`}>
      <div className="sidebar__nav" role="tablist" aria-label="Workspace views">
        {rows.map((item) => {
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
              // Expanded, a row that has to be told apart from its neighbour
              // says which question it answers instead.
              title={collapsed ? item.label : item.hint}
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

        {/* Appended after the map, not folded into WORKSPACE — see the header
            comment. Always the last row, however many workspace tabs precede
            it. */}
        <button
          role="tab"
          aria-selected={view === "settings"}
          className={`nav ${view === "settings" ? "nav--active" : ""}`}
          onClick={() => onSelect("settings")}
          title={collapsed ? "Settings" : undefined}
          aria-label={collapsed ? "Settings" : undefined}
        >
          <span className="nav__icon">{ICONS.settings}</span>
          <span className="nav__label">Settings</span>
        </button>
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
