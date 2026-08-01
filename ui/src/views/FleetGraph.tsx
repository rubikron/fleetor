// The Fleet topology — a bespoke, directional live-flow view (not a recolored
// node-graph library). The lead sits at the centre; the four workers around it in
// an asymmetric, signal-driven layout (blocked worker largest, idle smallest).
// Each non-idle edge carries a single message dot travelling in its real
// direction — assign/work flows lead→worker, an `ask_lead` flows worker→lead.
//
// It reads the same live data as the rest of the shell: worker state from the
// event stream, plus a glance at the newest feed entries to light an edge gold
// when a message just crossed it. No backend calls, no new data — another
// consumer of the 4c bus.

import type { WorkerCell } from "../fleet/useFleet";
import type { FleetEvent } from "../fleet/types";

type EdgeKind = "active" | "mail" | "blocked" | "idle";

// Fixed, deliberately asymmetric geometry per slot. `d` always runs lead→worker;
// blocked simply reverses the dot and puts the arrowhead at the lead end.
interface SlotLayout {
  cx: number;
  cy: number;
  d: string;
  labelX: number;
  labelY: number;
}

const LEAD = { cx: 450, cy: 235, r: 54 };

const LAYOUT: Record<number, SlotLayout> = {
  1: { cx: 208, cy: 112, d: "M 448 210 C 372 176, 296 146, 250 132", labelX: 330, labelY: 150 },
  2: { cx: 712, cy: 104, d: "M 452 210 C 545 168, 628 132, 672 118", labelX: 566, labelY: 150 },
  3: { cx: 268, cy: 360, d: "M 442 258 C 408 282, 352 314, 312 340", labelX: 392, labelY: 316 },
  4: { cx: 740, cy: 366, d: "M 452 262 C 560 306, 660 340, 706 356", labelX: 596, labelY: 330 },
};

function nodeRadius(state: WorkerCell["state"]): number {
  if (state === "blocked") return 46;
  if (state === "idle" || state === "dead") return 32;
  return 40;
}

function edgeKind(cell: WorkerCell, feed: FleetEvent[]): EdgeKind {
  if (cell.state === "blocked") return "blocked";
  if (cell.state === "idle" || cell.state === "dead") return "idle";
  // a message just crossed this edge → light it gold briefly (newest entries)
  const recentMail = feed.slice(0, 4).some((e) => e.type === "mail" && e.to === String(cell.slot));
  return recentMail ? "mail" : "active";
}

function WorkerNode({ cell, feed }: { cell: WorkerCell; feed: FleetEvent[] }) {
  const geo = LAYOUT[cell.slot];
  if (!geo) return null;
  const kind = edgeKind(cell, feed);
  const r = nodeRadius(cell.state);
  const idle = kind === "idle";
  const edgeId = `fe-s${cell.slot}`;

  const ringClass =
    cell.state === "blocked"
      ? "node-ring node-ring--blocked"
      : idle
        ? "node-ring node-ring--idle"
        : "node-ring node-ring--active";

  const label =
    kind === "blocked" ? "ask_lead" : kind === "mail" ? "mail" : cell.ticket ?? "";
  const labelClass =
    kind === "blocked" ? "edge-label edge-label--blocked" : kind === "mail" ? "edge-label edge-label--mail" : "edge-label";

  return (
    <g>
      {/* edge */}
      <path
        id={edgeId}
        className={`edge edge--${kind}`}
        d={geo.d}
        markerEnd={kind === "active" || kind === "mail" ? "url(#ah-coral)" : undefined}
        markerStart={kind === "blocked" ? "url(#ah-coral)" : undefined}
      />
      {/* travelling message dot (skipped for idle edges) */}
      {!idle && (
        <circle className={kind === "mail" ? "msg--mail" : kind === "blocked" ? "msg--blocked" : "msg--work"} r={3.4}>
          <animateMotion
            dur={kind === "blocked" ? "1.6s" : kind === "mail" ? "2s" : "2.8s"}
            repeatCount="indefinite"
            keyPoints={kind === "blocked" ? "1;0" : "0;1"}
            keyTimes="0;1"
            calcMode="linear"
          >
            <mpath href={`#${edgeId}`} />
          </animateMotion>
        </circle>
      )}
      {/* edge label */}
      {!idle && label && (
        <text className={labelClass} x={geo.labelX} y={geo.labelY} textAnchor="middle">
          {label}
        </text>
      )}
      {/* node */}
      <circle className={ringClass} cx={geo.cx} cy={geo.cy} r={r} />
      <text className={`node-label ${idle ? "node-label--dim" : ""}`} x={geo.cx} y={geo.cy - 6} textAnchor="middle">
        worker-{cell.slot}
      </text>
      <text className="node-sub" x={geo.cx} y={geo.cy + 9} textAnchor="middle">
        {cell.ticket ?? "idle"}
      </text>
      {!idle && (
        <text
          className="node-state"
          x={geo.cx}
          y={geo.cy + 24}
          textAnchor="middle"
          fill={cell.state === "blocked" ? "#d97757" : "#d97757"}
        >
          {cell.state}
        </text>
      )}
    </g>
  );
}

export function FleetGraph({ workers, feed }: { workers: WorkerCell[]; feed: FleetEvent[] }) {
  return (
    <div className="fleet-view">
      <div className="fleet-view__head">
        <h3>Fleet</h3>
        <span className="label">live topology</span>
      </div>
      <svg
        className="fleet-stage"
        viewBox="0 0 900 470"
        preserveAspectRatio="xMidYMid meet"
        role="img"
        aria-label="Fleet topology: the lead orchestrating its workers, with live message and work flow along the edges."
      >
        <defs>
          {/* literal hex — CSS vars don't resolve in marker attributes */}
          <marker id="ah-coral" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
            <path d="M0,0 L10,5 L0,10 z" fill="#d97757" />
          </marker>
        </defs>

        {workers.map((cell) => (
          <WorkerNode key={cell.slot} cell={cell} feed={feed} />
        ))}

        {/* lead node, drawn last so it sits above the edges */}
        <g>
          <circle className="node-ring node-ring--lead" cx={LEAD.cx} cy={LEAD.cy} r={LEAD.r} />
          <text className="node-label" x={LEAD.cx} y={LEAD.cy - 8} textAnchor="middle">
            orchestrator
          </text>
          <text className="node-sub" x={LEAD.cx} y={LEAD.cy + 8} textAnchor="middle">
            lead · opus
          </text>
          <text className="node-state" x={LEAD.cx} y={LEAD.cy + 24} textAnchor="middle" fill="#c69a4e">
            standby
          </text>
        </g>
      </svg>
      <div className="fleet-legend">
        <span><i className="legend-swatch" style={{ background: "var(--accent)" }} /> active work</span>
        <span><i className="legend-swatch" style={{ background: "var(--gold)" }} /> message in flight</span>
        <span><i className="legend-swatch" style={{ background: "var(--accent)" }} /> blocked → ask_lead</span>
        <span><i className="legend-swatch" style={{ background: "var(--border-soft)" }} /> idle link</span>
      </div>
    </div>
  );
}
