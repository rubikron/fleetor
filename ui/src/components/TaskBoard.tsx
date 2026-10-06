// The Tasks view: goals, the tasks cut from them, and each one's chain (D-100).
//
// Folded from the task store's chain entries in `fleet/board.ts`. A status is
// what someone said, so nothing here renders a tick or a progress bar; a goal's
// tasks are counted by status as text.

import { Fragment, useMemo, useState } from "react";
import { byGoal, replayBoard, type TaskRecord } from "../fleet/board";
import {
  WORKER_SLOTS,
  workerPane,
  type ChainEvent,
  type PaneId,
  type TaskAction,
  type TaskStatus,
} from "../fleet/types";
import type { TaskStoreInfo } from "../fleet/useFleet";

/// How the operator's controls reach the hub. Passed in rather than imported,
/// so the view renders with no backend behind it.
export interface TaskOps {
  /// One task change as `operator`; resolves to the task's number, rejects
  /// with the hub's refusal.
  run: (action: TaskAction) => Promise<string>;
  message: (to: PaneId, text: string) => Promise<unknown>;
}

const NEEDS_FLEET = "start a fleet to change tasks";

/// What the list can be narrowed to. `all` includes dropped tasks.
export type TaskFilter = "all" | "planned" | "in-progress" | "done";
const FILTERS: [TaskFilter, string][] = [
  ["all", "All"],
  ["planned", "Planned"],
  ["in-progress", "In progress"],
  ["done", "Done"],
];

/// An owner from another lineage is a pane that no longer exists. With no
/// fleet running, that is every owner.
const earlier = (record: TaskRecord, store: TaskStoreInfo | null): boolean =>
  !!record.owner && !!store && record.owner.lineage !== store.lineage;

const ownerName = (record: TaskRecord, store: TaskStoreInfo | null): string =>
  !record.owner ? "unowned" : earlier(record, store) ? `${record.owner.pane}, earlier run` : record.owner.pane;

const isOpen = (record: TaskRecord): boolean =>
  record.status === "planned" || record.status === "in-progress";

const lines = (text: string): string[] =>
  text
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0);

/// Run one change, holding the hub's refusal for display.
function useChange(): [string | null, boolean, (change: () => Promise<void>) => void] {
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

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="task-form__field">
      <span className="composer__label">{label}</span>
      {children}
    </label>
  );
}

/// The new goal / new task form: the same fields `fleet task post` takes.
function NewForm({
  kind,
  goals,
  ops,
  onDone,
  onCancel,
}: {
  kind: "goal" | "task";
  goals: TaskRecord[];
  ops: TaskOps;
  onDone: (number: number) => void;
  onCancel: () => void;
}) {
  const [outcome, setOutcome] = useState("");
  const [technical, setTechnical] = useState("");
  const [vision, setVision] = useState("");
  const [instructions, setInstructions] = useState("");
  const [owner, setOwner] = useState<PaneId | "">("");
  const [parent, setParent] = useState<string>("");
  const [tell, setTell] = useState(false);
  const [error, busy, act] = useChange();

  const submit = () =>
    act(async () => {
      const number = await ops.run({
        action: "post",
        goal: kind === "goal",
        outcome,
        technical: lines(technical),
        vision: lines(vision),
        owner: owner || null,
        instructions: instructions.trim() || null,
        parent: parent ? Number(parent) : null,
      });
      // Opening a task assigns nobody; telling the owner is a separate, chosen act.
      if (kind === "task" && owner && tell) {
        await ops.message(
          owner,
          `Task #${number} is yours: ${outcome.trim()}. Run \`fleet task show ${number}\` for its criteria.`,
        );
      }
      onDone(Number(number));
    });

  return (
    <form
      className="task-form"
      onSubmit={(e) => {
        e.preventDefault();
        submit();
      }}
    >
      <h4 className="task-form__title">New {kind}</h4>
      <Field label="outcome">
        <input className="composer__input" value={outcome} onChange={(e) => setOutcome(e.target.value)} />
      </Field>
      {kind === "task" && (
        <Field label="technical criteria, one per line">
          <textarea className="composer__input" rows={2} value={technical} onChange={(e) => setTechnical(e.target.value)} />
        </Field>
      )}
      <Field label="vision criteria, one per line">
        <textarea className="composer__input" rows={2} value={vision} onChange={(e) => setVision(e.target.value)} />
      </Field>
      {kind === "task" && (
        <>
          <Field label="instructions (optional)">
            <textarea className="composer__input" rows={2} value={instructions} onChange={(e) => setInstructions(e.target.value)} />
          </Field>
          <Field label="goal">
            <select className="composer__target" value={parent} onChange={(e) => setParent(e.target.value)}>
              <option value="">No goal</option>
              {goals.map((goal) => (
                <option key={goal.number} value={goal.number}>
                  #{goal.number} {goal.block.outcome}
                </option>
              ))}
            </select>
          </Field>
          <Field label="owner">
            <select className="composer__target" value={owner} onChange={(e) => setOwner(e.target.value as PaneId | "")}>
              <option value="">unowned</option>
              {WORKER_SLOTS.map(workerPane).map((pane) => (
                <option key={pane} value={pane}>
                  {pane}
                </option>
              ))}
            </select>
          </Field>
          <label className="task-form__check">
            <input type="checkbox" checked={tell} disabled={!owner} onChange={(e) => setTell(e.target.checked)} />
            message the owner with this task
          </label>
        </>
      )}
      <div className="task-form__actions">
        <button type="submit" className="composer__send" disabled={busy}>
          Open {kind}
        </button>
        <button type="button" className="task-form__quiet" onClick={onCancel}>
          Cancel
        </button>
      </div>
      {error && <p className="composer__result composer__result--bad">{error}</p>}
    </form>
  );
}

