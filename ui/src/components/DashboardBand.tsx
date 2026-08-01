// The always-visible dashboard band (handoff §11): one cell for the orchestrator,
// one per worker slot, one for the queue. Status is never colour alone — every
// dot is paired with a text label.

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
  if (state === "blocked") return "gold";
  if (state === "dead") return "red";
  if (state === "booting") return "muted";
  return "idle";
}

function WorkerCellView({ cell }: { cell: WorkerCell }) {
  return (
    <div className="cell">
      <div className="cell__head">
        <Dot tone={workerTone(cell.state)} />
        <span className="cell__name">worker-{cell.slot}</span>
        <span className="cell__state">{WORKER_LABEL[cell.state]}</span>
      </div>
      <div className="cell__body">
        {cell.ticket ? (
          <>
            <span className="mono">{cell.ticket}</span>
            {cell.activity && <span className="cell__activity mono">{cell.activity}</span>}
          </>
        ) : (
          <span className="cell__muted">no ticket</span>
        )}
      </div>
    </div>
  );
}

function OrchestratorCell() {
  return (
    <div className="cell cell--orch">
      <div className="cell__head">
        <Dot tone="muted" />
        <span className="cell__name">orchestrator</span>
        <span className="cell__state">standby</span>
      </div>
      <div className="cell__body">
        <span className="cell__muted">lead seat attaches in 4e-2</span>
      </div>
    </div>
  );
}

function QueueCell({ board }: { board: Ticket[] }) {
  const count = (predicate: (t: Ticket) => boolean) => board.filter(predicate).length;
  const backlog = count((t) => t.state === "backlog");
  const active = count((t) => t.state === "assigned" || t.state === "in-progress");
  const review = count((t) => t.state === "in-review");
  const done = count((t) => t.state === "done");
  return (
    <div className="cell cell--queue">
      <div className="cell__head">
        <span className="cell__name">queue</span>
      </div>
      <div className="cell__body queue__counts">
        <span>backlog <b className="mono">{backlog}</b></span>
        <span>active <b className="mono">{active}</b></span>
        <span>review <b className="mono">{review}</b></span>
        <span>done <b className="mono">{done}</b></span>
      </div>
    </div>
  );
}

interface BandProps {
  workers: WorkerCell[];
  board: Ticket[];
}

export function DashboardBand({ workers, board }: BandProps) {
  return (
    <section className="band">
      <OrchestratorCell />
      {workers.map((cell) => (
        <WorkerCellView key={cell.slot} cell={cell} />
      ))}
      <QueueCell board={board} />
    </section>
  );
}
