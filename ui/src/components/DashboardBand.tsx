// The always-visible band: one cell per pane, orchestrator first.
//
// Status is never colour alone — every dot is paired with a text label. What a
// cell shows is only what the shell actually knows: a pane is `live` once its
// pty has produced bytes, `dead` once it has exited, and `idle` before the fleet
// is started. There is no "working"/"blocked" any more, because a live `claude`
// TUI does not tell us which it is and inventing the distinction was how the
// headless fleet ended up with a mail queue and a turn boundary.

import { ORCH, ROSTER, type FleetConfig, type PaneId, type PaneStatus } from "../fleet/types";

const STATUS_LABEL: Record<PaneStatus, string> = {
  idle: "standby",
  live: "live",
  dead: "exited",
};

function tone(status: PaneStatus): string {
  if (status === "live") return "accent";
  if (status === "dead") return "red";
  return "muted";
}

interface PaneCellProps {
  pane: PaneId;
  status: PaneStatus;
  detail: string;
  onOpen: () => void;
}

function PaneCell({ pane, status, detail, onOpen }: PaneCellProps) {
  const isOrch = pane === ORCH;
  return (
    <button className={`cell ${isOrch ? "cell--orch" : ""}`} onClick={onOpen}>
      <div className="cell__head">
        <span className={`dot dot--${tone(status)}`} />
        <span className="cell__name">{isOrch ? "orchestrator" : pane}</span>
        {isOrch && <span className="role">lead</span>}
        <span className={`cell__state ${status === "live" ? "text-accent" : ""}`}>
          {STATUS_LABEL[status]}
        </span>
      </div>
      <div className="cell__body">
        <span className={status === "idle" ? "cell__muted" : "mono"}>{detail}</span>
      </div>
    </button>
  );
}

interface BandProps {
  statuses: Record<PaneId, PaneStatus>;
  config: FleetConfig | null;
  onOpen: (pane: PaneId) => void;
}

export function DashboardBand({ statuses, config, onOpen }: BandProps) {
  const lead = config?.lead_model ?? "opus (operator)";
  const worker = config?.worker_backend ?? "…";
  return (
    <section className="band">
      {ROSTER.map((pane) => {
        const status = statuses[pane] ?? "idle";
        const model = pane === ORCH ? lead : worker;
        return (
          <PaneCell
            key={pane}
            pane={pane}
            status={status}
            detail={status === "idle" ? "start the fleet to attach" : model}
            onOpen={() => onOpen(pane)}
          />
        );
      })}
    </section>
  );
}
