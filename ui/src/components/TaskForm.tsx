// The new goal / new task screen: the same fields `fleet task post` takes.

import { useState } from "react";
import type { TaskRecord } from "../fleet/board";
import { WORKER_SLOTS, workerPane, type PaneId } from "../fleet/types";
import { Chips, Field, ListInput, filled, useChange, type TaskOps } from "./taskKit";

const WORKERS = WORKER_SLOTS.map(workerPane);

export function NewForm({
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
  const [technical, setTechnical] = useState<string[]>([]);
  const [vision, setVision] = useState<string[]>([]);
  const [instructions, setInstructions] = useState("");
  const [owner, setOwner] = useState<PaneId>("");
  const [reviewer, setReviewer] = useState<PaneId>("");
  const [parent, setParent] = useState<string>("");
  const [tell, setTell] = useState(false);
  const [error, busy, act] = useChange();

  const submit = () =>
    act(async () => {
      const number = await ops.run({
        action: "post",
        goal: kind === "goal",
        outcome,
        technical: filled(technical),
        vision: filled(vision),
        owner: owner || null,
        instructions: instructions.trim() || null,
        parent: parent ? Number(parent) : null,
        reviewer: reviewer || null,
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

  const chooseOwner = (pane: PaneId) => {
    setOwner(pane);
    if (!pane) setTell(false);
    if (pane && pane === reviewer) setReviewer("");
  };

  return (
    <div className="task-screen">
      <div className="tasks__head">
        <h3>New {kind}</h3>
      </div>
      <form
        className="task-form task-form--new"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <Field label="Outcome" hint="What this makes possible">
          <input className="task-input" value={outcome} onChange={(e) => setOutcome(e.target.value)} />
        </Field>
        {kind === "task" && (
          <Field label="Technical checks" hint="Something anyone could run">
            <ListInput items={technical} onChange={setTechnical} add="Add a check" placeholder="Another check…" />
          </Field>
        )}
        <Field label="Vision" hint={kind === "task" ? "Which part of the goal it serves" : "What done looks like"}>
          <ListInput items={vision} onChange={setVision} add="Add another" placeholder="Another…" />
        </Field>
        {kind === "task" && (
          <>
            <Field label="Instructions" hint="Optional">
              <textarea
                className="task-input"
                rows={2}
                placeholder="Context a cold worker needs…"
                value={instructions}
                onChange={(e) => setInstructions(e.target.value)}
              />
            </Field>
            <Field label="Goal">
              <select className="task-input" value={parent} onChange={(e) => setParent(e.target.value)}>
                <option value="">No goal</option>
                {goals.map((goal) => (
                  <option key={goal.number} value={goal.number}>
                    #{goal.number} {goal.block.outcome}
                  </option>
                ))}
              </select>
            </Field>
            <Field label="Owner">
              <Chips
                label="owner"
                options={[["", "unowned"], ...WORKERS.map((p): [PaneId, string] => [p, p]), ["orch", "orch"]]}
                value={owner}
                onChange={chooseOwner}
                dashed=""
              />
            </Field>
            <Field label="Reviewer" hint="Optional">
              <Chips
                label="reviewer"
                options={[["", "none"], ...WORKERS.filter((p) => p !== owner).map((p): [PaneId, string] => [p, p])]}
                value={reviewer}
                onChange={setReviewer}
                dashed=""
              />
            </Field>
            <span className="task-form__k" />
            <div className="task-form__v task-form__stack">
              <label className="task-form__check">
                <input type="checkbox" checked={tell} disabled={!owner} onChange={(e) => setTell(e.target.checked)} />
                Message the owner with this task
              </label>
              <span className="task-form__note">
                Off while the task is unowned. Nothing else tells an agent this task exists.
              </span>
            </div>
          </>
        )}
        <span className="task-form__k" />
        <div className="task-form__v task-form__actions">
          <button type="submit" className="task-btn task-btn--primary" disabled={busy}>
            Open {kind}
          </button>
          <button type="button" className="task-btn" onClick={onCancel}>
            Cancel
          </button>
        </div>
        {error && <p className="task-form__error">{error}</p>}
      </form>
    </div>
  );
}
