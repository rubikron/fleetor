// The operator's inbox (WP-07) — what the fleet addressed to the human.
//
// **A surface, not a system.** No notification, no unread count, no chime, no
// nagging: the Open Loops alert was cut upstream for exactly this reason. It is
// a short list that is here when the operator looks, and it is short because
// nothing fans out to the operator — every row was addressed by name.
//
// Each row says `recorded`, never `accepted`. No terminal received it, because
// the human does not have one, and a word that promised otherwise would be the
// same lie as a delivery model that says "delivered" (L3).

import { OPERATOR, type MessageEvent } from "../fleet/types";

interface OperatorInboxProps {
  /// Newest-first, already filtered to what was addressed to the operator.
  messages: MessageEvent[];
}

export function OperatorInbox({ messages }: OperatorInboxProps) {
  return (
    <section className="inbox">
      <header className="inbox__head">
        <span className="inbox__title">
          inbox · addressed to <span className="mono">{OPERATOR}</span>
        </span>
        <span className="grow" style={{ flex: "1 1 auto" }} />
        <span className="mono text-mute">{messages.length}</span>
      </header>
      {messages.length === 0 ? (
        <p className="inbox__empty">
          Nothing addressed to you yet. A pane reaches you with{" "}
          <span className="mono">fleet send operator "…"</span>.
        </p>
      ) : (
        <ul className="inbox__list">
          {messages.map((m) => (
            <li className="inbox__item" key={m.id}>
              <span className="mono inbox__from">{m.from}</span>
              {/* The word is `recorded` and it is stated on every row, not
                  inferred from its absence: the human has no terminal, so no
                  pty took these bytes and none of them is `accepted`. */}
              <span className="inbox__outcome">recorded</span>
              <span className="inbox__body">{m.body}</span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
