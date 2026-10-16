// The Tasks view: goals, the tasks cut from them, and each one's chain (D-100).
//
// Folded from the task store's chain entries in `fleet/board.ts`. A status is
// what someone said, so nothing here renders a tick or a progress bar; a goal's
// tasks are counted by status as text. The list, a task page and the new task
// form each take the whole view (D-121).

import { useMemo, useState } from "react";
import { byGoal, flaggers, replayBoard, type TaskRecord } from "../fleet/board";
import { NEVER_TAKEN_UP_MS, assignedAt, elapsed, evidence, monitorFlags } from "../fleet/flags";
import { WORKER_SLOTS, workerPane, type ChainEvent, type PaneId, type TaskStatus } from "../fleet/types";
import type { TaskStoreInfo } from "../fleet/useFleet";
import { NewForm } from "./TaskForm";
import { TaskPage, type Mode } from "./TaskPage";
import { Chips, NEEDS_FLEET, Status, carriedFrom, clock, earlier, isOpen, runStarts, summary, useNow, type TaskOps } from "./taskKit";

export { summary, useNow, type TaskOps };

/// The list's tabs. `all` leaves out removed tasks, which show only under `removed`.
export type TaskFilter = "open" | "closed" | "all" | "removed";
const IN_TAB: Record<TaskFilter, (status: TaskStatus) => boolean> = {
  open: (s) => s === "planned" || s === "in-progress",
  closed: (s) => s === "done" || s === "dropped",
  all: (s) => s !== "removed",
  removed: (s) => s === "removed",
};
const TABS: [TaskFilter, string][] = [
  ["open", "Open"],
  ["closed", "Closed"],
  ["all", "All"],
  ["removed", "Removed"],
];

/// `any`, `unowned`, or a pane.
export type OwnerFilter = "any" | "unowned" | PaneId;
const OWNERS: [OwnerFilter, string][] = [
  ["any", "anyone"],
  ["unowned", "unowned"],
  ["orch", "orch"],
  ...WORKER_SLOTS.map(workerPane).map((p): [OwnerFilter, string] => [p, p]),
  ["operator", "operator"],
];

/// `this` keeps what has an entry from the live run.
export type RunFilter = "any" | "this";

const STATUSES: TaskStatus[] = ["planned", "in-progress", "done", "dropped"];

function counts(tasks: TaskRecord[], goal: boolean): string {
  const live = tasks.filter((t) => t.status !== "removed");
  const noun = goal ? "sub-task" : "task";
  const head = `${live.length} ${noun}${live.length === 1 ? "" : "s"}`;
  if (!goal || live.length === 0) return head;
  return [
    head,
    ...STATUSES.map((status) => [live.filter((t) => t.status === status).length, status] as const)
      .filter(([n]) => n > 0)
      .map(([n, status]) => `${n} ${status}`),
  ].join(" · ");
}

/// The row's second line: the latest entry, and an earlier release while it matters.
function Latest({ record }: { record: TaskRecord }) {
  const { chain } = record;
  const latest = chain[chain.length - 1];
  const release = [...chain].reverse().find((e) => e.entry.entry === "released");
  const clip = (text: string) => (text.length > 80 ? `${text.slice(0, 79)}…` : text);
  const quote = (e: ChainEvent) => (e.entry.entry === "released" ? `: "${clip(e.entry.why)}"` : "");
  const what = latest.entry.entry === "commented" ? `commented: "${clip(latest.entry.text)}"` : summary(latest) + quote(latest);
  return (
    <span className="task-row__latest">
      {latest.from} {what} · {clock(latest.at)}
      {release && release !== latest && isOpen(record) && (
        <>
          {"  ·  "}earlier, {release.from} released it{quote(release)}
        </>
      )}
    </span>
  );
}

function Signals({ record, board, now, store }: { record: TaskRecord; board: TaskRecord[]; now: number; store: TaskStoreInfo | null }) {
  const comments = record.chain.filter((e) => e.entry.entry === "commented").length;
  const { receipt, verdict } = evidence(record);
  const flagged = flaggers(record);
  const assigned = assignedAt(record, store?.lineage);
  const waited = assigned === null ? null : now - assigned;
  // The elapsed time on the row is how "never taken up" shows.
  const flags = monitorFlags(record, board, now, store?.lineage).filter((flag) => flag.kind !== "never-taken-up");
  return (
    <span className="task-row__signals">
      {comments > 0 && (
        <span className="task-row__sig">
          {comments} comment{comments === 1 ? "" : "s"}
        </span>
      )}
      {waited !== null && (
        <span className={`task-row__sig task-row__waiting ${waited >= NEVER_TAKEN_UP_MS ? "is-late" : ""}`}>
          assigned {elapsed(waited)} ago, not taken up
        </span>
      )}
      {receipt && <span className="task-row__sig task-row__receipt">{receipt.status}</span>}
      {!receipt && record.block.kind === "task" && record.status === "in-progress" && (
        <span className="task-row__sig">no receipt</span>
      )}
      {verdict && (
        <span className="task-row__sig">
          reviewed{verdict.requested ? "" : " (unrequested)"}: {verdict.met ? "met" : "not met"}
        </span>
      )}
      {flagged > 0 && <span className="task-row__sig task-row__flagged">flagged by {flagged}</span>}
      {flags.map((flag) => (
        <span key={flag.kind} className="task-flag">
          {flag.text}
        </span>
      ))}
    </span>
  );
}

