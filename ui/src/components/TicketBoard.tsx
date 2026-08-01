// The ticket board: cards grouped into the handoff's columns, driven straight
// from the store snapshot the hook keeps fresh. State is cued by a full 1px
// border tint plus a foot dot + chip — no side-tab accent bar (the redesign's
// first anti-slop fix). Monospace for the system-owned bits (ids, slots, paths).

import { BOARD_COLUMNS, type Ticket, type TicketState } from "../fleet/types";

const CARD_MOD: Record<TicketState, string> = {
  backlog: "backlog",
  assigned: "active",
  "in-progress": "active",
  "in-review": "review",
  done: "done",
  blocked: "blocked",
  failed: "blocked",
};

const FOOT: Record<TicketState, { tone: string; chip: string; label: string }> = {
  backlog: { tone: "idle", chip: "", label: "backlog" },
  assigned: { tone: "accent", chip: "accent", label: "assigned" },
  "in-progress": { tone: "accent", chip: "accent", label: "in progress" },
  "in-review": { tone: "gold", chip: "gold", label: "in review" },
  done: { tone: "green", chip: "green", label: "done" },
  blocked: { tone: "accent", chip: "accent", label: "blocked" },
  failed: { tone: "red", chip: "accent", label: "failed" },
};

function TicketCard({ ticket }: { ticket: Ticket }) {
  const foot = FOOT[ticket.state];
  return (
    <article className={`card card--${CARD_MOD[ticket.state]}`}>
      <div className="card__top">
        <span className="mono card__id">{ticket.id}</span>
        {ticket.slot != null && <span className="mono card__slot">w{ticket.slot}</span>}
      </div>
      <div className="card__title">{ticket.title}</div>
      {ticket.files_owned.length > 0 && (
        <div className="mono card__files">{ticket.files_owned.join(" · ")}</div>
      )}
      <div className="card__foot">
        <span className={`dot dot--${foot.tone}`} />
        <span className={`chip ${foot.chip ? `chip--${foot.chip}` : ""}`}>{foot.label}</span>
      </div>
    </article>
  );
}

export function TicketBoard({ board }: { board: Ticket[] }) {
  return (
    <div className="board">
      {BOARD_COLUMNS.map(({ state, label }) => {
        const cards = board.filter((t) => t.state === state);
        return (
          <div key={state} className="column">
            <div className="column__head">
              <span>{label}</span>
              <span className="mono column__count">{cards.length}</span>
            </div>
            <div className="column__body">
              {cards.map((t) => (
                <TicketCard key={t.id} ticket={t} />
              ))}
            </div>
          </div>
        );
      })}
    </div>
  );
}
