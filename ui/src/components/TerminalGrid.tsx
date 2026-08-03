// The fleet, as five terminals: the orchestrator fixed on the left, the four
// workers behind a tab strip on the right.
//
// **All five panes are mounted at once.** Only one worker is visible; the other
// three sit behind `.is-hidden`. This is not an optimization to be reversed —
// unmounting an xterm destroys its buffer and there is no replay on the backend,
// so a worker switched away from and back would return blank (L7). Hidden panes
// cost almost nothing: `display: none` skips layout and paint entirely, and
// per-pane event channels mean a hidden pane's listener is the only one woken by
// its own bytes.

import { Panel, PanelGroup, PanelResizeHandle } from "react-resizable-panels";
import { TerminalPane } from "./TerminalPane";
import { statusTone, STATUS_LABEL } from "../lib/statusTone";
import {
  ORCH,
  WORKER_SLOTS,
  workerPane,
  type FleetConfig,
  type PaneId,
  type PaneStatus,
} from "../fleet/types";

/// The orchestrator is the operator's own session and the one they read back
/// through; the workers are watched, not scrolled.
const ORCH_SCROLLBACK = 10000;
const WORKER_SCROLLBACK = 2000;

interface TerminalGridProps {
  started: boolean;
  selected: number;
  onSelect: (slot: number) => void;
  statuses: Record<PaneId, PaneStatus>;
  /// Model info for the orchestrator/worker pane heads — this is what the
  /// dashboard band used to show in its own grid; it now lives only in the
  /// pane it describes.
  config: FleetConfig | null;
  /// xterm fontSize in px, from the app-wide zoom factor — forwarded
  /// unchanged to every pane so all five terminals zoom in lockstep.
  fontSize: number;
  onStatus: (pane: PaneId, status: PaneStatus) => void;
  onRestart: (pane: PaneId) => void;
  /// Worker slots that have produced output since the operator last viewed
  /// that tab — App.tsx derives this from onStatus (see the comment there)
  /// without touching TerminalPane's mount effect. Presence only, no count.
  unreadWorkers: Set<number>;
  /// Registers each pane's `focus()` callback with App.tsx, for the
  /// Cmd+1..5 pane-jump shortcut.
  onRegisterFocus: (pane: PaneId, focus: () => void) => void;
}

export function TerminalGrid({
  started,
  selected,
  onSelect,
  statuses,
  config,
  fontSize,
  onStatus,
  onRestart,
  unreadWorkers,
  onRegisterFocus,
}: TerminalGridProps) {
  const leadModel = config?.lead_model ?? "opus (operator)";
  const workerModel = config?.worker_backend ?? "…";
  const orchStatus = statuses[ORCH] ?? "idle";

  return (
    <PanelGroup direction="horizontal" autoSaveId="fleetor-terminal-grid" className="split">
      <Panel defaultSize={52} minSize={30} className="pane-slot">
        <TerminalPane
          pane={ORCH}
          label="orchestrator · claude"
          scrollback={ORCH_SCROLLBACK}
          started={started}
          status={orchStatus}
          model={leadModel}
          fontSize={fontSize}
          onStatus={onStatus}
          onRestart={() => onRestart(ORCH)}
          onFocusReady={(focus) => onRegisterFocus(ORCH, focus)}
        />
      </Panel>
      <PanelResizeHandle className="divider">
        <span className="divider__grip" />
      </PanelResizeHandle>
      <Panel defaultSize={48} minSize={30} className="pane-slot">
        <div className="worker-stack">
          <div className="tabstrip" role="tablist" aria-label="Worker panes">
            {WORKER_SLOTS.map((slot) => {
              const pane = workerPane(slot);
              const status = statuses[pane] ?? "idle";
              const isSelected = selected === slot;
              return (
                <button
                  key={slot}
                  role="tab"
                  aria-selected={isSelected}
                  className={`tab ${isSelected ? "tab--active" : ""}`}
                  onClick={() => onSelect(slot)}
                >
                  <span className={`dot dot--${statusTone(status)}`} />
                  <span className="mono">worker-{slot}</span>
                  <span className="tab__status">{STATUS_LABEL[status]}</span>
                  {!isSelected && unreadWorkers.has(slot) && (
                    <span className="tab__unread" title="new output" aria-label="new output" />
                  )}
                </button>
              );
            })}
          </div>
          {WORKER_SLOTS.map((slot) => {
            const pane = workerPane(slot);
            return (
              <div
                key={slot}
                className={`worker-slot ${selected === slot ? "" : "is-hidden"}`}
                role="tabpanel"
              >
                <TerminalPane
                  pane={pane}
                  label={`worker-${slot} · claude`}
                  scrollback={WORKER_SCROLLBACK}
                  started={started}
                  status={statuses[pane] ?? "idle"}
                  model={workerModel}
                  fontSize={fontSize}
                  onStatus={onStatus}
                  onRestart={() => onRestart(pane)}
                  onFocusReady={(focus) => onRegisterFocus(pane, focus)}
                />
              </div>
            );
          })}
        </div>
      </Panel>
    </PanelGroup>
  );
}
