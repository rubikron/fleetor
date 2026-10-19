import { useMemo, useState } from "react";
import {
  ORCH,
  WORKER_SLOTS,
  workerPane,
  type GaugeReading,
  type PaneId,
  type PaneIdentity,
  type PaneIdentityMap,
  type PaneStatus,
  type MessageEvent,
  type ChainEvent,
} from "../fleet/types";
import { replayBoard, type TaskRecord } from "../fleet/board";
import { monitorFlags, namedReviewerVerdict } from "../fleet/flags";
import type { TaskStoreInfo } from "../fleet/useFleet";
import { summary, useNow } from "./TaskBoard";
import { gaugeView } from "../lib/contextGaugeTone";
import { PaneGauge } from "./PaneGauge";
import { HarnessMark } from "./PaneHead";
import { statusTone, STATUS_LABEL } from "../lib/statusTone";
import type { ContextGaugeMap } from "../fleet/useContextGauge";

const ALL_PANES: PaneId[] = [ORCH, ...WORKER_SLOTS.map(workerPane)];

const lastAt = (r: TaskRecord) => r.chain[r.chain.length - 1]?.at ?? 0;

/// The task a worker holds, else the one it finished or dropped last.
function currentTask(owned: TaskRecord[]): TaskRecord | undefined {
  const held = owned.filter((r) => r.status === "in-progress");
  const pool = held.length > 0 ? held : owned.filter((r) => r.status === "done" || r.status === "dropped");
  return pool.reduce<TaskRecord | undefined>((a, r) => (!a || lastAt(r) >= lastAt(a) ? r : a), undefined);
}

function heldFor(ms: number): string {
  const minutes = Math.max(0, Math.floor(ms / 60_000));
  return minutes < 60 ? `${minutes} min` : `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

function verb(event: ChainEvent): string {
  const { entry } = event;
  if (entry.entry === "status") return `marked ${entry.status}`;
  if (entry.entry === "commented") return "commented";
  return summary(event);
}

const clock = (at: number) =>
  new Date(at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false });

/// A pane's tasks are the ones it owns in this session; an earlier run's owner
/// of the same name is somebody else.
const ownedBy = (board: TaskRecord[], pane: PaneId, lineage: string | null | undefined) =>
  board.filter((r) => r.block.kind === "task" && r.owner?.pane === pane && r.owner.lineage === lineage);

/// The latest done task this pane is the named reviewer of, if it was named
/// in this session.
function reviewedTask(board: TaskRecord[], pane: PaneId, lineage: string | null | undefined) {
  const named = (r: TaskRecord) =>
    r.chain.filter((e) => e.entry.entry === "opened" || e.entry.entry === "reviewer-set").pop();
  const task = board
    .filter((r) => r.block.kind === "task" && r.status === "done" && r.block.reviewer === pane && named(r)?.lineage === lineage)
    .reduce<TaskRecord | undefined>((a, r) => (!a || lastAt(r) >= lastAt(a) ? r : a), undefined);
  return task && { number: task.number, verdict: namedReviewerVerdict(task) };
}

interface WorkerCardProps {
  pane: PaneId;
  status: PaneStatus;
  identity: PaneIdentity | undefined;
  reading: GaugeReading | undefined;
  board: TaskRecord[];
  lineage: string | null | undefined;
  now: number;
  selected: boolean;
  unread: boolean;
  onClick: () => void;
  title?: string;
}

/// One pane's card, the same beside the terminals and on Home.
function WorkerCard({
  pane,
  status,
  identity,
  reading,
  board,
  lineage,
  now,
  selected,
  unread,
  onClick,
  title,
}: WorkerCardProps) {
  const owned = ownedBy(board, pane, lineage);
  const current = currentTask(owned);
  const takenUp = current?.chain.filter((e) => e.entry.entry === "taken-up").pop();
  const latest = current?.chain[current.chain.length - 1];
  const flags = owned.flatMap((r) =>
    monitorFlags(r, board, now, lineage).map((flag) => ({ ...flag, number: r.number })),
  );
  const pct = reading?.kind === "sampled" ? reading.gauge.pct : null;
  const review = current || pane === ORCH ? undefined : reviewedTask(board, pane, lineage);
  const idle = !current && !review && pane !== ORCH;

  return (
    <button
      className={`mc-card${pane === ORCH ? " mc-card--orch" : ""}${selected ? " mc-card--selected" : ""}${idle ? " mc-card--idle" : ""}`}
      data-pane={pane}
      onClick={onClick}
      title={title}
    >
      <span className="mc-card__top">
        <span className="mc-card__name mono" title={identity?.model ?? undefined}>
          {pane === ORCH ? "orch" : pane}
        </span>
        <HarnessMark identity={identity} block="mc-card__mark" />
        {unread && <span className="mc-card__unread" title="new output" aria-label="new output" />}
        <span className={`mc-card__status mono mc-card__status--${statusTone(status)}`}>
          {STATUS_LABEL[status]}
        </span>
      </span>
      {pct === null ? (
        <PaneGauge reading={reading} block="mc-card__gauge" className="mono" />
      ) : (
        <span className="mc-card__context" title={gaugeView(reading)?.text}>
          <span className="mc-card__bar" style={{ width: `${Math.min(pct, 100)}%` }} />
        </span>
      )}
      {current && (
        <span className="mc-card__task">
          <span className="mc-card__id mono">#{current.number}</span> {current.block.outcome}
        </span>
      )}
      {review && (
        <span className="mc-card__task">
          {review.verdict ? "reviewed" : "reviewing"} <span className="mc-card__id mono">#{review.number}</span>
          {review.verdict && ` · ${review.verdict.met ? "met" : "not met"}`}
        </span>
      )}
      {idle && <span className="mc-card__task mc-card__task--none">No task claimed</span>}
      {(current || flags.length > 0) && (
        <span className="mc-card__chips">
          {current && (
            <span className={`mc-card__state mono mc-card__state--${current.status}`}>
              {current.status}
              {current.status === "in-progress" && takenUp && ` ${heldFor(now - takenUp.at)}`}
            </span>
          )}
          {flags.map((flag) => (
            <span key={`${flag.number}-${flag.kind}`} className="task-flag mc-card__flag mono">
              {flag.number === current?.number ? "" : `#${flag.number} `}
              {flag.text}
            </span>
          ))}
        </span>
      )}
      {latest && (
        <span className="mc-card__latest">
          <span className="mc-card__what">
            {latest.from !== pane && <span className="mono">{latest.from} </span>}
            {verb(latest)}
          </span>
          <span className="mc-card__at">
            · <span className="mono">{clock(latest.at)}</span>
          </span>
        </span>
      )}
    </button>
  );
}

