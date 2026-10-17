// One goal or task as a full page: criteria, its whole chain, the facts beside
// it, and the operator's controls.

import { Fragment, useState, type ReactNode } from "react";
import { byGoal, flaggers, replayBoard, type TaskRecord } from "../fleet/board";
import { monitorFlags } from "../fleet/flags";
import { WORKER_SLOTS, workerPane, type ChainEntry, type ChainEvent, type PaneId, type TaskAction } from "../fleet/types";
import type { TaskStoreInfo } from "../fleet/useFleet";
import {
  Field,
  ListInput,
  NEEDS_FLEET,
  Status,
  carriedFrom,
  clock,
  dayClock,
  earlier,
  filled,
  isOpen,
  runName,
  summary,
  useChange,
  type TaskOps,
} from "./taskKit";

/// Which form shows under the title in place of the actions.
export type Mode = "edit" | "release" | "flag" | "remove" | null;

const REASON = {
  flag: { title: "Flag", hint: "why it is not worth doing", send: "Flag" },
  remove: { title: "Remove", hint: "why it is destructive or counterproductive", send: "Remove" },
} as const;

const same = (a: string[], b: string[]) => a.length === b.length && a.every((x, i) => x === b[i]);

/// Releasing or removing only writes the record; telling the owner is a
/// separate, chosen act.
function TellOwner({ owner, tell, setTell }: { owner: PaneId; tell: boolean; setTell: (on: boolean) => void }) {
  return (
    <label className="task-form__check">
      <input type="checkbox" checked={tell} onChange={(e) => setTell(e.target.checked)} />
      message {owner} to stop work on it
    </label>
  );
}

function FormActions({ send, off, title, onCancel }: { send: string; off: boolean; title?: string; onCancel: () => void }) {
  return (
    <>
      <span className="task-form__k" />
      <div className="task-form__v task-form__actions">
        <button type="submit" className="task-btn task-btn--primary" disabled={off} title={title}>
          {send}
        </button>
        <button type="button" className="task-btn" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </>
  );
}

/// Flag and remove both take one required reason.
function ReasonForm({
  kind,
  record,
  owner,
  off,
  why: title,
  onSend,
  onCancel,
}: {
  kind: "flag" | "remove";
  record: TaskRecord;
  owner: PaneId | null;
  off: boolean;
  why: string | undefined;
  onSend: (action: TaskAction, tell: boolean) => void;
  onCancel: () => void;
}) {
  const [reason, setReason] = useState("");
  const [tell, setTell] = useState(false);
  const missing = reason.trim() === "";
  return (
    <form
      className="task-form task-form--reason"
      onSubmit={(e) => {
        e.preventDefault();
        onSend({ action: kind, task: record.number, reason }, tell);
      }}
    >
      <h4 className="task-form__title">
        {REASON[kind].title} #{record.number}
      </h4>
      <Field label="Reason" hint={REASON[kind].hint}>
        <input className="task-input" value={reason} onChange={(e) => setReason(e.target.value)} />
      </Field>
      {kind === "remove" && owner && (
        <>
          <span className="task-form__k" />
          <div className="task-form__v">
            <TellOwner owner={owner} tell={tell} setTell={setTell} />
          </div>
        </>
      )}
      <FormActions
        send={REASON[kind].send}
        off={off || missing}
        title={title ?? (missing ? "a reason is required" : undefined)}
        onCancel={onCancel}
      />
    </form>
  );
}

