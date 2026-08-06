// The single live-state hook: bootstrap the embedded fleet, then stream every
// appended event into two lists.
//
// **Two lists, not one, and that is the point.** `feed` is a scrolling activity
// log and is bounded — dropping its oldest rows costs nothing. `messages` is the
// record of what the fleet actually said, and is not bounded by anything: this
// product's whole deliverable is that record, so silently discarding the oldest
// message once 300 events have gone by would be a hole in the thing being sold.
//
// The listener is attached **before** `bootstrap()`. A Tauri `emit` with no
// listener is a drop, and bootstrap's own first act is to append notices about
// the target — so subscribing afterwards loses exactly the events that explain
// where the fleet is working.

import { useEffect, useState } from "react";
import { bootstrap, fetchConfig, onFleetEvent } from "./api";
import {
  isCommand,
  isMessage,
  isTask,
  type CommandEvent,
  type FleetConfig,
  type FleetEvent,
  type MessageEvent,
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
  refreshConfig: () => void;
}

/// The activity log's tail. Messages are exempt — see the module comment.
const MAX_FEED = 300;

export function useFleet(): FleetView {
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [feed, setFeed] = useState<FleetEvent[]>([]);
  const [messages, setMessages] = useState<MessageEvent[]>([]);
  const [commands, setCommands] = useState<CommandEvent[]>([]);
  const [tasks, setTasks] = useState<TaskEvent[]>([]);
  const [config, setConfig] = useState<FleetConfig | null>(null);
  const [configNonce, setConfigNonce] = useState(0);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    (async () => {
      try {
        unlisten = await onFleetEvent((event) => {
          setFeed((f) => [event, ...f].slice(0, MAX_FEED));
          if (isMessage(event)) setMessages((m) => [event, ...m]);
          // Unbounded for the same reason messages are: a command's `why` is the
          // reasoning chain the log exists to keep, and dropping the oldest ones
          // would quietly delete the earliest reasoning first.
          if (isCommand(event)) setCommands((c) => [event, ...c]);
          // Unbounded, and here for a harder reason than the other two: the
          // board is a *fold* over these events, so dropping the oldest would
          // silently delete blocks from the board rather than trimming a log.
          if (isTask(event)) setTasks((t) => [event, ...t]);
        });
        if (cancelled) {
          unlisten();
          return;
        }

        await bootstrap();
        if (cancelled) return;
        setReady(true);
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Config is a snapshot of the live posture; refetched when the operator
  // changes the target. A failure here must not sink the whole shell — the gate
  // falls back to saying it does not know.
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
    refreshConfig: () => setConfigNonce((n) => n + 1),
  };
}