/// Which form the controls show in place of their buttons.
type Mode = "edit" | "release" | null;

/// The release form: the same four fields `fleet task release` requires. The
/// operator stands in no checkout, so "where" is typed.
function ReleaseForm({
  record,
  off,
  why: title,
  onRelease,
  onCancel,
}: {
  record: TaskRecord;
  off: boolean;
  why: string | undefined;
  onRelease: (action: TaskAction) => void;
  onCancel: () => void;
}) {
  const [why, setWhy] = useState("");
  const [done, setDone] = useState("");
  const [left, setLeft] = useState("");
  const [place, setPlace] = useState("");
  const missing = [why, done, left, place].some((text) => text.trim() === "");
  return (
    <form
      className="task-form task-form--release"
      onSubmit={(e) => {
        e.preventDefault();
        onRelease({ action: "release", task: record.number, why, done, left, place });
      }}
    >
      <h4 className="task-form__title">Release #{record.number}</h4>
      <Field label="why it is being released">
        <input className="composer__input" value={why} onChange={(e) => setWhy(e.target.value)} />
      </Field>
      <Field label="done so far">
        <textarea className="composer__input" rows={2} value={done} onChange={(e) => setDone(e.target.value)} />
      </Field>
      <Field label="left to do">
        <textarea className="composer__input" rows={2} value={left} onChange={(e) => setLeft(e.target.value)} />
      </Field>
      <Field label="where the work sits: branch @ commit">
        <input className="composer__input" value={place} onChange={(e) => setPlace(e.target.value)} />
      </Field>
      <div className="task-form__actions">
        <button
          type="submit"
          className="composer__send"
          disabled={off || missing}
          title={title ?? (missing ? "all four fields are required" : undefined)}
        >
          Release
        </button>
        <button type="button" className="task-form__quiet" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </form>
  );
}

