// The record: every message the fleet has sent, newest first, with its body.
//
// This view is the product. Three rules it must not break:
//
//  1. **The body is shown.** The event it replaces (`FleetEvent::Mail`) carried
//     only who-to-whom; a product about watching agents talk cannot keep the
//     words out of its own log.
//  2. **`accepted` is never rendered as "delivered."** A zero exit means the
//     bytes reached a live terminal, not that the agent read them (L3). A
//     refused message stays on the list, marked, rather than disappearing.
//  3. **Nothing here is dropped.** The list is unbounded on purpose; the
//     activity log's 300-row tail does not apply to it.
//
// A broadcast arrives as N legs sharing a `group`, so they collapse back into
// the one gesture that produced them — with the legs that did not land named.
//
// **Commands share this timeline but never a row shape (D-045).** A `fleet cmd`
// is not something a pane said, so rendering it as a message would put words in
// a pane's mouth. It gets its own card — the command in mono, the why beneath it,
// and the word "accepted" rather than any claim that it ran.

import { useMemo } from "react";
import { OperatorComposer } from "./OperatorComposer";
import { OperatorInbox } from "./OperatorInbox";
import { inbox, isFromOperator, outcomeOf, replyTarget } from "../fleet/inbox";
import type { CommandEvent, MessageEvent, PaneId } from "../fleet/types";

/// One rendered row: either a direct message, or a whole broadcast.
interface Row {
  key: string;
  seq: number;
  from: PaneId;
  /// A single target, or the fan-out's targets.
  to: PaneId[];
  body: string;
  broadcast: boolean;
  /// The human said it. A mark on the row, never a reordering of it.
  fromOperator: boolean;
  /// It entered the log and no pty exists — the addressee is the operator
  /// (WP-07). Not a failure, and never counted as one.
  recorded: boolean;
  /// Every leg that a real terminal did not take, with the reason it gave.
  failures: { to: PaneId; detail: string }[];
}

/// The legs of one message that did not reach a pty **and should have** — so a
/// message to the operator, which never had one to reach, is not in here.
function failuresOf(m: MessageEvent): { to: PaneId; detail: string }[] {
  return outcomeOf(m.to, m.accepted) === "undelivered"
    ? [{ to: m.to, detail: m.detail ?? "refused" }]
    : [];
}

function collapse(messages: MessageEvent[]): Row[] {
  const rows: Row[] = [];
  const groups = new Map<string, Row>();

  // `messages` is newest-first; a group's row takes the position of its newest
  // leg, and later (older) legs fold into it.
  for (const m of messages) {
    if (!m.group) {
      rows.push({
        key: m.id,
        seq: m.seq,
        from: m.from,
        to: [m.to],
        body: m.body,
        broadcast: false,
        fromOperator: isFromOperator(m),
        recorded: outcomeOf(m.to, m.accepted) === "recorded",
        failures: failuresOf(m),
      });
      continue;
    }
    const existing = groups.get(m.group);
    if (existing) {
      existing.to.push(m.to);
      existing.failures.push(...failuresOf(m));
      continue;
    }
    const row: Row = {
      key: m.group,
      seq: m.seq,
      from: m.from,
      to: [m.to],
      body: m.body,
      broadcast: true,
      fromOperator: isFromOperator(m),
      // A fan-out never has the operator among its legs — nothing broadcasts to
      // a participant with no pty — so a broadcast row is never recorded.
      recorded: false,
      failures: failuresOf(m),
    };
    groups.set(m.group, row);
    rows.push(row);
  }
  return rows;
}

