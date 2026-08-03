// The topology — a bespoke, directional live-flow view (not a recolored
// node-graph library). The orchestrator sits at the centre with the four workers
// around it, and an edge lights when a message has just crossed it, travelling
// in the direction it actually went.
//
// Repurposed for the TUI fleet. Two things changed and both were bugs of the same
// kind — a picture that showed something the system no longer does:
//
//  - `blocked` / `ask_lead` are gone. Nothing blocks any more; a live `claude`
//    TUI is always writable, which is the whole reason the mail queue and the
//    turn boundary went away. An edge that could still light "blocked" would be
//    drawing a state the fleet cannot enter.
//  - `edgeKind` read only `e.to`, so a worker→orch message never lit anything.
//    Half the traffic in a fleet whose product is conversation was invisible.

import { ORCH, isMessage, type FleetEvent, type PaneId, type PaneStatus } from "../fleet/types";

type EdgeKind = "inbound" | "outbound" | "quiet";

interface SlotLayout {
  cx: number;
  cy: number;
  /// Always drawn orch→worker; an inbound edge reverses the dot and the arrowhead.
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

/// How many of the newest events count as "just now" for lighting an edge.
const RECENT = 6;

/// Which way, if any, this worker's edge is lit. Reads **both** ends of a
/// message, so worker→orch traffic lights the edge exactly as orch→worker does.
function edgeKind(slot: number, recent: FleetEvent[]): EdgeKind {
  const pane: PaneId = `worker-${slot}`;
  for (const event of recent) {
    if (!isMessage(event)) continue;
    if (event.from === ORCH && event.to === pane) return "outbound";
    if (event.from === pane) return "inbound";
  }
  return "quiet";
}

function nodeRadius(status: PaneStatus): number {
  if (status === "live") return 40;
  return 32;
}

interface NodeProps {
  slot: number;
  status: PaneStatus;
  recent: FleetEvent[];
}

function WorkerNode({ slot, status, recent }: NodeProps) {
  const geo = LAYOUT[slot];
  if (!geo) return null;

  const kind = edgeKind(slot, recent);
  const quiet = kind === "quiet";
  const edgeId = `fe-s${slot}`;
  const ringClass =
    status === "live" ? "node-ring node-ring--active" : "node-ring node-ring--idle";

  return (
    <g>
      <path
        id={edgeId}
        className={`edge edge--${kind}`}
        d={geo.d}
        markerEnd={kind === "outbound" ? "url(#ah-coral)" : undefined}
        markerStart={kind === "inbound" ? "url(#ah-coral)" : undefined}
      />
      {!quiet && (
        <circle className="msg--mail" r={3.4}>
          <animateMotion
            dur="2s"
            repeatCount="indefinite"
            keyPoints={kind === "inbound" ? "1;0" : "0;1"}
            keyTimes="0;1"
            calcMode="linear"
          >
            <mpath href={`#${edgeId}`} />
          </animateMotion>
        </circle>
      )}
      {!quiet && (
        <text className="edge-label edge-label--mail" x={geo.labelX} y={geo.labelY} textAnchor="middle">
          {kind === "inbound" ? "→ orch" : "→ worker"}
        </text>
      )}
      <circle className={ringClass} cx={geo.cx} cy={geo.cy} r={nodeRadius(status)} />
      <text
        className={`node-label ${status === "live" ? "" : "node-label--dim"}`}
        x={geo.cx}
        y={geo.cy - 4}
        textAnchor="middle"
      >
        worker-{slot}
      </text>
      <text className="node-sub" x={geo.cx} y={geo.cy + 12} textAnchor="middle">
        {status === "live" ? "live" : status === "dead" ? "exited" : "standby"}
      </text>
    </g>
  );
}

interface FleetGraphProps {
  statuses: Record<PaneId, PaneStatus>;
  feed: FleetEvent[];
  leadModel: string;
}

export function FleetGraph({ statuses, feed, leadModel }: FleetGraphProps) {
  const recent = feed.slice(0, RECENT);
  const orchStatus = statuses[ORCH] ?? "idle";
  const slots = Object.keys(LAYOUT)
    .map(Number)
    .sort((a, b) => a - b);

  return (
    <div className="fleet-view">
      <div className="fleet-view__head">
        <h3>Topology</h3>
        <span className="label">who is talking to whom, live</span>
      </div>
      <svg
        className="fleet-stage"
        viewBox="0 0 900 470"
        preserveAspectRatio="xMidYMid meet"
        role="img"
        aria-label="Fleet topology: the orchestrator and its four worker terminals, with message flow along the edges."
      >
        <defs>
          {/* literal hex — CSS vars don't resolve in marker attributes */}
          <marker id="ah-coral" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
            <path d="M0,0 L10,5 L0,10 z" fill="#d97757" />
          </marker>
        </defs>

        {slots.map((slot) => (
          <WorkerNode
            key={slot}
            slot={slot}
            status={statuses[`worker-${slot}`] ?? "idle"}
            recent={recent}
          />
        ))}

        {/* orchestrator, drawn last so it sits above the edges */}
        <g>
          <circle className="node-ring node-ring--lead" cx={LEAD.cx} cy={LEAD.cy} r={LEAD.r} />
          <text className="node-label" x={LEAD.cx} y={LEAD.cy - 8} textAnchor="middle">
            orchestrator
          </text>
          <text className="node-sub" x={LEAD.cx} y={LEAD.cy + 8} textAnchor="middle">
            {leadModel}
          </text>
          <text className="node-state" x={LEAD.cx} y={LEAD.cy + 24} textAnchor="middle" fill="#c69a4e">
            {orchStatus === "live" ? "live" : orchStatus === "dead" ? "exited" : "standby"}
          </text>
        </g>
      </svg>
      <div className="fleet-legend">
        <span><i className="legend-swatch" style={{ background: "var(--gold)" }} /> message in flight</span>
        <span><i className="legend-swatch" style={{ background: "var(--accent)" }} /> live pane</span>
        <span><i className="legend-swatch" style={{ background: "var(--border-soft)" }} /> quiet link</span>
      </div>
    </div>
  );
}
