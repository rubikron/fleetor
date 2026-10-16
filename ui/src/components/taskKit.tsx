// Pieces shared by the Tasks list, the task page and the task forms.

import { useEffect, useState, type ReactNode } from "react";
import type { TaskRecord } from "../fleet/board";
import type { ChainEvent, PaneId, TaskAction, TaskStatus } from "../fleet/types";
import type { TaskStoreInfo } from "../fleet/useFleet";

/// How the operator's controls reach the hub. Passed in rather than imported,
/// so the view renders with no backend behind it.
export interface TaskOps {
  /// One task change as `operator`; resolves to the task's number, rejects
  /// with the hub's refusal.
  run: (action: TaskAction) => Promise<string>;
  message: (to: PaneId, text: string) => Promise<unknown>;
}

export const NEEDS_FLEET = "start a fleet to change tasks";

export const isOpen = (record: TaskRecord): boolean =>
  record.status === "planned" || record.status === "in-progress";

/// An owner from another lineage is a pane that no longer exists.
export const earlier = (record: TaskRecord, store: TaskStoreInfo | null): boolean =>
  !!record.owner && !!store && record.owner.lineage !== store.lineage;

/// The time, re-read often enough for a minutes-old figure to stay true.
export function useNow(fixed?: number): number {
  const [now, setNow] = useState(() => fixed ?? Date.now());
  useEffect(() => {
    if (fixed !== undefined) return;
    const timer = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(timer);
  }, [fixed]);
  return fixed ?? now;
}

/// Run one change, holding the hub's refusal for display.
export function useChange(): [string | null, boolean, (change: () => Promise<void>) => void] {
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const act = (change: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    change()
      .catch((e) => setError(String(e)))
      .finally(() => setBusy(false));
  };
  return [error, busy, act];
}

const pad = (n: number) => String(n).padStart(2, "0");
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// `19:24`, local time.
export function clock(at: number): string {
  const d = new Date(at);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/// `2 Oct 19:24`.
export function dayClock(at: number): string {
  const d = new Date(at);
  return `${d.getDate()} ${MONTHS[d.getMonth()]} ${clock(at)}`;
}

/// Each run's start: the earliest entry carrying its id.
export function runStarts(chain: ChainEvent[]): Map<string, number> {
  const starts = new Map<string, number>();
  for (const event of chain) {
    const seen = starts.get(event.run);
    if (seen === undefined || event.at < seen) starts.set(event.run, event.at);
  }
  return starts;
}

/// `run 19:24`, or `an earlier run` when the start is unknown.
export const runName = (run: string, starts: Map<string, number>): string => {
  const at = starts.get(run);
  return at === undefined ? "an earlier run" : `run ${clock(at)}`;
};

/// Where a carried task's owner came from, or null when it is not carried.
export function carriedFrom(record: TaskRecord, store: TaskStoreInfo | null, starts: Map<string, number>): string | null {
  if (!isOpen(record) || !earlier(record, store) || !record.owner) return null;
  return `from ${runName(record.owner.run, starts)}`;
}

export function Status({ status }: { status: TaskStatus }) {
  return <span className={`task__status task__status--${status}`}>{status}</span>;
}

const FIELD: Record<string, string> = {
  outcome: "the outcome",
  technical: "the technical criteria",
  vision: "the vision criteria",
};

/// One chain entry in a sentence, for a row's "latest" line.
export function summary(event: ChainEvent): string {
  const { entry } = event;
  switch (entry.entry) {
    case "opened":
      return "opened this";
    case "taken-up":
      return "took this up";
    case "status":
      return `marked it ${entry.status}`;
    case "commented":
      return entry.text;
    case "edited":
      return `edited ${FIELD[entry.field]}`;
    case "receipt":
      return `ran a check · ${entry.status}`;
    case "reviewer-set":
      return `named ${entry.new} reviewer`;
    case "reviewed":
      return `reviewed it${entry.requested ? "" : " (unrequested)"}: ${entry.met ? "met" : "not met"}`;
    case "flagged":
      return "flagged it as not worth doing";
    case "removed":
      return "removed it";
    case "restored":
      return "restored it";
    case "attached":
      return entry.old ? `moved it under #${entry.new}, was under #${entry.old}` : `put it under #${entry.new}`;
    case "handoff":
      return "handed off this goal";
    case "released":
      return entry.on_behalf_of ? `released this on behalf of ${entry.on_behalf_of}` : "released this";
  }
}

/// A row of pill-shaped choices; `dashed` marks the "nobody" choice.
export function Chips<T extends string>({
  label,
  options,
  value,
  onChange,
  dashed,
  disabled,
  title,
}: {
  label: string;
  options: [T, string][];
  value: T;
  onChange: (value: T) => void;
  dashed?: T;
  disabled?: boolean;
  title?: string;
}) {
  return (
    <span className="task-chips" role="group" aria-label={label}>
      {options.map(([option, text]) => (
        <button
          key={option}
          type="button"
          className={`task-chip ${option === dashed ? "task-chip--dash" : ""} ${value === option ? "is-on" : ""}`}
          aria-pressed={value === option}
          disabled={disabled}
          title={title}
          onClick={() => onChange(option)}
        >
          {text}
        </button>
      ))}
    </span>
  );
}

/// One form row: a bold label with a quiet hint, beside its input.
export function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <>
      <span className="task-form__k">
        {label}
        {hint && <small>{hint}</small>}
      </span>
      <div className="task-form__v">{children}</div>
    </>
  );
}

/// A list typed one item per input, with a link to add another.
export function ListInput({
  items,
  onChange,
  add,
  placeholder,
}: {
  items: string[];
  onChange: (items: string[]) => void;
  add: string;
  placeholder: string;
}) {
  const shown = items.length === 0 ? [""] : items;
  return (
    <div className="task-form__stack">
      {shown.map((item, index) => (
        <input
          key={index}
          className="task-input"
          value={item}
          placeholder={index === shown.length - 1 && index > 0 ? placeholder : undefined}
          onChange={(e) => onChange(shown.map((old, i) => (i === index ? e.target.value : old)))}
        />
      ))}
      <button type="button" className="task-form__add" onClick={() => onChange([...shown, ""])}>
        {add}
      </button>
    </div>
  );
}

/// The typed list, without blank inputs.
export const filled = (items: string[]): string[] => items.map((item) => item.trim()).filter((item) => item.length > 0);
