// Monitor flags: what a task's chain shows is missing (PRD stories 10, 67–69).
//
// Worked out on screen from the chain and its timestamps. Nothing here is
// stored, and nothing reads it to act on an agent.

import type { TaskRecord } from "./board";

/// How long a task may sit assigned before "never taken up" shows.
export const NEVER_TAKEN_UP_MS = 10 * 60 * 1000;

export type MonitorFlag = {
  kind: "never-taken-up" | "no-receipt" | "no-verdict" | "open-tasks";
  text: string;
};

const isOpen = (record: TaskRecord): boolean =>
  record.status === "planned" || record.status === "in-progress";

const lastIndex = (record: TaskRecord, match: (index: number) => boolean): number => {
  for (let i = record.chain.length - 1; i >= 0; i--) if (match(i)) return i;
  return -1;
};

/// When a planned task with an owner was last handed to them, or null when
/// nobody is being waited on. An owner from another lineage than `lineage` is
/// a pane that no longer exists, so nobody waits on it.
export function assignedAt(record: TaskRecord, lineage?: string | null): number | null {
  if (record.block.kind !== "task" || record.status !== "planned" || !record.owner) return null;
  if (lineage !== undefined && record.owner.lineage !== lineage) return null;
  const since = lastIndex(record, (i) =>
    ["opened", "status", "released"].includes(record.chain[i].entry.entry),
  );
  return since < 0 ? null : record.chain[since].at;
}

/// `4m`, `1h 12m`.
export function elapsed(ms: number): string {
  const minutes = Math.max(0, Math.floor(ms / 60_000));
  return minutes < 60 ? `${minutes}m` : `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

/// The latest receipt and verdict on a task, for its row.
export function evidence(record: TaskRecord) {
  const receipts = record.chain.flatMap((e) => (e.entry.entry === "receipt" ? [e.entry] : []));
  const verdicts = record.chain.flatMap((e) => (e.entry.entry === "reviewed" ? [e.entry] : []));
  return { receipts, receipt: receipts[receipts.length - 1], verdict: verdicts[verdicts.length - 1] };
}

export function monitorFlags(
  record: TaskRecord,
  board: TaskRecord[],
  now: number,
  lineage?: string | null,
): MonitorFlag[] {
  const flags: MonitorFlag[] = [];
  if (record.block.kind === "goal") {
    const open = board.filter((r) => r.block.parent === record.number && isOpen(r)).length;
    if (!isOpen(record) && open > 0) {
      flags.push({ kind: "open-tasks", text: `closed with ${open} open task${open === 1 ? "" : "s"}` });
    }
    return flags;
  }
  const assigned = assignedAt(record, lineage);
  if (assigned !== null && now - assigned >= NEVER_TAKEN_UP_MS) {
    flags.push({ kind: "never-taken-up", text: "assigned, never taken up" });
  }
  if (record.status === "done") {
    // A receipt counts from the latest take-up; a verdict from the latest done.
    const takenUp = lastIndex(record, (i) => record.chain[i].entry.entry === "taken-up");
    if (!record.chain.some((e, i) => i > takenUp && e.entry.entry === "receipt")) {
      flags.push({ kind: "no-receipt", text: "done, no receipt" });
    }
    const reviewer = record.block.reviewer;
    const done = lastIndex(record, (i) => {
      const { entry } = record.chain[i];
      return entry.entry === "status" && entry.status === "done";
    });
    if (
      reviewer &&
      !record.chain.some((e, i) => i > done && e.entry.entry === "reviewed" && e.from === reviewer)
    ) {
      flags.push({ kind: "no-verdict", text: `done, no verdict from ${reviewer}` });
    }
  }
  return flags;
}
