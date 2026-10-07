// The render probe behind `tests/task_view_renders.rs` (D-100).
//
// It imports the real `TaskBoard` and renders it through React's server
// renderer against synthetic chain entries. It renders and prints; it asserts
// nothing — the same shape as `tests/history_probe/render.tsx`.

import { renderToStaticMarkup } from "react-dom/server";
import { EventFeed } from "../../../ui/src/components/EventFeed";
import { TaskBoard, type TaskOps } from "../../../ui/src/components/TaskBoard";
import type { ChainEntry, ChainEvent, FleetEvent, PaneId, TaskBlock } from "../../../ui/src/fleet/types";
import type { TaskStoreInfo } from "../../../ui/src/fleet/useFleet";

let seq = 0;
const at = (task: number, from: PaneId, entry: ChainEntry, run = "run-1"): ChainEvent => ({
  seq: ++seq,
  type: "chain",
  task,
  from,
  at: 1_789_000_000_000 + seq * 60_000,
  run,
  lineage: run === "run-1" ? "lin-1" : "lin-2",
  entry,
});

const goal: TaskBlock = { kind: "goal", outcome: "one grammar", vision: ["the parser is the product"] };
const task = (outcome: string, owner: PaneId | null, parent: number | null): TaskBlock => ({
  kind: "task",
  outcome,
  technical: ["cargo test -p parser"],
  vision: ["one grammar"],
  owner,
  parent,
  instructions: "start from the tokenizer",
});

const CHAIN: ChainEvent[] = [
  at(1, "operator", { entry: "opened", block: goal }),
  at(2, "orch", { entry: "opened", block: task("nested groups parse", "worker-2", 1) }),
  at(3, "orch", { entry: "opened", block: task("errors name the token", null, 1) }),
  at(4, "worker-3", { entry: "opened", block: task("the tokenizer leaks", null, null) }),
  at(2, "worker-2", { entry: "taken-up", note: "starting" }),
  at(2, "worker-3", { entry: "commented", text: "the tokenizer leaks a buffer" }),
  at(2, "orch", {
    entry: "edited",
    field: "technical",
    old: ["cargo test -p parser"],
    new: ["cargo test -p parser", "clippy is clean"],
  }),
  at(2, "worker-2", { entry: "status", status: "done", note: "cargo test passes" }, "run-2"),
  at(3, "orch", { entry: "status", status: "dropped" }),
];

// S2: #5 released by its owner then released again by orch for worker-1;
// #6 still held by a pane from the earlier session.
const RELEASE = { why: "out of context", done: "the tokenizer", left: "the parser" };
const HANDED_ON: ChainEvent[] = [
  ...CHAIN,
  at(5, "orch", { entry: "opened", block: task("spans survive a reparse", "worker-3", 1) }),
  at(5, "worker-3", { entry: "taken-up" }),
  at(5, "worker-3", { entry: "released", ...RELEASE, where: "fleet/logstat/worker-3 @ a1b2c3d" }),
  at(5, "worker-1", { entry: "taken-up" }),
  at(5, "orch", {
    entry: "released",
    ...RELEASE,
    where: "fleet/logstat/worker-1 @ d4e5f6a",
    on_behalf_of: "worker-1",
  }),
  at(6, "orch", { entry: "opened", block: task("errors carry a span", null, 1) }),
  at(6, "worker-4", { entry: "taken-up" }),
];

const LIVE: TaskStoreInfo = { live: true, target: "/work/logstat", run: "run-2", lineage: "lin-2" };
const GATE: TaskStoreInfo = { live: false, target: "/work/logstat", run: null, lineage: null };

const OPS: TaskOps = { run: async () => "1", message: async () => {} };

const notice = (seq: number, text: string): FleetEvent => ({ seq, type: "notice", level: "info", text });

process.stdout.write(
  JSON.stringify({
    list: renderToStaticMarkup(<TaskBoard chain={CHAIN} store={LIVE} ops={OPS} />),
    page: renderToStaticMarkup(<TaskBoard chain={CHAIN} store={LIVE} ops={OPS} initialOpen={2} />),
    readonly: renderToStaticMarkup(<TaskBoard chain={CHAIN} store={GATE} ops={OPS} initialOpen={2} />),
    newGoal: renderToStaticMarkup(<TaskBoard chain={CHAIN} store={LIVE} ops={OPS} initialForm="goal" />),
    newTask: renderToStaticMarkup(<TaskBoard chain={CHAIN} store={LIVE} ops={OPS} initialForm="task" />),
    editing: renderToStaticMarkup(
      <TaskBoard chain={CHAIN} store={LIVE} ops={OPS} initialOpen={2} initialEdit />,
    ),
    done: renderToStaticMarkup(<TaskBoard chain={CHAIN} store={LIVE} ops={OPS} initialFilter="done" />),
    inProgress: renderToStaticMarkup(
      <TaskBoard chain={CHAIN} store={LIVE} ops={OPS} initialFilter="in-progress" />,
    ),
    // Newest first: the take-up arrived after run-log seq 1, the done after seq 2.
    activity: renderToStaticMarkup(
      <EventFeed
        feed={[notice(3, "third"), notice(2, "second"), notice(1, "first")]}
        tasks={[
          { event: CHAIN[4], after: 1 },
          { event: CHAIN[7], after: 2 },
        ]}
      />,
    ),
    dropped: renderToStaticMarkup(<TaskBoard chain={CHAIN} store={LIVE} ops={OPS} initialOpen={3} />),
    released: renderToStaticMarkup(
      <TaskBoard chain={HANDED_ON} store={LIVE} ops={OPS} initialOpen={5} />,
    ),
    releasing: renderToStaticMarkup(
      <TaskBoard chain={HANDED_ON} store={LIVE} ops={OPS} initialOpen={6} initialRelease />,
    ),
    released_activity: renderToStaticMarkup(
      <EventFeed feed={[notice(1, "first")]} tasks={[{ event: HANDED_ON[13], after: 1 }]} />,
    ),
    empty: renderToStaticMarkup(<TaskBoard chain={[]} store={GATE} />),
  }),
);
