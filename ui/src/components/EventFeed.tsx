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

import { isMessage, type ChainEvent, type FleetEvent } from "../fleet/types";
import type { TaskFeedEntry } from "../fleet/useFleet";

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
    // The harness and the model are said here or nowhere: the roster is
    // deliberately harness-free, because a worker that knows which peer is
    // "weaker" starts routing work on that belief (M25). This line is the
    // operator's, not a pane's, and it is the only place the live feed can be as
    // specific about a mixed fleet as the archive of it will be (M24).
    case "pane-state":
      return {
        kind: "pane",
        tone: event.to === "dead" ? "red" : "neutral",
        text: event.harness
          ? `${event.pane} ${event.from} → ${event.to} · ${event.harness}${
              event.model ? ` · ${event.model}` : ""
            }`
          : `${event.pane} ${event.from} → ${event.to}`,
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
    // A task claim (WP-05). It belongs here as well as on the board, because the
    // board shows the *state* and this shows the moment somebody changed it —
    // and who. Never a rail: a block being posted or claimed is the fleet
    // working, not something wrong.
    case "task":
      return {
        kind: "task",
        tone: "gold",
        text:
          event.change.change === "posted"
            ? `${event.from} posted ${event.task} · ${event.change.block.worker} · ${event.change.block.outcome}`
            : `${event.from} → ${event.task}${
                event.change.status ? ` · ${event.change.status}` : ""
              }${event.change.note ? ` — ${event.change.note}` : ""}`,
        rail: false,
      };
    // The orchestrator saying the whole goal is met (WP-13). Coral and railed —
    // the palette's rule is that the accent means *needs or has attention*, and
    // this is the one line in the feed the operator must not scroll past. It is
    // not an error, and the text says a claim was made rather than that anything
    // was verified: nothing in this app checked a word of it.
    //
    // The evidence and the loose ends are printed in full, for the reason a
    // command's `why` is: they are the whole of what makes the claim checkable,
    // and a truncated handoff is a summary of a summary.
    case "handoff":
      return {
        kind: "handoff",
        tone: "accent",
        text: `${event.from} says the goal is met · ${event.built} — evidence: ${event.evidence.join(
          " · ",
        )}${event.open?.length ? ` — still open: ${event.open.join(" · ")}` : ""}`,
        rail: true,
      };
    // Messages have their own view; `EventFeed` filters them out before it gets
    // here, so this arm exists only to keep the switch exhaustive.
    case "message":
      return { kind: "message", tone: "neutral", text: event.body, rail: false };
  }
}

/// One chain entry as a feed line (D-100): who did what to which task.
function renderTask(event: ChainEvent): Rendered {
  const { entry } = event;
  const note = (text?: string | null) => (text ? ` — ${text}` : "");
  const what =
    entry.entry === "opened"
      ? `opened ${entry.block.kind} #${event.task} · ${entry.block.outcome}`
      : entry.entry === "taken-up"
        ? `took up #${event.task}${note(entry.note)}`
        : entry.entry === "status"
          ? `marked #${event.task} ${entry.status}${note(entry.note)}`
          : entry.entry === "commented"
            ? `commented on #${event.task} — ${entry.text}`
            : entry.entry === "released"
              ? `released #${event.task}${
                  entry.on_behalf_of ? ` on behalf of ${entry.on_behalf_of}` : ""
                } — left: ${entry.left}`
              : `edited #${event.task} · ${entry.field}`;
  return { kind: "task", tone: "gold", text: `${event.from} ${what}`, rail: false };
}

interface Row {
  key: string;
  label: string;
  rendered: Rendered;
  /// Run-log seq, or the seq a task entry arrived after; then the entry's own.
  order: [number, number];
}

export function EventFeed({ feed, tasks = [] }: { feed: FleetEvent[]; tasks?: TaskFeedEntry[] }) {
  const rows: Row[] = [
    ...feed
      .filter((event) => !isMessage(event))
      .map((event) => ({
        key: `e${event.seq}`,
        label: String(event.seq),
        rendered: render(event),
        order: [event.seq, 0] as [number, number],
      })),
    ...tasks.map(({ event, after }) => ({
      key: `t${event.seq}`,
      label: `#${event.task}`,
      rendered: renderTask(event),
      order: [after, event.seq] as [number, number],
    })),
  ].sort((a, b) => b.order[0] - a.order[0] || b.order[1] - a.order[1]);
  const latest = rows.find((row) => row.order[1] === 0);
  return (
    <div className="events-view">
      <div className="events-view__head">
        <h3>Activity</h3>
        <span className="label">append-only · newest first</span>
        <span className="grow" style={{ flex: "1 1 auto" }} />
        {latest && <span className="mono text-mute">seq {latest.label}</span>}
      </div>
      {rows.length === 0 ? (
        <div className="feed feed--empty">Nothing yet.</div>
      ) : (
        <div className="feed">
          {rows.map(({ key, label, rendered: r }) => (
            <div key={key} className={`line line--${r.tone} ${r.rail ? "line--rail" : ""}`}>
              <span className="mono line__seq">{label}</span>
              <span className={`mono line__kind line__kind--${r.tone}`}>{r.kind}</span>
              <span className="line__text">{r.text}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
