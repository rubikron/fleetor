import { TerminalPane } from "./TerminalPane";
import { MissionControl } from "./MissionControl";
import {
  ORCH,
  WORKER_SLOTS,
  workerPane,
  type PaneId,
  type PaneIdentityMap,
  type PaneStatus,
  type MessageEvent,
  type TaskEvent,
} from "../fleet/types";
import type { ContextGaugeMap } from "../fleet/useContextGauge";
import type { Theme } from "../ui/useTheme";

const ORCH_SCROLLBACK = 10000;
const WORKER_SCROLLBACK = 2000;

const ALL_PANES: { pane: PaneId; label: string; scrollback: number }[] = [
  { pane: ORCH, label: "orchestrator", scrollback: ORCH_SCROLLBACK },
  ...WORKER_SLOTS.map((slot) => ({
    pane: workerPane(slot),
    label: workerPane(slot),
    scrollback: WORKER_SCROLLBACK,
  })),
];

interface TerminalGridProps {
  started: boolean;
  selected: PaneId;
  onSelect: (pane: PaneId) => void;
  statuses: Record<PaneId, PaneStatus>;
  panes: PaneIdentityMap;
  fontSize: number;
  theme: Theme;
  onStatus: (pane: PaneId, status: PaneStatus) => void;
  onRestart: (pane: PaneId) => void;
  unreadPanes: Set<PaneId>;
  onRegisterFocus: (pane: PaneId, focus: () => void) => void;
  gauges: ContextGaugeMap;
  messages: MessageEvent[];
  tasks: TaskEvent[];
}

export function TerminalGrid({
  started,
  selected,
  onSelect,
  statuses,
  panes,
  fontSize,
  theme,
  onStatus,
  onRestart,
  unreadPanes,
  onRegisterFocus,
  gauges,
  messages,
  tasks,
}: TerminalGridProps) {
  return (
    <div className="fleet-layout">
      <div className="fleet-layout__terminal">
        {ALL_PANES.map(({ pane, label, scrollback }) => (
          <div
            key={pane}
            className={`pane-slot ${selected === pane ? "" : "is-hidden"}`}
          >
            <TerminalPane
              pane={pane}
              label={label}
              scrollback={scrollback}
              started={started}
              status={statuses[pane] ?? "idle"}
              identity={panes[pane]}
              fontSize={fontSize}
              theme={theme}
              onStatus={onStatus}
              onRestart={() => onRestart(pane)}
              onFocusReady={(focus) => onRegisterFocus(pane, focus)}
              gauge={gauges[pane]}
            />
          </div>
        ))}
      </div>
      <div className="fleet-layout__control">
        <MissionControl
          selected={selected}
          onSelect={onSelect}
          statuses={statuses}
          panes={panes}
          gauges={gauges}
          messages={messages}
          tasks={tasks}
          unreadPanes={unreadPanes}
        />
      </div>
    </div>
  );
}