type RowProps = {
  record: TaskRecord;
  store: TaskStoreInfo | null;
  board: TaskRecord[];
  now: number;
  starts: Map<string, number>;
  onOpen: (number: number) => void;
};

function Row({ record, store, board, now, starts, onOpen }: RowProps) {
  const carried = carriedFrom(record, store, starts);
  return (
    <button type="button" className="task-row" onClick={() => onOpen(record.number)}>
      <span className="mono task-row__number">#{record.number}</span>
      <span className="task-row__main">
        <span className="task-row__outcome">{record.block.outcome}</span>
        {carried && <span className="task-carried"> · {carried}</span>}
        <Latest record={record} />
      </span>
      <Status status={record.status} />
      <span className="task-row__owner">
        {record.owner ? <span className="mono">{record.owner.pane}</span> : <span className="task-owner--none">unowned</span>}
      </span>
      <Signals record={record} board={board} now={now} store={store} />
    </button>
  );
}

/// A goal as its group's header, still a way into its page.
function GoalHead({ record, tasks, ...rest }: RowProps & { tasks: TaskRecord[] }) {
  const { store, board, now, onOpen } = rest;
  return (
    <button type="button" className="task-row task-row--goal" onClick={() => onOpen(record.number)}>
      <span className="mono task-row__number">#{record.number}</span>
      <span className="task-row__goal">
        <strong>Goal: {record.block.outcome}</strong>
        <span className="task-row__counts">{counts(tasks, true)}</span>
        {record.status !== "planned" && <Status status={record.status} />}
      </span>
      <Signals record={record} board={board} now={now} store={store} />
    </button>
  );
}

