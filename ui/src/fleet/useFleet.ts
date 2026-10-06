// The single live-state hook: launch the embedded fleet, then stream every
// appended event into two lists.
//
// **Two lists, not one, and that is the point.** `feed` is a scrolling activity
// log and is bounded — dropping its oldest rows costs nothing. `messages` is the
// record of what the fleet actually said, and is not bounded by anything: this
// product's whole deliverable is that record, so silently discarding the oldest
// message once 300 events have gone by would be a hole in the thing being sold.
//
// The listener is attached at mount, **before** any launch. A Tauri `emit` with
// no listener is a drop, and a launch's own first act is to append notices about
// the target — so subscribing afterwards loses exactly the events that explain
// where the fleet is working.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  fetchConfig,
  fetchTasks,
  launchFleet,
  onFleetEvent,
  onFleetLaunching,
  onTaskEvent,
  type LaunchFailure,
  type LaunchSource,
} from "./api";
import { stopListening } from "./listeners";
import {
  isCommand,
  isMessage,
  isTask,
  type ChainEvent,
  type CommandEvent,
  type FleetConfig,
  type FleetEvent,
  type MessageEvent,
  type PaneIdentityMap,
  type TaskEvent,
} from "./types";

/// The task store on screen: the live fleet's (`live`), or the gate target's
/// shown read-only when no fleet runs.
export interface TaskStoreInfo {
  live: boolean;
  target: string;
  run: string | null;
  lineage: string | null;
}

/// A task entry for the Activity feed. Run-log events carry no timestamp, so
/// it is placed by arrival: `after` is the last run-log `seq` seen before it.
export interface TaskFeedEntry {
  event: ChainEvent;
  after: number;
}

/// Add entries to a list kept oldest first, once each by `seq`.
function mergeChain(have: ChainEvent[], incoming: ChainEvent[]): ChainEvent[] {
  const seen = new Set(have.map((e) => e.seq));
  const fresh = incoming.filter((e) => !seen.has(e.seq));
  if (fresh.length === 0) return have;
  return [...have, ...fresh].sort((a, b) => a.seq - b.seq);
}

export interface FleetView {
  ready: boolean;
  error: string | null;
  feed: FleetEvent[];
  messages: MessageEvent[];
  commands: CommandEvent[];
  /// Legacy run-log task events; only a reopened pre-D-100 run has any.
  tasks: TaskEvent[];
  /// Every chain entry in the task store being shown, oldest first.
  chain: ChainEvent[];
  /// Whose store that is and whether it can be written; `null` until read.
  taskStore: TaskStoreInfo | null;
  /// This lineage's task entries, for the Activity feed.
  taskFeed: TaskFeedEntry[];
  config: FleetConfig | null;
  /// What each pane was placed as, folded out of the spawn events (#50).
  ///
  /// **A fold rather than a query**, for the reason there is no query to make: the
  /// roster is harness-free and stays that way (M25), so the spawn event is the
  /// only per-pane channel carrying a harness — which is exactly what C56 said a
  /// badge would need and expected to cost a new command. It did not: the event was
  /// already on the wire and already rendered in the feed.
  panes: PaneIdentityMap;
  refreshConfig: () => void;
  /// Bring a fleet up (D-099). Feed, messages, pane identities and tasks reset
  /// when the backend says the verdict passed, and `onVerdictPassed` runs with
  /// them — so a refused launch resets nothing. Rejects with the backend's
  /// `LaunchFailure`.
  launch: (source: LaunchSource, onVerdictPassed: () => void) => Promise<void>;
}

function asFailure(e: unknown): LaunchFailure {
  if (e && typeof e === "object" && "reason" in e) return e as LaunchFailure;
  return { reason: String(e), torn_down: false };
}

const MAX_FEED = 300;