interface CardsProps {
  statuses: Record<PaneId, PaneStatus>;
  panes: PaneIdentityMap;
  gauges: ContextGaugeMap;
  /// The task store's chain, and which session is reading it.
  chain: ChainEvent[];
  taskStore: TaskStoreInfo | null;
  /// The pane open in Terminals; its new output is not unread.
  selected: PaneId;
  unreadPanes: Set<PaneId>;
  /// The clock, fixed; the render probe passes one.
  now?: number;
}

interface MissionControlProps extends CardsProps {
  onSelect: (pane: PaneId) => void;
  messages: MessageEvent[];
}

/// The column beside the terminals: every pane, selectable.
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

  const handleSelect = (pane: PaneId) => {
    onSelect(pane);
    setDetailPane(pane === detailPane ? null : pane);
  };

  const detailMessages = detailPane
    ? messages.filter((m: MessageEvent) => m.from === detailPane || m.to === detailPane).slice(-5)
    : [];
  const detailTasks = detailPane ? ownedBy(board, detailPane, taskStore?.lineage).slice(-5) : [];

  return (
    <div className="mission-control">
      <div className="mission-control__grid">
        {ALL_PANES.map((pane) => (
          <WorkerCard
            key={pane}
            pane={pane}
            status={statuses[pane] ?? "idle"}
            identity={panes[pane]}
            reading={gauges[pane]}
            board={board}
            lineage={taskStore?.lineage}
            now={now}
            selected={selected === pane}
            unread={selected !== pane && unreadPanes.has(pane)}
            onClick={() => handleSelect(pane)}
          />
        ))}
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

export interface HomeCardsProps extends CardsProps {
  /// Opens that pane in Terminals.
  onOpen: (pane: PaneId) => void;
}

/// Home's grid: orch and the four workers, while a fleet is live (D-120).
export function HomeCards({
  statuses,
  panes,
  gauges,
  chain,
  taskStore,
  selected,
  unreadPanes,
  onOpen,
  now: fixedNow,
}: HomeCardsProps) {
  const now = useNow(fixedNow);
  const board = useMemo(() => replayBoard(chain), [chain]);
  return (
    <div className="home-cards">
      {ALL_PANES.map((pane) => (
        <WorkerCard
          key={pane}
          pane={pane}
          status={statuses[pane] ?? "idle"}
          identity={panes[pane]}
          reading={gauges[pane]}
          board={board}
          lineage={taskStore?.lineage}
          now={now}
          selected={false}
          unread={selected !== pane && unreadPanes.has(pane)}
          onClick={() => onOpen(pane)}
          title={`Open ${pane === ORCH ? "orch" : pane} in Terminals`}
        />
      ))}
    </div>
  );
}
