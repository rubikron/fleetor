// The always-visible dashboard band (handoff §11): the orchestrator as a visual
// anchor, one cell per worker slot, and a queue data-figure block. Status is
// never colour alone — every dot is paired with a text label. Metrics we don't
// track yet (model, context %, elapsed) are intentionally omitted rather than
// faked; they arrive with the live orchestrator (4e-2).

import type { View } from "./Sidebar";
import type { WorkerCell } from "../fleet/useFleet";
import type { Ticket, WorkerState } from "../fleet/types";

const WORKER_LABEL: Record<WorkerState, string> = {
  booting: "booting",
  idle: "idle",
  working: "working",
  blocked: "blocked",
  dead: "dead",
};

function Dot({ tone }: { tone: string }) {
  return <span className={`dot dot--${tone}`} />;
}

function workerTone(state: WorkerState): string {
  if (state === "working") return "accent";
  if (state === "blocked") return "accent";
  if (state === "dead") return "red";
  if (state === "booting") return "muted";
  return "idle";
}

function WorkerCellView({ cell, onOpen }: { cell: WorkerCell; onOpen: () => void }) {
  const blocked = cell.state === "blocked";
  return (
    <button className={`cell ${blocked ? "cell--blocked" : ""}`} onClick={onOpen}>
      <div className="cell__head">
        <Dot tone={workerTone(cell.state)} />
        <span className="cell__name">worker-{cell.slot}</span>
        <span className={`cell__state ${blocked ? "text-accent" : ""}`}>{WORKER_LABEL[cell.state]}</span>
      </div>
      <div className="cell__body">
        {cell.ticket ? (
          <>
            <span className="mono">{cell.ticket}</span>
            {cell.activity && (
              <span className={`cell__activity mono ${blocked ? "text-accent" : ""}`}>{cell.activity}</span>
            )}
          </>
        ) : (
          <span className="cell__muted">no ticket</span>
        )}
      </div>
    </button>
  );
}

function OrchestratorCell({ onOpen }: { onOpen: () => void }) {
  return (
    <button className="cell cell--orch" onClick={onOpen}>
      <div className="cell__head">
        <Dot tone="muted" />
        <span className="cell__name">orchestrator</span>
        <span className="role">lead</span>
        <span className="cell__state">standby</span>
      </div>
      <div className="cell__body">
        <span className="cell__muted">live lead seat attaches in 4e-2</span>
      </div>
    </button>
  );
}

function QueueCell({ board }: { board: Ticket[] }) {
  const count = (predicate: (t: Ticket) => boolean) => board.filter(predicate).length;
  const stats: { k: string; v: number; tone?: string }[] = [
    { k: "backlog", v: count((t) => t.state === "backlog") },
    { k: "active", v: count((t) => t.state === "assigned" || t.state === "in-progress"), tone: "gold" },
    { k: "review", v: count((t) => t.state === "in-review"), tone: "gold" },
    { k: "done", v: count((t) => t.state === "done"), tone: "green" },
  ];
  return (
    <div className="cell cell--queue">
      <div className="cell__head">
        <span className="cell__name">queue</span>
      </div>
      <div className="queue__grid">
        {stats.map((s) => (
          <div key={s.k} className="queue__stat">
            <span className="k">{s.k}</span>
            <span className={`v ${s.tone ? `v--${s.tone}` : ""}`}>{s.v}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

interface BandProps {
  workers: WorkerCell[];
  board: Ticket[];
  onNavigate: (view: View) => void;
}

export function DashboardBand({ workers, board, onNavigate }: BandProps) {
  return (
    <section className="band">
      <OrchestratorCell onOpen={() => onNavigate("fleet")} />
      {workers.map((cell) => (
        <WorkerCellView key={cell.slot} cell={cell} onOpen={() => onNavigate("fleet")} />
      ))}
      <QueueCell board={board} />
    </section>
  );
}