/// The operator's controls on one record: comment, edit, release, close and
/// reopen.
function Controls({
  record,
  ops,
  writable,
  start,
}: {
  record: TaskRecord;
  ops: TaskOps | null;
  writable: boolean;
  start: Mode;
}) {
  const { block } = record;
  const [comment, setComment] = useState("");
  const [mode, setMode] = useState<Mode>(start);
  const editing = mode === "edit";
  const setEditing = (on: boolean) => setMode(on ? "edit" : null);
  const [outcome, setOutcome] = useState(block.outcome);
  const [technical, setTechnical] = useState((block.technical ?? []).join("\n"));
  const [vision, setVision] = useState(block.vision.join("\n"));
  const [error, busy, act] = useChange();
  const off = !writable || busy;
  const why = writable ? undefined : NEEDS_FLEET;
  const run = (action: TaskAction, then?: () => void) =>
    act(async () => {
      if (!ops) return;
      await ops.run(action);
      then?.();
    });
  const same = (a: string[], b: string[]) => a.length === b.length && a.every((x, i) => x === b[i]);

  // Each field is sent whole, and only when it differs; the hub refuses an
  // edit that changes nothing.
  const saveEdit = () =>
    run(
      {
        action: "edit",
        task: record.number,
        outcome: outcome.trim() === block.outcome ? null : outcome,
        technical: same(lines(technical), block.technical ?? []) ? [] : lines(technical),
        vision: same(lines(vision), block.vision) ? [] : lines(vision),
      },
      () => setEditing(false),
    );

  return (
    <div className="task-controls">
      {editing ? (
        <form
          className="task-form"
          onSubmit={(e) => {
            e.preventDefault();
            saveEdit();
          }}
        >
          <h4 className="task-form__title">Edit #{record.number}</h4>
          <Field label="outcome">
            <input className="composer__input" value={outcome} onChange={(e) => setOutcome(e.target.value)} />
          </Field>
          {block.kind === "task" && (
            <Field label="technical criteria, one per line">
              <textarea className="composer__input" rows={2} value={technical} onChange={(e) => setTechnical(e.target.value)} />
            </Field>
          )}
          <Field label="vision criteria, one per line">
            <textarea className="composer__input" rows={2} value={vision} onChange={(e) => setVision(e.target.value)} />
          </Field>
          <div className="task-form__actions">
            <button type="submit" className="composer__send" disabled={off} title={why}>
              Save edit
            </button>
            <button type="button" className="task-form__quiet" onClick={() => setEditing(false)}>
              Cancel
            </button>
          </div>
        </form>
      ) : mode === "release" ? (
        <ReleaseForm
          record={record}
          off={off}
          why={why}
          onRelease={(action) => run(action, () => setMode(null))}
          onCancel={() => setMode(null)}
        />
      ) : (
        <div className="task-form__actions">
          <button type="button" className="task-form__quiet" disabled={off} title={why} onClick={() => setEditing(true)}>
            Edit
          </button>
          {block.kind === "task" && record.owner && isOpen(record) && (
            <button type="button" className="task-form__quiet" disabled={off} title={why} onClick={() => setMode("release")}>
              Release
            </button>
          )}
          {record.status !== "dropped" && (
            <button
              type="button"
              className="task-form__quiet"
              disabled={off}
              title={why}
              onClick={() => run({ action: "update", task: record.number, status: "dropped" })}
            >
              Close
            </button>
          )}
          {(record.status === "dropped" || record.status === "done") && (
            <button
              type="button"
              className="task-form__quiet"
              disabled={off}
              title={why}
              onClick={() => run({ action: "update", task: record.number, status: "planned" })}
            >
              Reopen
            </button>
          )}
        </div>
      )}
      <form
        className="task-form__comment"
        onSubmit={(e) => {
          e.preventDefault();
          run({ action: "comment", task: record.number, text: comment }, () => setComment(""));
        }}
      >
        <input
          className="composer__input"
          placeholder={writable ? "comment as operator" : NEEDS_FLEET}
          value={comment}
          disabled={!writable}
          onChange={(e) => setComment(e.target.value)}
        />
        <button type="submit" className="composer__send" disabled={off || comment.trim() === ""} title={why}>
          Comment
        </button>
      </form>
      {error && <p className="composer__result composer__result--bad">{error}</p>}
    </div>
  );
}

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
    case "receipt":
      return `ran a check · ${entry.status}`;
    case "released":
      return entry.on_behalf_of ? `released this on behalf of ${entry.on_behalf_of}` : "released this";
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
  store,
  tasks,
  open,
  onOpen,
}: {
  record: TaskRecord;
  store: TaskStoreInfo | null;
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
        <span className="mono task-row__owner">{ownerName(record, store)}</span>
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
      {entry.entry === "receipt" && (
        <dl className="chain__release">
          <dt>check</dt>
          <dd className="mono">{entry.check}</dd>
          <dt>result</dt>
          <dd className="mono">{entry.status}</dd>
          <dt>where</dt>
          <dd className="mono">
            {entry.branch ?? "no branch"} @ {entry.commit ?? "no commit"}
            {entry.uncommitted && " + uncommitted changes"}
          </dd>
          {!entry.accepted && (
            <>
              <dt>message</dt>
              <dd>the receipt message was not delivered to orch</dd>
            </>
          )}
        </dl>
      )}
      {entry.entry === "released" && (
        <dl className="chain__release">
          <dt>why</dt>
          <dd>{entry.why}</dd>
          <dt>done</dt>
          <dd>{entry.done}</dd>
          <dt>left</dt>
          <dd>{entry.left}</dd>
          <dt>where</dt>
          <dd className="mono">{entry.where}</dd>
        </dl>
      )}
    </li>
  );
}