function MessageRow({ row }: { row: Row }) {
  const failed = row.failures.length > 0;
  return (
    <article className={`msg ${row.fromOperator ? "msg--operator" : ""}`}>
      <header className="msg__head">
        <span className="mono msg__from">{row.from}</span>
        <span className="msg__arrow">→</span>
        <span className="mono msg__to">
          {row.broadcast ? `all (${row.to.length})` : row.to[0]}
        </span>
        {row.broadcast && <span className="msg__tag">broadcast</span>}
        <span className="grow" style={{ flex: "1 1 auto" }} />
        <span className="mono text-mute">{row.seq}</span>
      </header>
      <p className="msg__body">{row.body}</p>
      {/* "recorded", never "accepted": the human has no terminal, so no pty
          took these bytes. Stated rather than left blank — an unmarked row
          would read as a delivery that quietly worked. */}
      {row.recorded && (
        <footer className="msg__failures">
          <span className="msg__note">recorded — it is in the log and the operator's inbox</span>
        </footer>
      )}
      {failed && (
        <footer className="msg__failures">
          {row.failures.map((f) => (
            // "undelivered", never "failed to send" — the send happened; it is
            // the arrival that did not.
            <span key={f.to} className="msg__failure">
              undelivered to <span className="mono">{f.to}</span> — {f.detail}
            </span>
          ))}
        </footer>
      )}
    </article>
  );
}

function CommandRow({ event }: { event: CommandEvent }) {
  return (
    <article className="msg msg--command">
      <header className="msg__head">
        <span className="mono msg__from">{event.from}</span>
        <span className="msg__arrow">→</span>
        <span className="mono msg__to">{event.to}</span>
        <span className="msg__tag">command</span>
        <span className="grow" style={{ flex: "1 1 auto" }} />
        <span className="mono text-mute">{event.seq}</span>
      </header>
      <p className="mono msg__command">{event.command}</p>
      {/* The why is never collapsed or hidden. It is the record the whole verb
          exists to keep — an entry showing only the effect would be the thing
          this feature was built to avoid. */}
      <p className="msg__why">
        <span className="msg__why-label">why</span> {event.why}
      </p>
      <footer className="msg__failures">
        {event.accepted ? (
          // "accepted", never "ran": the bytes reached a live pty and nothing on
          // this side knows whether Claude Code executed them (L3).
          <span className="msg__note">accepted — the bytes reached the terminal</span>
        ) : (
          <span className="msg__failure">
            not accepted — {event.detail ?? "the pane refused"}
          </span>
        )}
      </footer>
    </article>
  );
}

/// One entry in the record: a message row, or a command. Merged on `seq` so the
/// two read as one timeline rather than two lists that have to be cross-referenced.
type Entry = { seq: number; key: string; message?: Row; command?: CommandEvent };

function timeline(messages: MessageEvent[], commands: CommandEvent[]): Entry[] {
  const entries: Entry[] = collapse(messages).map((row) => ({
    seq: row.seq,
    key: row.key,
    message: row,
  }));
  for (const command of commands) {
    entries.push({ seq: command.seq, key: command.id, command });
  }
  return entries.sort((a, b) => b.seq - a.seq);
}

export function MessageFeed({
  messages,
  commands,
}: {
  messages: MessageEvent[];
  commands: CommandEvent[];
}) {
  const rows = useMemo(() => timeline(messages, commands), [messages, commands]);
  const mine = useMemo(() => inbox(messages), [messages]);
  const replyTo = useMemo(() => replyTarget(messages), [messages]);

  return (
    <div className="events-view">
      <div className="events-view__head">
        <h3>Messages</h3>
        <span className="label">
          everything the fleet has said and every command it ran · newest first
        </span>
        <span className="grow" style={{ flex: "1 1 auto" }} />
        <span className="mono text-mute">{rows.length}</span>
      </div>
      {rows.length === 0 ? (
        <div className="feed feed--empty">
          Nothing yet. Start the fleet, then ask the orchestrator to run{" "}
          <span className="mono">fleet send 2 "say hello back with fleet reply"</span>.
        </div>
      ) : (
        <div className="feed feed--messages">
          {rows.map((entry) =>
            entry.command ? (
              <CommandRow key={entry.key} event={entry.command} />
            ) : (
              <MessageRow key={entry.key} row={entry.message!} />
            ),
          )}
        </div>
      )}
      {/* The operator's own end of the record (WP-07). Below the feed rather
          than beside it: the record is the product, and the human's two
          surfaces are a footer to it, not a second column competing with it.
          Both stay mounted — an inbox that appeared only when it had something
          would be attention machinery wearing a layout's clothes. */}
      <OperatorInbox messages={mine} />
      <OperatorComposer replyTo={replyTo} />
    </div>
  );
}
