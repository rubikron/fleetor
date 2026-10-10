// Goals and tasks, folded out of chain entries (D-100). Mirrors
// `fleetor_core::task::board`; Rust is the authority when they disagree.

import type { ChainEvent, PaneId, TaskBlock, TaskStatus } from "./types";

export interface Owner {
  pane: PaneId;
  run: string;
  lineage: string;
}

/// A goal or task as it currently reads, with its whole chain oldest first.
export interface TaskRecord {
  number: number;
  block: TaskBlock;
  creator: PaneId;
  status: TaskStatus;
  owner: Owner | null;
  chain: ChainEvent[];
}

export function replayBoard(events: ChainEvent[]): TaskRecord[] {
  const oldestFirst = [...events].sort((a, b) => a.seq - b.seq);
  const order: number[] = [];
  const records = new Map<number, TaskRecord>();
  // The status each removed task had, for a restore to bring back.
  const before = new Map<number, TaskStatus>();

  for (const event of oldestFirst) {
    const { entry } = event;
    const owner = (pane: PaneId): Owner => ({ pane, run: event.run, lineage: event.lineage });
    if (entry.entry === "opened") {
      if (records.has(event.task)) continue;
      order.push(event.task);
      records.set(event.task, {
        number: event.task,
        block: entry.block,
        creator: event.from,
        status: "planned",
        owner: entry.block.owner ? owner(entry.block.owner) : null,
        chain: [event],
      });
      continue;
    }
    const record = records.get(event.task);
    if (!record) continue;
    const next: TaskRecord = { ...record, chain: [...record.chain, event] };
    if (entry.entry === "taken-up") {
      next.status = "in-progress";
      next.owner = owner(event.from);
    } else if (entry.entry === "status") {
      next.status = entry.status;
    } else if (entry.entry === "reviewer-set") {
      next.block = { ...record.block, reviewer: entry.new };
    } else if (entry.entry === "handoff") {
      next.status = "done";
    } else if (entry.entry === "released") {
      next.status = "planned";
      next.owner = null;
    } else if (entry.entry === "removed") {
      if (record.status !== "removed") before.set(event.task, record.status);
      next.status = "removed";
    } else if (entry.entry === "restored") {
      if (record.status === "removed") {
        next.status = before.get(event.task) ?? "planned";
        before.delete(event.task);
      }
    } else if (entry.entry === "attached") {
      next.block = { ...record.block, parent: entry.new };
    } else if (entry.entry === "edited") {
      next.block =
        entry.field === "outcome"
          ? { ...record.block, outcome: entry.new.join("\n") }
          : { ...record.block, [entry.field]: entry.new };
    }
    records.set(event.task, next);
  }

  return order.flatMap((number) => {
    const record = records.get(number);
    return record ? [record] : [];
  });
}

/// How many different panes have flagged a record as not worth doing.
export function flaggers(record: TaskRecord): number {
  return new Set(record.chain.filter((e) => e.entry.entry === "flagged").map((e) => e.from)).size;
}

/// Goals in the order they were opened, each with the tasks that name it, then
/// the tasks that name no goal on the board.
export function byGoal(board: TaskRecord[]): { goal: TaskRecord | null; tasks: TaskRecord[] }[] {
  const goals = board.filter((r) => r.block.kind === "goal");
  const numbers = new Set(goals.map((g) => g.number));
  const groups = goals.map((goal) => ({
    goal: goal as TaskRecord | null,
    tasks: board.filter((r) => r.block.kind === "task" && r.block.parent === goal.number),
  }));
  const stray = board.filter(
    (r) => r.block.kind === "task" && (r.block.parent == null || !numbers.has(r.block.parent)),
  );
  return stray.length > 0 ? [...groups, { goal: null, tasks: stray }] : groups;
}