export function useFleet(): FleetView {
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [feed, setFeed] = useState<FleetEvent[]>([]);
  const [messages, setMessages] = useState<MessageEvent[]>([]);
  const [commands, setCommands] = useState<CommandEvent[]>([]);
  const [tasks, setTasks] = useState<TaskEvent[]>([]);
  const [config, setConfig] = useState<FleetConfig | null>(null);
  const [panes, setPanes] = useState<PaneIdentityMap>({});
  const [configNonce, setConfigNonce] = useState(0);
  const listenerReady = useRef(false);
  // The reset a launch in flight is owed, taken exactly once.
  const owedReset = useRef<(() => void) | null>(null);

  const [chain, setChain] = useState<ChainEvent[]>([]);
  const [taskStore, setTaskStore] = useState<TaskStoreInfo | null>(null);
  // Bumped whenever the store on screen changes, so a read of the old one is dropped.
  const taskStoreGen = useRef(0);
  // The last run-log seq seen, and the one each task entry arrived after.
  const lastRunSeq = useRef(0);
  const arrivedAfter = useRef(new Map<number, number>());

  // Task state on a launch (D-099, Coordination A): clear, then the new fleet's
  // store arrives on `fleet://task` and through `loadTasks`.
  const resetTasks = useCallback(() => {
    taskStoreGen.current += 1;
    lastRunSeq.current = 0;
    arrivedAfter.current.clear();
    setTasks([]);
    setChain([]);
  }, []);

  const loadTasks = useCallback(async () => {
    const gen = taskStoreGen.current;
    try {
      const snapshot = await fetchTasks();
      if (gen !== taskStoreGen.current) return;
      setTaskStore({
        live: snapshot.live,
        target: snapshot.target,
        run: snapshot.run ?? null,
        lineage: snapshot.lineage ?? null,
      });
      setChain((have) => mergeChain(have, snapshot.events));
    } catch {
      /* the Tasks view renders empty */
    }
  }, []);

  // Drop the run being left. Fired by the backend's "verdict passed" event; the
  // launch's own settle calls it too, so it happens once whichever lands first.
  const reset = useCallback(() => {
    const onVerdictPassed = owedReset.current;
    if (!onVerdictPassed) return;
    owedReset.current = null;
    setReady(false);
    setFeed([]);
    setMessages([]);
    setCommands([]);
    setPanes({});
    resetTasks();
    onVerdictPassed();
  }, [resetTasks]);

  const launch = useCallback(
    async (source: LaunchSource, onVerdictPassed: () => void) => {
      owedReset.current = onVerdictPassed;
      try {
        await launchFleet(source);
        reset();
        setReady(true);
      } catch (e) {
        const failure = asFailure(e);
        if (failure.torn_down) {
          reset();
          resetTasks();
        }
        // Refused at the verdict: the fleet that was running still is.
        owedReset.current = null;
        throw failure;
      } finally {
        setConfigNonce((n) => n + 1);
      }
    },
    [reset, resetTasks],
  );

  // Attach the event listener on mount — before any launch, so events emitted
  // during one are captured.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    let unlistenLaunching: (() => void) | undefined;
    let unlistenTasks: (() => void) | undefined;

    (async () => {
      try {
        unlisten = await onFleetEvent((event) => {
          lastRunSeq.current = Math.max(lastRunSeq.current, event.seq);
          setFeed((f) => [event, ...f].slice(0, MAX_FEED));
          if (isMessage(event)) setMessages((m) => [event, ...m]);
          if (isCommand(event)) setCommands((c) => [event, ...c]);
          if (isTask(event)) setTasks((t) => [event, ...t]);
          // Only a spawn carries a harness; every other transition leaves the
          // pane's identity exactly as it was rather than clearing it, so a pane
          // that has died still says what it ran.
          if (event.type === "pane-state" && event.harness) {
            const { pane, harness, mark, model } = event;
            setPanes((p) => ({ ...p, [pane]: { harness, mark, model } }));
          }
        });
        unlistenLaunching = await onFleetLaunching(reset);
        unlistenTasks = await onTaskEvent((event) => {
          if (!arrivedAfter.current.has(event.seq)) {
            arrivedAfter.current.set(event.seq, lastRunSeq.current);
          }
          setChain((have) => mergeChain(have, [event]));
        });
        if (cancelled) {
          stopListening(unlisten, "fleet events");
          stopListening(unlistenLaunching, "fleet launching");
          stopListening(unlistenTasks, "task events");
          return;
        }
        listenerReady.current = true;
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    })();

    return () => {
      cancelled = true;
      stopListening(unlisten, "fleet events");
      stopListening(unlistenLaunching, "fleet launching");
      stopListening(unlistenTasks, "task events");
    };
  }, [reset]);

  // The store on screen: the live fleet's once one is up, otherwise the gate
  // target's, re-read whenever the gate's target may have moved.
  useEffect(() => {
    if (!ready) {
      taskStoreGen.current += 1;
      setChain([]);
    }
    void loadTasks();
  }, [ready, configNonce, loadTasks]);

  // Config works before a launch (fleet_config reads config.json directly
  // when no fleet is up), so the start gate can show what
  // a click will launch.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const cfg = await fetchConfig();
        if (!cancelled) setConfig(cfg);
      } catch {
        /* the gate and top bar render placeholders */
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [ready, configNonce]);

  // The Activity feed shows this lineage's entries only: this run and the runs
  // it was reopened from, not every session the target has had.
  const lineage = taskStore?.live ? taskStore.lineage : null;
  const taskFeed = useMemo(
    () =>
      chain
        .filter((event) => lineage !== null && event.lineage === lineage)
        .map((event) => ({ event, after: arrivedAfter.current.get(event.seq) ?? 0 })),
    [chain, lineage],
  );

  return {
    ready,
    error,
    feed,
    messages,
    commands,
    tasks,
    chain,
    taskStore,
    taskFeed,
    config,
    panes,
    refreshConfig: () => setConfigNonce((n) => n + 1),
    launch,
  };
}
