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

import { useCallback, useEffect, useRef, useState } from "react";
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
  start: () => Promise<void>;
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
  const [configNonce, setConfigNonce] = useState(0);
  const listenerReady = useRef(false);

  // Attach the event listener on mount — before any bootstrap, so events
  // emitted during bootstrap are captured.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    (async () => {
      try {
        unlisten = await onFleetEvent((event) => {
          setFeed((f) => [event, ...f].slice(0, MAX_FEED));
          if (isMessage(event)) setMessages((m) => [event, ...m]);
          if (isCommand(event)) setCommands((c) => [event, ...c]);
          if (isTask(event)) setTasks((t) => [event, ...t]);
        });
        if (cancelled) {
          unlisten();
          return;
        }
        listenerReady.current = true;
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Config works before bootstrap (fleet_config reads config.json directly
  // when the fleet isn't bootstrapped yet), so the start gate can show what
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

  const start = useCallback(async () => {
    try {
      await bootstrap();
      setReady(true);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  return {
    ready,
    error,
    feed,
    messages,
    commands,
    tasks,
    config,
    refreshConfig: () => setConfigNonce((n) => n + 1),
    start,
  };
}