/// The four fields `fleet task release` requires (D-105). The operator stands
/// in no checkout, so "where" is typed.
function ReleaseForm({
  record,
  owner,
  off,
  why: title,
  onRelease,
  onCancel,
}: {
  record: TaskRecord;
  owner: PaneId | null;
  off: boolean;
  why: string | undefined;
  onRelease: (action: TaskAction, tell: boolean) => void;
  onCancel: () => void;
}) {
  const [tell, setTell] = useState(false);
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
        onRelease({ action: "release", task: record.number, why, done, left, place }, tell);
      }}
    >
      <h4 className="task-form__title">Release #{record.number}</h4>
      <Field label="Why" hint="why it is being released">
        <input className="task-input" value={why} onChange={(e) => setWhy(e.target.value)} />
      </Field>
      <Field label="Done" hint="done so far">
        <textarea className="task-input" rows={2} value={done} onChange={(e) => setDone(e.target.value)} />
      </Field>
      <Field label="Left" hint="left to do">
        <textarea className="task-input" rows={2} value={left} onChange={(e) => setLeft(e.target.value)} />
      </Field>
      <Field label="Where" hint="branch @ commit">
        <input className="task-input mono" value={place} onChange={(e) => setPlace(e.target.value)} />
      </Field>
      {owner && (
        <>
          <span className="task-form__k" />
          <div className="task-form__v">
            <TellOwner owner={owner} tell={tell} setTell={setTell} />
          </div>
        </>
      )}
      <FormActions
        send="Release"
        off={off || missing}
        title={title ?? (missing ? "all four fields are required" : undefined)}
        onCancel={onCancel}
      />
    </form>
  );
}

function EditForm({
  record,
  off,
  why,
  onSave,
  onCancel,
}: {
  record: TaskRecord;
  off: boolean;
  why: string | undefined;
  onSave: (action: TaskAction) => void;
  onCancel: () => void;
}) {
  const { block } = record;
  const [outcome, setOutcome] = useState(block.outcome);
  const [technical, setTechnical] = useState(block.technical ?? []);
  const [vision, setVision] = useState(block.vision);
  // Each field is sent whole, and only when it differs; the hub refuses an
  // edit that changes nothing.
  const save = () =>
    onSave({
      action: "edit",
      task: record.number,
      outcome: outcome.trim() === block.outcome ? null : outcome,
      technical: same(filled(technical), block.technical ?? []) ? [] : filled(technical),
      vision: same(filled(vision), block.vision) ? [] : filled(vision),
    });
  return (
    <form
      className="task-form task-form--edit"
      onSubmit={(e) => {
        e.preventDefault();
        save();
      }}
    >
      <h4 className="task-form__title">Edit #{record.number}</h4>
      <Field label="Outcome">
        <input className="task-input" value={outcome} onChange={(e) => setOutcome(e.target.value)} />
      </Field>
      {block.kind === "task" && (
        <Field label="Technical checks">
          <ListInput items={technical} onChange={setTechnical} add="Add a check" placeholder="Another check…" />
        </Field>
      )}
      <Field label="Vision">
        <ListInput items={vision} onChange={setVision} add="Add another" placeholder="Another…" />
      </Field>
      <FormActions send="Save edit" off={off} title={why} onCancel={onCancel} />
    </form>
  );
}

/// The entries that set a status, for "planned by orch · 19:58".
const SETS_STATUS = new Set(["opened", "taken-up", "status", "released", "removed", "restored", "handoff"]);

const last = <T,>(items: T[], match: (item: T) => boolean): T | undefined => {
  for (let i = items.length - 1; i >= 0; i--) if (match(items[i])) return items[i];
  return undefined;
};

type Receipt = Extract<ChainEntry, { entry: "receipt" }>;
const place = (r: Receipt) => `${r.branch ?? "no branch"} @ ${r.commit ?? "no commit"}${r.uncommitted ? " + uncommitted changes" : ""}`;

function Fact({ k, children }: { k: string; children: ReactNode }) {
  return (
    <div className="task-fact">
      <span className="task-fact__k">{k}</span>
      <div className="task-fact__v">{children}</div>
    </div>
  );
}

