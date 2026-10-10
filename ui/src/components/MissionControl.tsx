import { useMemo, useState } from "react";
import {
  ORCH,
  WORKER_SLOTS,
  workerPane,
  type PaneId,
  type PaneIdentityMap,
  type PaneStatus,
  type MessageEvent,
  type ChainEvent,
} from "../fleet/types";
import { replayBoard } from "../fleet/board";
import { elapsed, monitorFlags } from "../fleet/flags";
import type { TaskStoreInfo } from "../fleet/useFleet";
import { summary, useNow } from "./TaskBoard";
import { gaugeView } from "../lib/contextGaugeTone";
import { PaneGauge } from "./PaneGauge";
import { HarnessMark } from "./PaneHead";
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
  /// The task store's chain, and which session is reading it.
  chain: ChainEvent[];
  taskStore: TaskStoreInfo | null;
  unreadPanes: Set<PaneId>;
  /// The clock, fixed; the render probe passes one.
  now?: number;
}

export function MissionControl({
  selected,
  onSelect,
  statuses,
  panes,
  gauges,
  messages,
  chain,
  taskStore,
  unreadPanes,
  now: fixedNow,
}: MissionControlProps) {
  const [detailPane, setDetailPane] = useState<PaneId | null>(null);
  const now = useNow(fixedNow);
  const board = useMemo(() => replayBoard(chain), [chain]);
  // A pane's tasks are the ones it owns in this session; an earlier run's
  // owner of the same name is somebody else.
  const ownedBy = (pane: PaneId) =>
    board.filter(
      (r) => r.block.kind === "task" && r.owner?.pane === pane && r.owner.lineage === taskStore?.lineage,
    );

  const handleSelect = (pane: PaneId) => {
    onSelect(pane);
    setDetailPane(pane === detailPane ? null : pane);
  };

  const detailMessages = detailPane
    ? messages.filter((m: MessageEvent) => m.from === detailPane || m.to === detailPane).slice(-5)
    : [];
  const detailTasks = detailPane ? ownedBy(detailPane).slice(-5) : [];

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
          const owned = ownedBy(pane);
          const current = owned.filter((r) => r.status === "in-progress").pop();
          const takenUp = current?.chain.filter((e) => e.entry.entry === "taken-up").pop();
          const latest = current?.chain[current.chain.length - 1];
          const flags = owned.flatMap((r) =>
            monitorFlags(r, board, now, taskStore?.lineage).map((flag) => ({ ...flag, number: r.number })),
          );
          const reading = gauges[pane];
          const pct = reading?.kind === "sampled" ? reading.gauge.pct : null;

          return (
            <button
              key={pane}
              className={`mc-card ${selected === pane ? "mc-card--selected" : ""}`}
              data-pane={pane}
              onClick={() => handleSelect(pane)}
              onMouseMove={(e) => {
                const rect = e.currentTarget.getBoundingClientRect();
                e.currentTarget.style.setProperty("--mx", `${e.clientX - rect.left}px`);
                e.currentTarget.style.setProperty("--my", `${e.clientY - rect.top}px`);
              }}
            >
              <div className="mc-card__header">
                <span className={`dot dot--${statusTone(status)}`} />
                <span className="mc-card__name mono">{pane === ORCH ? "orch" : pane}</span>
                <HarnessMark identity={identity} block="mc-card__mark" />
                {selected !== pane && unreadPanes.has(pane) && (
                  <span className="mc-card__unread" title="new output" aria-label="new output" />
                )}
                <span className="mc-card__status">{STATUS_LABEL[status]}</span>
              </div>
              {identity?.model && (
                <span className="mc-card__model mono">{identity.model}</span>
              )}
              {pct === null && (
                <PaneGauge reading={reading} block="mc-card__gauge" />
              )}
              {pct !== null && (
                <div className="mc-card__context">
                  <div
                    className={`mc-card__bar mc-card__bar--${contextTone(pct)}`}
                    style={{ width: `${Math.min(pct, 100)}%` }}
                  />
                </div>
              )}
              {pct !== null && <span className="mc-card__pct">{gauge?.text}</span>}
              {current && (
                <span className="mc-card__task">
                  <span className="mono">#{current.number}</span> {current.block.outcome}
                  {takenUp && <span className="mc-card__held"> · held {elapsed(now - takenUp.at)}</span>}
                </span>
              )}
              {latest && (
                <span className="mc-card__latest">
                  <span className="mono">{latest.from}</span> {summary(latest)}
                </span>
              )}
              {flags.map((flag) => (
                <span key={`${flag.number}-${flag.kind}`} className="task-flag mc-card__flag">
                  #{flag.number} {flag.text}
                </span>
              ))}
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
      {detailPane && (detailMessages.length > 0 || detailTasks.length > 0) && (
        <div className="mc-detail">
          <div className="mc-detail__header">
            <span className="mc-detail__title mono">{detailPane}</span>
            <button
              type="button"
              className="mc-detail__close"
              onClick={() => setDetailPane(null)}
              aria-label="Close detail"
            >
              ×
            </button>
          </div>
          {detailMessages.length > 0 && (
            <div className="mc-detail__section">
              <span className="mc-detail__label">Messages</span>
              {detailMessages.map((m) => (
                <p key={m.id} className="mc-detail__line">
                  <span className="mono">{m.from}</span> → <span className="mono">{m.to}</span>:{" "}
                  {m.body.slice(0, 120)}{m.body.length > 120 ? "…" : ""}
                </p>
              ))}
            </div>
          )}
          {detailTasks.length > 0 && (
            <div className="mc-detail__section">
              <span className="mc-detail__label">Tasks</span>
              {detailTasks.map((t) => (
                <p key={t.number} className="mc-detail__line">
                  <span className="mono">#{t.number}</span> {t.status} · {t.block.outcome}
                </p>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
