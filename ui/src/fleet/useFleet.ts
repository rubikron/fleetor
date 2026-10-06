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

import { useCallback, useEffect, useRef, useState } from "react";
import {
  fetchConfig,
  launchFleet,
  onFleetEvent,
  onFleetLaunching,
  type LaunchFailure,
  type LaunchSource,
} from "./api";
import { stopListening } from "./listeners";
import {
  isCommand,
  isMessage,
  isTask,
  type CommandEvent,
  type FleetConfig,
  type FleetEvent,
  type MessageEvent,
  type PaneIdentityMap,
  type TaskEvent,
} from "./types";

export interface FleetView {
  ready: boolean;
  error: string | null;
  feed: FleetEvent[];
  messages: MessageEvent[];
  commands: CommandEvent[];
  tasks: TaskEvent[];
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

  // Task state on a launch (D-099, Coordination A). Clears the list; the task
  // PRD replaces the body with a replay of the fleet's task store.
  const resetTasks = useCallback(() => setTasks([]), []);

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

    (async () => {
      try {
        unlisten = await onFleetEvent((event) => {
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
        if (cancelled) {
          stopListening(unlisten, "fleet events");
          stopListening(unlistenLaunching, "fleet launching");
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
    };
  }, [reset]);

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

  return {
    ready,
    error,
    feed,
    messages,
    commands,
    tasks,
    config,
    panes,
    refreshConfig: () => setConfigNonce((n) => n + 1),
    launch,
  };
}
