import { TerminalPane } from "./TerminalPane";
import { MissionControl } from "./MissionControl";
import { statusTone, STATUS_LABEL } from "../lib/statusTone";
import { PaneGauge } from "./PaneGauge";
import { HarnessMark } from "./PaneHead";
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
import { OUT_OF_SCOPE, type ContextGaugeMap } from "../fleet/useContextGauge";
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
        <div className="tabstrip" role="tablist" aria-label="Fleet panes">
          {ALL_PANES.map(({ pane }) => {
            const status = statuses[pane] ?? "idle";
            const isSelected = selected === pane;
            const gauge = pane === ORCH ? OUT_OF_SCOPE : gauges[pane];
            return (
              <button
                key={pane}
                role="tab"
                aria-selected={isSelected}
                className={`tab ${isSelected ? "tab--active" : ""}`}
                onClick={() => onSelect(pane)}
              >
                <span className={`dot dot--${statusTone(status)}`} />
                <span className="mono">{pane === ORCH ? "orch" : pane}</span>
                <HarnessMark identity={panes[pane]} block="tab__mark" />
                <span className="tab__status">{STATUS_LABEL[status]}</span>
                <PaneGauge reading={gauge} block="tab__gauge" />
                {!isSelected && unreadPanes.has(pane) && (
                  <span className="tab__unread" title="new output" aria-label="new output" />
                )}
              </button>
            );
          })}
        </div>
        {ALL_PANES.map(({ pane, label, scrollback }) => (
          <div
            key={pane}
            className={`pane-slot ${selected === pane ? "" : "is-hidden"}`}
            role="tabpanel"
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
              gauge={pane === ORCH ? OUT_OF_SCOPE : gauges[pane]}
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
        />
      </div>
    </div>
  );
}
