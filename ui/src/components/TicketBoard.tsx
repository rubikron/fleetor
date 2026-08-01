// The ticket board: cards grouped into the handoff's columns, driven straight
// from the store snapshot the hook keeps fresh. Monospace for the system-owned
// bits (ids, slots, paths); sans for the human title.

import { BOARD_COLUMNS, type Ticket } from "../fleet/types";

function TicketCard({ ticket }: { ticket: Ticket }) {
  return (
    <article className="card">
      <div className="card__top">
        <span className="mono card__id">{ticket.id}</span>
        {ticket.slot != null && <span className="mono card__slot">w{ticket.slot}</span>}
      </div>
      <div className="card__title">{ticket.title}</div>
      {ticket.files_owned.length > 0 && (
        <div className="mono card__files">{ticket.files_owned.join(" · ")}</div>
      )}
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
