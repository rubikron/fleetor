// The Tasks view: goals, the tasks cut from them, and each one's chain (D-100).
//
// Folded from the task store's chain entries in `fleet/board.ts`. A status is
// what someone said, so nothing here renders a tick or a progress bar; a goal's
// tasks are counted by status as text.

import { Fragment, useMemo, useState } from "react";
import { byGoal, replayBoard, type TaskRecord } from "../fleet/board";
import type { ChainEvent, TaskStatus } from "../fleet/types";
import type { TaskStoreInfo } from "../fleet/useFleet";

const STATUSES: TaskStatus[] = ["planned", "in-progress", "done", "dropped"];

function Status({ status }: { status: TaskStatus }) {
  return <span className={`task__status task__status--${status}`}>{status}</span>;
}

const FIELD: Record<string, string> = {
  outcome: "the outcome",
  technical: "the technical criteria",
  vision: "the vision criteria",
};

/// One chain entry in a sentence, for a row's "latest" column.
function summary(event: ChainEvent): string {
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
  }
}

function counts(tasks: TaskRecord[]): string {
  if (tasks.length === 0) return "no tasks yet";
  return STATUSES.map((status) => [tasks.filter((t) => t.status === status).length, status] as const)
    .filter(([n]) => n > 0)
    .map(([n, status]) => `${n} ${status}`)
    .join(" · ");
}

function Row({
  record,
  tasks,
  open,
  onOpen,
}: {
  record: TaskRecord;
  tasks?: TaskRecord[];
  open: boolean;
  onOpen: (number: number) => void;
}) {
  const goal = record.block.kind === "goal";
  const latest = record.chain[record.chain.length - 1];
  const comments = record.chain.filter((e) => e.entry.entry === "commented").length;
  return (
    <button
      type="button"
      className={`task-row ${goal ? "task-row--goal" : ""} ${open ? "is-open" : ""}`}
      aria-pressed={open}
      onClick={() => onOpen(record.number)}
    >
      <span className="mono task-row__number">#{record.number}</span>
      <Status status={record.status} />
      {goal ? (
        <span className="task-row__kind">goal</span>
      ) : (
        <span className="mono task-row__owner">{record.owner?.pane ?? "unowned"}</span>
      )}
      <span className="task-row__outcome">{record.block.outcome}</span>
      {tasks && <span className="task-row__counts">{counts(tasks)}</span>}
      <span className="task-row__latest">
        <span className="mono">{latest.from}</span> {summary(latest)}
      </span>
      {comments > 0 && (
        <span className="task-row__comments">
          {comments} comment{comments === 1 ? "" : "s"}
        </span>
      )}
    </button>
  );
}

function Criteria({ label, items }: { label: string; items: string[] | undefined }) {
  if (!items || items.length === 0) return null;
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

function Entry({ event }: { event: ChainEvent }) {
  const { entry } = event;
  const note = entry.entry === "taken-up" || entry.entry === "status" ? entry.note : null;
  return (
    <li className={`chain__entry chain__entry--${entry.entry}`}>
      <span className="mono chain__from">{event.from}</span>
      <span className="chain__what">
        {entry.entry === "commented" ? "commented" : summary(event)}
      </span>
      <time className="chain__at">{new Date(event.at).toLocaleString()}</time>
      {entry.entry === "commented" && <p className="chain__text">{entry.text}</p>}
      {note && <p className="chain__text">{note}</p>}
      {entry.entry === "edited" && (
        <div className="chain__edit">
          <Criteria label="was" items={entry.old} />
          <Criteria label="now" items={entry.new} />
        </div>
      )}
    </li>
  );
}

function TaskPage({ record, goal }: { record: TaskRecord; goal: TaskRecord | undefined }) {
  const { block } = record;
  return (
    <article className="task-page">
      <header className="task__head">
        <span className="mono task__id">#{record.number}</span>
        <span className="task-row__kind">{block.kind}</span>
        <Status status={record.status} />
        {block.kind === "task" && (
          <span className="task__posted">
            owner <span className="mono">{record.owner?.pane ?? "unowned"}</span>
          </span>
        )}
        <span className="task__posted">
          opened by <span className="mono">{record.creator}</span>
        </span>
      </header>

      <p className="task__outcome">{block.outcome}</p>
      <Criteria label="technical" items={block.technical} />
      <Criteria label="vision" items={block.vision} />
      {block.instructions && <p className="task__instructions">{block.instructions}</p>}
      {goal && (
        <div className="task__links">
          <span className="task__link">
            serves goal <span className="mono">#{goal.number}</span> {goal.block.outcome}
          </span>
        </div>
      )}

      <ol className="chain">
        {record.chain.map((event, index) => {
          const previous = record.chain[index - 1];
          return (
            <Fragment key={event.seq}>
              {/* Run boundaries are not entries: they are drawn wherever
                  neighbouring entries carry different run ids. */}
              {previous && previous.run !== event.run && (
                <li className="chain__run">a later session</li>
              )}
              <Entry event={event} />
            </Fragment>
          );
        })}
      </ol>
    </article>
  );
}

export function TaskBoard({
  chain,
  store,
  initialOpen = null,
}: {
  chain: ChainEvent[];
  store: TaskStoreInfo | null;
  /// Which record's page starts open; the render probe uses it.
  initialOpen?: number | null;
}) {
  const board = useMemo(() => replayBoard(chain), [chain]);
  const groups = useMemo(() => byGoal(board), [board]);
  const [open, setOpen] = useState<number | null>(initialOpen);
  const shown = board.find((r) => r.number === open);
  const toggle = (number: number) => setOpen((now) => (now === number ? null : number));

  return (
    <div className="events-view">
      <div className="events-view__head">
        <h3>Tasks</h3>
        <span className="label">a status is what someone said, not a verdict</span>
        <span className="grow" style={{ flex: "1 1 auto" }} />
        {store && !store.live && (
          <span className="tasks__readonly">read-only · start a fleet to change tasks</span>
        )}
        {store && <span className="mono text-mute tasks__target">{store.target}</span>}
      </div>
      {board.length === 0 ? (
        <div className="feed feed--empty">
          No goals or tasks for this repository yet. Once the vision is confirmed, the orchestrator
          records it with <span className="mono">fleet task post --goal</span> and cuts tasks under
          it.
        </div>
      ) : (
        <div className="tasks">
          <div className="tasks__list">
            {groups.map(({ goal, tasks }) => (
              <section key={goal?.number ?? "none"} className="tasks__group">
                {goal ? (
                  <Row record={goal} tasks={tasks} open={open === goal.number} onOpen={toggle} />
                ) : (
                  <div className="tasks__no-goal">No goal</div>
                )}
                {tasks.map((task) => (
                  <Row key={task.number} record={task} open={open === task.number} onOpen={toggle} />
                ))}
              </section>
            ))}
          </div>
          {shown && (
            <TaskPage
              record={shown}
              goal={board.find((r) => r.number === shown.block.parent && r.block.kind === "goal")}
            />
          )}
        </div>
      )}
    </div>
  );
}
