// The activity log: everything the backend has appended that is not a message.
//
// Messages have their own view, because they are the product. What is left here
// is the honest-failure channel — the target the fleet resolved, a `fleet` binary
// it could not find, a worktree it could not create, a socket that would not
// bind. Every one of those is a case where the shell otherwise looks fine while
// something the operator cares about is broken, so there has to be somewhere they
// are said out loud.
//
// Commands (D-045) appear here *and* in the message record, because they are the
// one thing the fleet does to a pane rather than says to it: an operator watching
// activity needs to see a worker's context being cleared, and an operator reading
// the record needs it in the timeline next to what was said around it.

import { isMessage, type FleetEvent } from "../fleet/types";

interface Rendered {
  kind: string;
  tone: "neutral" | "accent" | "gold" | "green" | "red";
  text: string;
  /// The coral rail — the one thing meant to catch the eye.
  rail: boolean;
}

function render(event: FleetEvent): Rendered {
  switch (event.type) {
    case "notice":
      return {
        kind: "notice",
        tone: event.level === "error" ? "red" : event.level === "warn" ? "gold" : "accent",
        text: event.text,
        rail: event.level !== "info",
      };
    case "pane-state":
      return {
        kind: "pane",
        tone: event.to === "dead" ? "red" : "neutral",
        text: `${event.pane} ${event.from} → ${event.to}`,
        rail: event.to === "dead",
      };
    // A command is not a message and must never read like one, so it says what
    // was run, at whom, and why — and the why is not truncated away, because it
    // is the reason the verb exists. Gold: noteworthy, not urgent.
    //
    // "accepted" is the word on purpose. The bytes reached a live pty; whether
    // Claude Code ran the command is not observable from here, and a feed saying
    // "cleared worker-2" would be claiming exactly that.
    case "command":
      return {
        kind: "command",
        tone: event.accepted ? "gold" : "red",
        text: event.accepted
          ? `${event.from} → ${event.to} · ${event.command} · accepted — why: ${event.why}`
          : `${event.from} → ${event.to} · ${event.command} · not accepted (${
              event.detail ?? "refused"
            }) — why: ${event.why}`,
        rail: !event.accepted,
      };
    // Messages have their own view; `EventFeed` filters them out before it gets
    // here, so this arm exists only to keep the switch exhaustive.
    case "message":
      return { kind: "message", tone: "neutral", text: event.body, rail: false };
  }
}

export function EventFeed({ feed }: { feed: FleetEvent[] }) {
  const rows = feed.filter((event) => !isMessage(event));
  return (
    <div className="events-view">
      <div className="events-view__head">
        <h3>Activity</h3>
        <span className="label">append-only · newest first</span>
        <span className="grow" style={{ flex: "1 1 auto" }} />
        {rows.length > 0 && <span className="mono text-mute">seq {rows[0].seq}</span>}
      </div>
      {rows.length === 0 ? (
        <div className="feed feed--empty">Nothing yet.</div>
      ) : (
        <div className="feed">
          {rows.map((event) => {
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