function Criteria({ record }: { record: TaskRecord }) {
  const { block, chain } = record;
  const task = block.kind === "task";
  const added = (item: string) => {
    const edit = last(chain, (e) => e.entry.entry === "edited" && e.entry.field === "technical" && e.entry.new.includes(item) && !e.entry.old.includes(item));
    return edit ? `added ${clock(edit.at)}` : "";
  };
  const receipts = chain.filter((e) => e.entry.entry === "receipt");
  const verdict = last(chain, (e) => e.entry.entry === "reviewed");
  const row = (kind: string, value: ReactNode, side: string, key: string) => (
    <div key={key} className="task-crit">
      <span className="task-crit__kind">{kind}</span>
      <span className="task-crit__v">{value}</span>
      <span className="task-crit__s">{side}</span>
    </div>
  );
  const rows = [
    ...(block.technical ?? []).map((item, i) => row("technical", <span className="mono">{item}</span>, added(item), `t${i}`)),
    // Every receipt, none matched to one criterion (D-108).
    ...(task
      ? receipts.length > 0
        ? receipts.map((e) => {
            const r = e.entry as Receipt;
            return row("evidence", <span className="mono">{`${r.check} — ${r.status} @ ${r.commit ?? "no commit"}${r.uncommitted ? " + uncommitted changes" : ""}`}</span>, `${e.from} · ${clock(e.at)}`, `e${e.seq}`);
          })
        : [row("evidence", <span className="task-crit__none">no receipt recorded</span>, "", "e")]
      : []),
    ...block.vision.map((item, i) => row("vision", item, "", `v${i}`)),
    ...(task
      ? [
          verdict && verdict.entry.entry === "reviewed"
            ? row(
                "review",
                `${verdict.entry.met ? "met" : "not met"}${verdict.entry.reason ? ` — ${verdict.entry.reason}` : ""}`,
                `${verdict.from}${verdict.entry.requested ? "" : " (unrequested)"} · ${clock(verdict.at)}`,
                "r",
              )
            : row("review", <span className="task-crit__none">no review yet</span>, "", "r"),
        ]
      : []),
  ];
  return <div className="task-crits">{rows}</div>;
}

function EditDiff({ entry }: { entry: Extract<ChainEntry, { entry: "edited" }> }) {
  const swap = (old: string, now: string) => (
    <div>
      <del>{old}</del> → <ins>{now}</ins>
    </div>
  );
  if (entry.field === "outcome") return <div className="chain__edit">{swap(entry.old.join("\n"), entry.new.join("\n"))}</div>;
  const gone = entry.old.filter((x) => !entry.new.includes(x));
  const added = entry.new.filter((x) => !entry.old.includes(x));
  return (
    <div className="chain__edit">
      {gone.length === 1 && added.length === 1 ? (
        swap(gone[0], added[0])
      ) : gone.length + added.length === 0 ? (
        swap(entry.old.join(" · "), entry.new.join(" · "))
      ) : (
        <>
          {added.map((item) => (
            <div key={`+${item}`}>
              added <ins>{item}</ins>
            </div>
          ))}
          {gone.map((item) => (
            <div key={`-${item}`}>
              removed <del>{item}</del>
            </div>
          ))}
        </>
      )}
    </div>
  );
}

const BOXED = new Set(["commented", "receipt", "released", "handoff"]);