export function TaskBoard({
  chain,
  store,
  ops = null,
  initialOpen = null,
  initialForm = null,
  initialEdit = false,
  initialRelease = false,
  initialFilter = "open",
  initialOwner = "any",
  initialRun = "any",
  initialMode = null,
  now: fixedNow,
}: {
  chain: ChainEvent[];
  store: TaskStoreInfo | null;
  ops?: TaskOps | null;
  /// What starts open; the render probe uses these.
  initialOpen?: number | null;
  initialForm?: "goal" | "task" | null;
  initialEdit?: boolean;
  initialRelease?: boolean;
  initialFilter?: TaskFilter;
  initialOwner?: OwnerFilter;
  initialRun?: RunFilter;
  initialMode?: "flag" | "remove" | null;
  /// The clock, fixed; the render probe passes one.
  now?: number;
}) {
  const now = useNow(fixedNow);
  // With no fleet running, no tasks are shown (D-117).
  const idle = !!store && !store.live;
  const events = useMemo(() => (idle ? [] : chain), [idle, chain]);
  const board = useMemo(() => replayBoard(events), [events]);
  const starts = useMemo(() => runStarts(events), [events]);
  const groups = useMemo(() => byGoal(board), [board]);
  const [open, setOpen] = useState<number | null>(initialOpen);
  const shown = board.find((r) => r.number === open);
  const [form, setForm] = useState<"goal" | "task" | null>(initialForm);
  const [filter, setFilter] = useState<TaskFilter>(initialFilter);
  const [owner, setOwner] = useState<OwnerFilter>(initialOwner);
  const thisRun = store?.live ? store.run : null;
  const [runFilter, setRunFilter] = useState<RunFilter>(initialRun);
  const byRun = runFilter === "this" && !!thisRun;

  const tasks = board.filter((r) => r.block.kind === "task");
  const tally = (tab: TaskFilter) => tasks.filter((t) => IN_TAB[tab](t.status)).length;
  const touched = (r: TaskRecord) => !byRun || r.chain.some((e) => e.run === thisRun);
  const matches = (r: TaskRecord) =>
    IN_TAB[filter](r.status) &&
    touched(r) &&
    (owner === "any" || (owner === "unowned" ? !r.owner : r.owner?.pane === owner));
  // A goal heads its group while any of its tasks is shown, and its counts
  // still cover all of them; a goal with no tasks stands on its own status.
  const visible = groups
    .map((group) => ({ ...group, shown: group.tasks.filter(matches) }))
    .filter(({ goal, tasks, shown }) =>
      shown.length > 0 ||
      (!!goal && owner === "any" && touched(goal) && IN_TAB[filter](goal.status) &&
        (filter === "removed" || tasks.every((t) => t.status === "removed"))),
    );
  // Writes go only through the hub, so with no fleet every control is off.
  const writable = !!store?.live && !!ops;
  const why = writable ? undefined : NEEDS_FLEET;
  const goals = board.filter((r) => r.block.kind === "goal");
  const carried = board.filter((r) => isOpen(r) && earlier(r, store)).length;
  const narrowed = owner !== "any" || byRun;
  const rowProps = { store, board, now, starts, onOpen: setOpen };

  if (form && ops && writable) {
    return (
      <div className="events-view tasks-view">
        <NewForm
          key={form}
          kind={form}
          goals={goals.filter((g) => g.status !== "removed")}
          ops={ops}
          onCancel={() => setForm(null)}
          onDone={(number) => {
            setForm(null);
            setOpen(number);
          }}
        />
      </div>
    );
  }

  if (shown) {
    return (
      <div className="events-view tasks-view">
        <TaskPage
          key={shown.number}
          record={shown}
          store={store}
          board={board}
          now={now}
          starts={starts}
          ops={ops}
          writable={writable}
          start={(initialEdit ? "edit" : initialRelease ? "release" : initialMode) as Mode}
          onNavigate={setOpen}
        />
      </div>
    );
  }

  return (
    <div className="events-view tasks-view">
      <div className="tasks__head">
        <h3>Tasks</h3>
        {board.length > 0 && (
          <span className="task-tabs" role="group" aria-label="filter tasks by status">
            {TABS.filter(([tab]) => tab !== "removed" || tally("removed") > 0).map(([tab, label]) => (
              <button
                key={tab}
                type="button"
                className={`task-tab ${filter === tab ? "is-on" : ""}`}
                aria-pressed={filter === tab}
                onClick={() => setFilter(tab)}
              >
                {`${label} ${tally(tab)}`}
              </button>
            ))}
          </span>
        )}
        <span className="grow" />
        <button type="button" className="task-btn" disabled={!writable} title={why} onClick={() => setForm("goal")}>
          New goal
        </button>
        <button type="button" className="task-btn task-btn--primary" disabled={!writable} title={why} onClick={() => setForm("task")}>
          New task
        </button>
      </div>
      {idle ? (
        <div className="tasks__empty">no fleet running · start one to see this repo's tasks</div>
      ) : board.length === 0 ? (
        <div className="tasks__empty">
          No goals or tasks for this repository yet. Once the vision is confirmed, the orchestrator records it with{" "}
          <span className="mono">fleet task post --goal</span> and cuts tasks under it.
        </div>
      ) : (
        <div className="tasks">
          <div className="tasks__filters">
            <span className="tasks__filter">
              <span className="tasks__filter-label">Owner</span>
              <Chips label="filter tasks by owner" options={OWNERS} value={owner} onChange={setOwner} dashed="unowned" />
            </span>
            {thisRun && (
              <span className="tasks__filter">
                <span className="tasks__filter-label">Run</span>
                <Chips
                  label="filter tasks by run"
                  options={[
                    ["any", "any run"],
                    ["this", "this run"],
                  ]}
                  value={runFilter}
                  onChange={setRunFilter}
                />
              </span>
            )}
          </div>
          {carried > 0 && (
            <div className="tasks__carried">
              <strong>Carried over</strong> {carried} open task{carried === 1 ? "" : "s"} owned by an earlier run.
              Their owners are gone, so they are available to take up.
            </div>
          )}
          {visible.length === 0 && (
            <div className="tasks__empty">No {narrowed ? "matching" : TABS.find(([t]) => t === filter)![1].toLowerCase()} tasks.</div>
          )}
          {visible.map(({ goal, tasks, shown: matching }) => (
            <section key={goal?.number ?? "none"} className="tasks__group">
              {goal ? (
                <GoalHead record={goal} tasks={tasks} {...rowProps} />
              ) : (
                <div className="tasks__no-goal">
                  <strong>No goal</strong>
                  <span className="task-row__counts">{counts(tasks, false)}</span>
                </div>
              )}
              {matching.map((task) => (
                <Row key={task.number} record={task} {...rowProps} />
              ))}
            </section>
          ))}
        </div>
      )}
    </div>
  );
}
