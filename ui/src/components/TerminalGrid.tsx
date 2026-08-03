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
import { ORCH, WORKER_SLOTS, workerPane, type PaneId, type PaneStatus } from "../fleet/types";

/// The orchestrator is the operator's own session and the one they read back
/// through; the workers are watched, not scrolled.
const ORCH_SCROLLBACK = 10000;
const WORKER_SCROLLBACK = 2000;

interface TerminalGridProps {
  started: boolean;
  selected: number;
  onSelect: (slot: number) => void;
  statuses: Record<PaneId, PaneStatus>;
  onStatus: (pane: PaneId, status: PaneStatus) => void;
  onRestart: (pane: PaneId) => void;
}

function statusDot(status: PaneStatus): string {
  if (status === "live") return "accent";
  if (status === "dead") return "red";
  return "muted";
}

export function TerminalGrid({
  started,
  selected,
  onSelect,
  statuses,
  onStatus,
  onRestart,
}: TerminalGridProps) {
  return (
    <PanelGroup direction="horizontal" autoSaveId="fleetor-terminal-grid" className="split">
      <Panel defaultSize={52} minSize={30} className="pane-slot">
        <TerminalPane
          pane={ORCH}
          label="orchestrator · claude"
          scrollback={ORCH_SCROLLBACK}
          started={started}
          onStatus={onStatus}
          onRestart={() => onRestart(ORCH)}
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
              return (
                <button
                  key={slot}
                  role="tab"
                  aria-selected={selected === slot}
                  className={`tab ${selected === slot ? "tab--active" : ""}`}
                  onClick={() => onSelect(slot)}
                >
                  <span className={`dot dot--${statusDot(status)}`} />
                  <span className="mono">worker-{slot}</span>
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
                  onStatus={onStatus}
                  onRestart={() => onRestart(pane)}
                />
              </div>
            );
          })}
        </div>
      </Panel>
    </PanelGroup>
  );
}