function Entry({ event, record }: { event: ChainEvent; record: TaskRecord }) {
  const { entry } = event;
  const who = <span className="mono chain__from">{event.from}</span>;
  const at = <time className="chain__at">{clock(event.at)}</time>;
  const note =
    entry.entry === "taken-up" || entry.entry === "status"
      ? entry.note
      : entry.entry === "reviewed" || entry.entry === "flagged" || entry.entry === "removed" || entry.entry === "restored"
        ? entry.reason
        : null;
  const opener = entry.entry === "opened" && record.block.kind === "task" ? entry.block.owner : null;
  let head: ReactNode;
  if (entry.entry === "receipt") {
    const ok = entry.status === "exit 0";
    head = (
      <>
        {who} <span className="chain__what">sent a receipt</span>{" "}
        <span className={`task-exit ${ok ? "task-exit--ok" : "task-exit--bad"}`}>{entry.status}</span> {at}
        <span className="chain__where">
          {" · "}
          <span className="mono">{place(entry)}</span>
        </span>
      </>
    );
  } else {
    head = (
      <>
        {who}{" "}
        <span className="chain__what">
          {entry.entry === "commented" ? "commented" : summary(event)}
          {opener && (
            <>
              {" "}
              and assigned <span className="mono">{opener}</span>
            </>
          )}
        </span>{" "}
        {at}
      </>
    );
  }
  return (
    <li className={`chain__entry chain__entry--${entry.entry}${BOXED.has(entry.entry) ? " chain__entry--box" : ""}`}>
      <div className="chain__head">{head}</div>
      {entry.entry === "commented" && <p className="chain__text">{entry.text}</p>}
      {note && <p className="chain__text">{note}</p>}
      {entry.entry === "edited" && <EditDiff entry={entry} />}
      {entry.entry === "receipt" &&
        (entry.tail ? (
          <pre className="chain__out">{`$ ${entry.check}\n${entry.tail.replace(/\n+$/, "")}`}</pre>
        ) : (
          <code className="chain__check">$ {entry.check}</code>
        ))}
      {entry.entry === "receipt" && !entry.accepted && (
        <p className="chain__text chain__text--mute">the receipt message was not delivered to orch</p>
      )}
      {entry.entry === "handoff" && (
        <dl className="chain__release">
          <dt>built</dt>
          <dd>{entry.built}</dd>
          <dt>evidence</dt>
          <dd>{entry.evidence.join(" · ")}</dd>
          {entry.open && entry.open.length > 0 && (
            <>
              <dt>open</dt>
              <dd>{entry.open.join(" · ")}</dd>
            </>
          )}
          {entry.open_tasks && entry.open_tasks.length > 0 && (
            <>
              <dt>tasks still open</dt>
              <dd className="mono">{entry.open_tasks.map((n) => `#${n}`).join(" ")}</dd>
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

/// Run boundaries are not entries: drawn wherever neighbouring entries carry
/// different run ids, with the status the task had when the earlier run ended.
function Boundary({ record, index, starts }: { record: TaskRecord; index: number; starts: Map<string, number> }) {
  const before = replayBoard(record.chain.slice(0, index))[0];
  const state = !before ? "" : isOpen(before) ? " with this open" : ` with this ${before.status}`;
  return (
    <li className="chain__run">
      {runName(record.chain[index - 1].run, starts)} ended{state} · {runName(record.chain[index].run, starts)} started
    </li>
  );
}

export function TaskPage({
  record,
  store,
  board,
  now,
  starts,
  ops,
  writable,
  start,
  onNavigate,
}: {
  record: TaskRecord;
  store: TaskStoreInfo | null;
  board: TaskRecord[];
  now: number;
  starts: Map<string, number>;
  ops: TaskOps | null;
  writable: boolean;
  start: Mode;
  onNavigate: (number: number | null) => void;
}) {
  const { block, chain } = record;
  const isTask = block.kind === "task";
  const goals = board.filter((r) => r.block.kind === "goal" && r.status !== "removed");
  const goal = isTask ? board.find((r) => r.number === block.parent && r.block.kind === "goal") : undefined;
  const carried = carriedFrom(record, store, starts);
  const owner = record.owner && !earlier(record, store) ? record.owner.pane : null;
  const flagged = flaggers(record);

  const [mode, setMode] = useState<Mode>(start);
  const [comment, setComment] = useState("");
  const [error, busy, act] = useChange();
  const off = !writable || busy;
  const why = writable ? undefined : NEEDS_FLEET;
  const run = (action: TaskAction, then?: () => void) =>
    act(async () => {
      if (!ops) return;
      await ops.run(action);
      then?.();
    });
  const stop = (action: TaskAction, tell: boolean, word: string) =>
    act(async () => {
      if (!ops) return;
      await ops.run(action);
      if (tell && owner) {
        await ops.message(
          owner,
          `The operator ${word} task #${record.number}. Stop work on it and commit what you have; run \`fleet task show ${record.number}\` for why.`,
        );
      }
      setMode(null);
    });
  const quiet = (label: string, onClick: () => void, hint?: string) => (
    <button type="button" className="task-btn task-btn--quiet" disabled={off} title={why ?? hint} onClick={onClick}>
      {label}
    </button>
  );

  const setter = last(chain, (e) => SETS_STATUS.has(e.entry.entry));
  const named = last(chain, (e) => e.entry.entry === "reviewer-set");
  const code = last(chain, (e) => e.entry.entry === "receipt" || e.entry.entry === "released");
  const runs = [...new Set(chain.map((e) => e.run))];
  const tasks = isTask ? [] : (byGoal(board).find((g) => g.goal?.number === record.number)?.tasks ?? []);
  const live = tasks.filter((t) => t.status !== "removed");

  return (
    <article className="task-page">
      <div className="task-page__main">
        <nav className="task-crumbs" aria-label="breadcrumb">
          <button type="button" onClick={() => onNavigate(null)}>
            Tasks
          </button>
          <span>/</span>
          {goal && (
            <>
              <button type="button" onClick={() => onNavigate(goal.number)}>
                #{goal.number} {goal.block.outcome}
              </button>
              <span>/</span>
            </>
          )}
          <span className="task-crumbs__here">#{record.number}</span>
        </nav>

        <header className="task__head">
          <h3 className="task__title">
            <span className="mono task__id">#{record.number}</span> {!isTask && "Goal: "}
            {block.outcome}
          </h3>
          {record.status !== "removed" && mode !== "edit" && quiet("Edit", () => setMode("edit"))}
        </header>

        {mode === "edit" ? (
          <EditForm record={record} off={off} why={why} onSave={(action) => run(action, () => setMode(null))} onCancel={() => setMode(null)} />
        ) : mode === "release" ? (
          <ReleaseForm record={record} owner={owner} off={off} why={why} onRelease={(a, tell) => stop(a, tell, "released")} onCancel={() => setMode(null)} />
        ) : mode === "flag" || mode === "remove" ? (
          <ReasonForm
            kind={mode}
            record={record}
            owner={owner}
            off={off}
            why={why}
            onSend={(action, tell) => (mode === "remove" ? stop(action, tell, "removed") : run(action, () => setMode(null)))}
            onCancel={() => setMode(null)}
          />
        ) : (
          <div className="task-controls">
            {record.status === "removed" ? (
              quiet("Restore", () => run({ action: "restore", task: record.number }))
            ) : (
              <>
                {quiet("Flag", () => setMode("flag"), "flag it as not worth doing")}
                {quiet("Remove", () => setMode("remove"))}
                {isTask && record.owner && isOpen(record) && quiet("Release", () => setMode("release"))}
              </>
            )}
            {flagged > 0 && <span className="task-row__flagged">flagged by {flagged}</span>}
            {monitorFlags(record, board, now, store?.lineage).map((flag) => (
              <span key={flag.kind} className="task-flag">
                {flag.text}
              </span>
            ))}
          </div>
        )}

        <Criteria record={record} />
        {block.instructions && <p className="task__instructions">{block.instructions}</p>}

        <ol className="chain">
          {chain.map((event, index) => (
            <Fragment key={event.seq}>
              {index > 0 && chain[index - 1].run !== event.run && <Boundary record={record} index={index} starts={starts} />}
              <Entry event={event} record={record} />
            </Fragment>
          ))}
        </ol>

        <form
          className="task-composer"
          onSubmit={(e) => {
            e.preventDefault();
            run({ action: "comment", task: record.number, text: comment }, () => setComment(""));
          }}
        >
          <textarea
            className="task-composer__area"
            rows={3}
            placeholder={writable ? "Comment as operator…" : NEEDS_FLEET}
            value={comment}
            disabled={!writable}
            onChange={(e) => setComment(e.target.value)}
          />
          <div className="task-composer__row">
            {record.status !== "removed" &&
              record.status !== "dropped" &&
              quiet("Close as dropped", () => run({ action: "update", task: record.number, status: "dropped" }))}
            {(record.status === "dropped" || record.status === "done") &&
              quiet("Reopen", () => run({ action: "update", task: record.number, status: "planned" }))}
            <span className="grow" />
            <button type="submit" className="task-btn task-btn--primary" disabled={off || comment.trim() === ""} title={why}>
              Comment
            </button>
          </div>
        </form>
        {error && <p className="task-form__error">{error}</p>}
      </div>

      <aside className="task-facts">
        <Fact k="status">
          <Status status={record.status} />
          {setter && (
            <>
              {" "}
              by <span className="mono">{setter.from}</span> · <span className="mono">{clock(setter.at)}</span>
            </>
          )}
        </Fact>
        {isTask && (
          <Fact k="owner">
            {record.owner ? <span className="mono">{record.owner.pane}</span> : <span className="task-owner--none">unowned</span>}
            {carried && <span className="task-carried"> · {carried}</span>}
          </Fact>
        )}
        {isTask && (
          <Fact k="reviewer">
            {block.reviewer ? (
              <>
                <span className="mono">{block.reviewer}</span>, named by{" "}
                <span className="mono">{named ? named.from : record.creator}</span>
              </>
            ) : (
              "none"
            )}{" "}
            <label className="task-change">
              Change reviewer
              <select
              className="task-select"
              aria-label="reviewer"
              value={block.reviewer ?? ""}
              disabled={off}
              title={why}
              onChange={(e) =>
                e.target.value && run({ action: "edit", task: record.number, technical: [], vision: [], reviewer: e.target.value as PaneId })
              }
            >
              <option value="" disabled>
                name a reviewer
              </option>
              {WORKER_SLOTS.map(workerPane)
                .filter((pane) => pane !== record.owner?.pane)
                .map((pane) => (
                  <option key={pane} value={pane}>
                    {pane}
                  </option>
                ))}
              </select>
            </label>
          </Fact>
        )}
        {isTask && (
          <Fact k="latest code">
            {code?.entry.entry === "receipt" ? (
              <>
                <span className="mono">{place(code.entry)}</span>, from the receipt
              </>
            ) : code?.entry.entry === "released" ? (
              <>
                <span className="mono">{code.entry.where}</span>, from the release
              </>
            ) : (
              "none recorded"
            )}
          </Fact>
        )}
        {isTask && (
          <Fact k="goal">
            {goal ? (
              <button type="button" className="task-link" onClick={() => onNavigate(goal.number)}>
                #{goal.number} {goal.block.outcome}
              </button>
            ) : (
              "none"
            )}
            <label className="task-change">
              Move to another goal
              <select
              className="task-select"
              aria-label="goal"
              value={goals.some((g) => g.number === block.parent) ? String(block.parent) : ""}
              disabled={off}
              title={why}
              onChange={(e) =>
                e.target.value && run({ action: "edit", task: record.number, technical: [], vision: [], parent: Number(e.target.value) })
              }
            >
              <option value="" disabled>
                No goal
              </option>
              {goals.map((g) => (
                <option key={g.number} value={g.number}>
                  #{g.number} {g.block.outcome}
                </option>
              ))}
              </select>
            </label>
          </Fact>
        )}
        {!isTask && (
          <Fact k="tasks">
            {live.length === 0 ? "none yet" : `${live.length} task${live.length === 1 ? "" : "s"}, ${live.filter(isOpen).length} open`}
          </Fact>
        )}
        <Fact k="converges on">{block.converges_on != null ? <span className="mono">#{block.converges_on}</span> : "none"}</Fact>
        <Fact k="runs">
          <span className="mono">{runs.map((r) => runName(r, starts).replace(/^run /, "")).join(" · ")}</span>
        </Fact>
        <Fact k="opened by">
          <span className="mono">{record.creator}</span> · {dayClock(chain[0].at)}
        </Fact>
      </aside>
    </article>
  );
}