function TaskPage({
  record,
  store,
  goal,
  ops,
  writable,
  start,
}: {
  record: TaskRecord;
  store: TaskStoreInfo | null;
  goal: TaskRecord | undefined;
  ops: TaskOps | null;
  writable: boolean;
  start: Mode;
}) {
  const { block } = record;
  return (
    <article className="task-page">
      <header className="task__head">
        <span className="mono task__id">#{record.number}</span>
        <span className="task-row__kind">{block.kind}</span>
        <Status status={record.status} />
        {block.kind === "task" && (
          <span className="task__posted">
            owner <span className="mono">{ownerName(record, store)}</span>
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
      <Controls record={record} ops={ops} writable={writable} start={start} />
    </article>
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
  initialFilter = "all",
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
}) {
  const board = useMemo(() => replayBoard(chain), [chain]);
  const groups = useMemo(() => byGoal(board), [board]);
  const [open, setOpen] = useState<number | null>(initialOpen);
  const shown = board.find((r) => r.number === open);
  const toggle = (number: number) => setOpen((now) => (now === number ? null : number));
  const [form, setForm] = useState<"goal" | "task" | null>(initialForm);
  const [filter, setFilter] = useState<TaskFilter>(initialFilter);
  // A filter narrows the tasks; a goal stays while any of its tasks match, and
  // its counts still describe all of them.
  const visible = groups
    .map((group) => ({
      ...group,
      shown: group.tasks.filter((task) => filter === "all" || task.status === filter),
    }))
    .filter((group) => filter === "all" || group.shown.length > 0);
  // Writes go only through the hub, so with no fleet every control is off.
  const writable = !!store?.live && !!ops;
  const why = writable ? undefined : NEEDS_FLEET;
  const goals = board.filter((r) => r.block.kind === "goal");
  const carried = board.filter((r) => isOpen(r) && earlier(r, store)).length;

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
        <button type="button" className="task-form__quiet" disabled={!writable} title={why} onClick={() => setForm("goal")}>
          New goal
        </button>
        <button type="button" className="task-form__quiet" disabled={!writable} title={why} onClick={() => setForm("task")}>
          New task
        </button>
      </div>
      {carried > 0 && (
        <div className="tasks__carried">
          <strong>Carried over</strong> {carried} open task{carried === 1 ? "" : "s"} owned by an
          earlier run. Their owners are gone, so they are available to take up.
        </div>
      )}
      {form && ops && writable && (
        <NewForm
          key={form}
          kind={form}
          goals={goals}
          ops={ops}
          onCancel={() => setForm(null)}
          onDone={(number) => {
            setForm(null);
            setOpen(number);
          }}
        />
      )}
      {board.length === 0 ? (
        <div className="feed feed--empty">
          No goals or tasks for this repository yet. Once the vision is confirmed, the orchestrator
          records it with <span className="mono">fleet task post --goal</span> and cuts tasks under
          it.
        </div>
      ) : (
        <div className="tasks">
          <div className="tasks__list">
            <div className="tasks__filters" role="group" aria-label="filter tasks by status">
              {FILTERS.map(([value, label]) => (
                <button
                  key={value}
                  type="button"
                  className={`task-form__quiet ${filter === value ? "is-on" : ""}`}
                  aria-pressed={filter === value}
                  onClick={() => setFilter(value)}
                >
                  {label}
                </button>
              ))}
            </div>
            {visible.length === 0 && <div className="feed--empty">No {filter} tasks.</div>}
            {visible.map(({ goal, tasks, shown: matching }) => (
              <section key={goal?.number ?? "none"} className="tasks__group">
                {goal ? (
                  <Row record={goal} store={store} tasks={tasks} open={open === goal.number} onOpen={toggle} />
                ) : (
                  <div className="tasks__no-goal">No goal</div>
                )}
                {matching.map((task) => (
                  <Row key={task.number} record={task} store={store} open={open === task.number} onOpen={toggle} />
                ))}
              </section>
            ))}
          </div>
          {shown && (
            <TaskPage
              key={shown.number}
              record={shown}
              store={store}
              ops={ops}
              writable={writable}
              start={initialEdit ? "edit" : initialRelease ? "release" : null}
              goal={board.find((r) => r.number === shown.block.parent && r.block.kind === "goal")}
            />
          )}
        </div>
      )}
    </div>
  );
}
