// The board, replayed from the task events — the UI's mirror of
// `fleetor_core::task::board`.
//
// **Rust is the authority.** `fleet task list` and this file must agree, and
// when they do not it is this one that is wrong: the CLI's answer is computed by
// the hub over the same log. The fold is duplicated rather than fetched because
// the events already arrive here on the live bus, and a Tauri command that
// re-asked the backend for something the UI has already received would be a
// second source of truth for a screen that is meant to be a mirror.
//
// The rules, all three from `task.rs`: entries come back in the order they were
// posted; a later `posted` for an id already on the board replaces it; an
// `updated` for an id that was never posted is skipped, because there is no
// block for it to be a claim about.
//
// Nothing here validates the tree links. A block may name a parent that is not
// on the board, and two blocks may name each other — the view renders the links
// as written (see TaskBoard.tsx), because the shape is somebody's note about how
// the work fits together, not a workflow anything executes.

import type { PaneId, TaskBlock, TaskEvent, TaskStatus } from "./types";

/// One claim appended after the block went up. Kept in full: a status with no
/// reasoning is an effect whose cause was thrown away.
export interface TaskNote {
  from: PaneId;
  at: number;
  status?: TaskStatus | null;
  note?: string | null;
}

/// One block as the board currently reads.
export interface BoardEntry {
  id: string;
  block: TaskBlock;
  postedBy: PaneId;
  postedAt: number;
  /// The most recent status *claimed* — `planned` until somebody says otherwise,
  /// never inferred from anything the fleet did.
  status: TaskStatus;
  updates: TaskNote[];
}

export function replayBoard(events: TaskEvent[]): BoardEntry[] {
  // `useFleet` keeps its lists newest-first; a fold has to run the other way.
  const oldestFirst = [...events].sort((a, b) => a.seq - b.seq);
  const order: string[] = [];
  const entries = new Map<string, BoardEntry>();

  for (const event of oldestFirst) {
    if (event.change.change === "posted") {
      if (!entries.has(event.task)) order.push(event.task);
      entries.set(event.task, {
        id: event.task,
        block: event.change.block,
        postedBy: event.from,
        postedAt: event.at,
        status: "planned",
        updates: [],
      });
      continue;
    }
    const existing = entries.get(event.task);
    if (!existing) continue;
    const note: TaskNote = {
      from: event.from,
      at: event.at,
      status: event.change.status,
      note: event.change.note,
    };
    entries.set(event.task, {
      ...existing,
      status: note.status ?? existing.status,
      updates: [...existing.updates, note],
    });
  }

  return order.flatMap((id) => {
    const entry = entries.get(id);
    return entry ? [entry] : [];
  });
}
