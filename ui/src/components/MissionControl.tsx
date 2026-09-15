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
import { gaugeView } from "../lib/contextGaugeTone";
import { statusTone, STATUS_LABEL } from "../lib/statusTone";
import type { ContextGaugeMap } from "../fleet/useContextGauge";

const ALL_PANES: PaneId[] = [ORCH, ...WORKER_SLOTS.map(workerPane)];

function contextTone(pct: number): "green" | "gold" | "coral" {
  if (pct >= 80) return "coral";
  if (pct >= 50) return "gold";
  return "green";
}

interface MissionControlProps {
  selected: PaneId;
  onSelect: (pane: PaneId) => void;
  statuses: Record<PaneId, PaneStatus>;
  panes: PaneIdentityMap;
  gauges: ContextGaugeMap;
  messages: MessageEvent[];
  tasks: TaskEvent[];
}

export function MissionControl({
  selected,
  onSelect,
  statuses,
  panes,
  gauges,
  messages,
  tasks,
}: MissionControlProps) {
  return (
    <div className="mission-control">
      <div className="mission-control__grid">
        {ALL_PANES.map((pane) => {
          const status = statuses[pane] ?? "idle";
          const identity = panes[pane];
          const gauge = gaugeView(gauges[pane]);
          const lastMsg = [...messages].reverse().find(
            (m: MessageEvent) => m.from === pane || m.to === pane,
          );
          const activeTasks = tasks.filter(
            (t) => t.from === pane && t.change.change === "posted",
          );
          const reading = gauges[pane];
          const pct = reading?.kind === "sampled" ? reading.gauge.pct : null;

          return (
            <button
              key={pane}
              className={`mc-card ${selected === pane ? "mc-card--selected" : ""}`}
              data-pane={pane}
              onClick={() => onSelect(pane)}
            >
              <div className="mc-card__header">
                <span className={`dot dot--${statusTone(status)}`} />
                <span className="mc-card__name mono">{pane === ORCH ? "orch" : pane}</span>
                <span className="mc-card__status">{STATUS_LABEL[status]}</span>
              </div>
              {identity?.model && (
                <span className="mc-card__model mono">{identity.model}</span>
              )}
              {pct !== null && (
                <div className="mc-card__context">
                  <div
                    className={`mc-card__bar mc-card__bar--${contextTone(pct)}`}
                    style={{ width: `${Math.min(pct, 100)}%` }}
                  />
                  <span className="mc-card__pct">{gauge?.text}</span>
                </div>
              )}
              {activeTasks.length > 0 && (
                <span className="mc-card__task">
                  {activeTasks.length} task{activeTasks.length !== 1 ? "s" : ""}
                </span>
              )}
              {lastMsg && (
                <p className="mc-card__msg">
                  {lastMsg.from === pane ? "→" : "←"} {lastMsg.body.slice(0, 60)}
                  {lastMsg.body.length > 60 ? "…" : ""}
                </p>
              )}
            </button>
          );
        })}
      </div>
    </div>
  );
}
