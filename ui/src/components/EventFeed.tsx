// The live event log (handoff §11, "Event log incl. DMs"). Newest first, one line
// per event, coloured by kind but always labelled. A blocked/notice line gets the
// coral accent rail — the one thing meant to catch the eye (handoff §11).

import type { FleetEvent } from "../fleet/types";

interface Rendered {
  kind: string;
  tone: "neutral" | "accent" | "gold" | "green" | "red";
  text: string;
  rail: boolean;
}

function render(event: FleetEvent): Rendered {
  switch (event.type) {
    case "worker-state":
      return {
        kind: "worker",
        tone: event.to === "blocked" ? "gold" : event.to === "working" ? "accent" : "neutral",
        text: `worker-${event.slot} ${event.from} → ${event.to}`,
        rail: event.to === "blocked",
      };
    case "ticket-state":
      return { kind: "ticket", tone: "neutral", text: `${event.ticket} ${event.from} → ${event.to}`, rail: false };
    case "tool-activity":
      return { kind: "tool", tone: "accent", text: `worker-${event.slot} · ${event.ticket} · ${event.tool}`, rail: false };
    case "report-filed":
      return { kind: "report", tone: "green", text: `${event.ticket} filed report — ${event.status}`, rail: false };
    case "mail":
      return { kind: "mail", tone: "neutral", text: `${event.from} → ${event.to} (${event.kind})`, rail: false };
    case "gate-result":
      return {
        kind: "gate",
        tone: event.outcome === "pass" ? "green" : "red",
        text: `${event.ticket} gate ${event.outcome}`,
        rail: event.outcome === "fail",
      };
    case "review-result":
      return {
        kind: "review",
        tone: event.outcome === "approved" ? "green" : "gold",
        text: `${event.ticket} review by w${event.reviewer_slot} — ${event.outcome}`,
        rail: false,
      };
    case "notice":
      return {
        kind: "notice",
        tone: event.level === "error" ? "red" : event.level === "warn" ? "gold" : "accent",
        text: event.text,
        rail: event.level !== "info",
      };
  }
}

export function EventFeed({ feed }: { feed: FleetEvent[] }) {
  return (
    <div className="events-view">
      <div className="events-view__head">
        <h3>Event log</h3>
        <span className="label">append-only · newest first</span>
        <span className="grow" style={{ flex: "1 1 auto" }} />
        {feed.length > 0 && <span className="mono text-mute">seq {feed[0].seq}</span>}
      </div>
      {feed.length === 0 ? (
        <div className="feed feed--empty">No events yet. Assign a ticket or run the demo lifecycle.</div>
      ) : (
        <div className="feed">
          {feed.map((event) => {
            const r = render(event);
            return (
              <div key={event.seq} className={`line line--${r.tone} ${r.rail ? "line--rail" : ""}`}>
                <span className="mono line__seq">{event.seq}</span>
                <span className={`mono line__kind line__kind--${r.tone}`}>{r.kind}</span>
                <span className="line__text">{r.text}</span>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
