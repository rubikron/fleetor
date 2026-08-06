// The board: the decomposition the fleet agreed on, as everyone can see it.
//
// A sibling of MessageFeed (the unbounded record) and EventFeed (the bounded
// tail), and the third thing this shell shows. Where those two are timelines,
// this is a *state* — replayed from the same log, in `fleet/board.ts`.
//
// Three rules it must not break:
//
//  1. **Read-only, and visibly so.** There is no post button, no status
//     dropdown, no drag-to-reorder. The board is maintained by the agents
//     through `fleet task`; an operator control here would be a second writer
//     with no attribution, and every entry on this screen is an attributed
//     claim. WP-07 is where the operator gets a voice, and it will be by
//     speaking to the fleet, not by editing its record behind its back.
//  2. **A status is a claim, never a verdict.** `done` is what somebody said,
//     unverified until WP-06's review — so the card says who claimed it and
//     shows the note they left, and nothing renders as a checkmark or a
//     progress bar. A board that looked like a burndown would be asserting
//     completion this product cannot observe.
//  3. **Links are shown as written.** A parent id that names nothing on the
//     board still renders, as a chip, dimmed. Cycles need no special case at
//     all, because this view does not build a tree: children are shown by an
//     explicit indent one level deep, and anything whose parent is not directly
//     above it keeps its chip instead. Nothing here validates a graph.

import { useMemo } from "react";
import { replayBoard, type BoardEntry } from "../fleet/board";
import type { TaskEvent, TaskStatus } from "../fleet/types";

/// Gold labels, never a colour alone (building.md §7 rule 3) — every pill
/// carries its word. Coral is reserved for attention; a `done` claim is not
/// urgent and a `dropped` one is not an error.
const STATUS_TONE: Record<TaskStatus, string> = {
  planned: "task__status--planned",
  claimed: "task__status--claimed",
  done: "task__status--done",
  dropped: "task__status--dropped",
};

function Chip({ label, id }: { label: string; id: string }) {
  return (
    <span className="task__link">
      {label} <span className="mono">{id}</span>
    </span>
  );
}

function Criteria({ label, items }: { label: string; items: string[] }) {
  if (items.length === 0) return null;
  return (
    <div className="task__crits">
      <span className="task__crits-label">{label}</span>
      <ul className="task__crit-list">
        {items.map((item, index) => (
          <li key={`${index}-${item}`} className="task__crit">
            {item}
          </li>
        ))}
      </ul>
    </div>
  );
}

function TaskCard({ entry, nested }: { entry: BoardEntry; nested: boolean }) {
  const { block } = entry;
  return (
    <article className={`task ${nested ? "task--nested" : ""}`}>
      <header className="task__head">
        <span className="mono task__id">{entry.id}</span>
        <span className={`task__status ${STATUS_TONE[entry.status]}`}>{entry.status}</span>
        <span className="mono task__worker">{block.worker}</span>
        <span className="grow" style={{ flex: "1 1 auto" }} />
        <span className="task__posted">
          posted by <span className="mono">{entry.postedBy}</span>
        </span>
      </header>

      <p className="task__outcome">{block.outcome}</p>

      {/* The criteria are the point of a block, so they are on the card rather
          than behind a disclosure: a board you have to expand to see what
          "done" means is a board of titles. */}
      <Criteria label="technical" items={block.technical} />
      <Criteria label="vision" items={block.semantic} />

      {block.instructions && <p className="task__instructions">{block.instructions}</p>}

      {(block.parent || block.converges_on) && (
        <div className="task__links">
          {block.parent && <Chip label="cut from" id={block.parent} />}
          {block.converges_on && <Chip label="converges on" id={block.converges_on} />}
        </div>
      )}

      {entry.updates.length > 0 && (
        <footer className="task__trail">
          {entry.updates.map((update, index) => (
            <div key={`${update.at}-${index}`} className="task__claim">
              <span className="mono task__claim-from">{update.from}</span>
              {update.status && (
                <span className={`task__status ${STATUS_TONE[update.status]}`}>
                  {update.status}
                </span>
              )}
              {/* Never truncated. The note is why a status changed, and a trail
                  of statuses with the reasoning dropped is the part of the
                  record that would have been worth keeping. */}
              {update.note && <span className="task__claim-note">{update.note}</span>}
            </div>
          ))}
        </footer>
      )}
    </article>
  );
}

export function TaskBoard({ tasks }: { tasks: TaskEvent[] }) {
  const board = useMemo(() => replayBoard(tasks), [tasks]);
  const claimed = board.filter((entry) => entry.status !== "planned").length;

  return (
    <div className="events-view">
      <div className="events-view__head">
        <h3>Tasks</h3>
        <span className="label">
          the fleet's own record of the work · a status is a claim, not a verdict
        </span>
        <span className="grow" style={{ flex: "1 1 auto" }} />
        <span className="mono text-mute">
          {claimed}/{board.length}
        </span>
      </div>
      {board.length === 0 ? (
        <div className="feed feed--empty">
          Nothing on the board. Once the vision is confirmed, the orchestrator cuts the work into
          blocks with{" "}
          <span className="mono">
            fleet task post --to 2 --outcome "…" --crit-t "…" --crit-s "…"
          </span>
          .
        </div>
      ) : (
        <div className="feed feed--tasks">
          {board.map((entry, index) => (
            <TaskCard
              key={entry.id}
              entry={entry}
              // One level of indent, and only when the parent is the card
              // directly above — enough to read a decomposition, and not a tree
              // walk that would have to decide what a cycle means.
              nested={!!entry.block.parent && entry.block.parent === board[index - 1]?.id}
            />
          ))}
        </div>
      )}
    </div>
  );
}
