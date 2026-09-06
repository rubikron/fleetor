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
import { PaneGauge } from "./PaneGauge";
import { HarnessMark } from "./PaneHead";
import {
  ORCH,
  WORKER_SLOTS,
  workerPane,
  type PaneId,
  type PaneIdentityMap,
  type PaneStatus,
} from "../fleet/types";
import { OUT_OF_SCOPE, type ContextGaugeMap } from "../fleet/useContextGauge";
import type { Theme } from "../ui/useTheme";

/// The orchestrator is the operator's own session and the one they read back
/// through; the workers are watched, not scrolled.
const ORCH_SCROLLBACK = 10000;
const WORKER_SCROLLBACK = 2000;

interface TerminalGridProps {
  started: boolean;
  selected: number;
  onSelect: (slot: number) => void;
  statuses: Record<PaneId, PaneStatus>;
  /// **What each pane was actually placed as** (#50) — its harness, that
  /// harness's mark and its model, folded out of the spawn events by
  /// `useFleet`. This replaced the fleet-wide launch config the heads used to
  /// print a model from: one `worker_backend` cannot describe a mixed fleet,
  /// and beside it the vendor was a literal that was simply wrong for half the
  /// seats. A pane with no entry has not spawned and its head says so by
  /// showing no harness at all.
  panes: PaneIdentityMap;
  /// xterm fontSize in px, from the app-wide zoom factor — forwarded
  /// unchanged to every pane so all five terminals zoom in lockstep.
  fontSize: number;
  /// The app-wide light/dark preference — forwarded unchanged so all five
  /// terminals theme in lockstep with the chrome.
  theme: Theme;
  onStatus: (pane: PaneId, status: PaneStatus) => void;
  onRestart: (pane: PaneId) => void;
  /// Worker slots that have produced output since the operator last viewed
  /// that tab — App.tsx derives this from onStatus (see the comment there)
  /// without touching TerminalPane's mount effect. Presence only, no count.
  unreadWorkers: Set<number>;
  /// Registers each pane's `focus()` callback with App.tsx, for the
  /// Cmd+1..5 pane-jump shortcut.
  onRegisterFocus: (pane: PaneId, focus: () => void) => void;
  /// What each pane's rail says about its context (WP-04, #48). Threaded
  /// down to both the pane head and the tab strip, which render it through
  /// the one `PaneGauge` so they never drift apart — including on the states
  /// that carry no figure.
  gauges: ContextGaugeMap;
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
  unreadWorkers,
  onRegisterFocus,
  gauges,
}: TerminalGridProps) {
  const orchStatus = statuses[ORCH] ?? "idle";

  return (
    <PanelGroup direction="horizontal" autoSaveId="fleetor-terminal-grid" className="split">
      <Panel defaultSize={52} minSize={30} className="pane-slot">
        <TerminalPane
          pane={ORCH}
          label="orchestrator"
          scrollback={ORCH_SCROLLBACK}
          started={started}
          status={orchStatus}
          identity={panes[ORCH]}
          fontSize={fontSize}
          theme={theme}
          onStatus={onStatus}
          onRestart={() => onRestart(ORCH)}
          onFocusReady={(focus) => onRegisterFocus(ORCH, focus)}
          // Not a lookup: the orchestrator's transcript is the operator's
          // own and has no gauge to be unavailable, so it says so from the
          // first frame rather than reading `pending` until a poll lands.
          gauge={OUT_OF_SCOPE}
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
              const gauge = gauges[pane];
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
                  <HarnessMark identity={panes[pane]} block="tab__mark" />
                  <span className="tab__status">{STATUS_LABEL[status]}</span>
                  <PaneGauge reading={gauge} block="tab__gauge" />
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
                  label={pane}
                  scrollback={WORKER_SCROLLBACK}
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
            );
          })}
        </div>
      </Panel>
    </PanelGroup>
  );
}
