// The single live-state hook. On mount it bootstraps the embedded fleet, seeds
// the board from the snapshot, and subscribes to the event stream — reducing
// worker/tool events into the dashboard band, appending everything to the feed,
// and refetching the board whenever a ticket moves (the store stays the board's
// source of truth; events only trigger the refetch).

import { useCallback, useEffect, useRef, useState } from "react";
import { bootstrap, fetchBoard, fetchConfig, onFleetEvent } from "./api";
import { WORKER_SLOTS, type FleetConfig, type FleetEvent, type Ticket, type WorkerState } from "./types";

/// A worker slot as the band renders it, reduced from the event stream.
export interface WorkerCell {
  slot: number;
  state: WorkerState;
  ticket: string | null;
  activity: string | null;
}

/// One rendered line in a worker's transcript view (observability).
export interface TranscriptLine {
  seq: number;
  slot: number;
  kind: "said" | "tool" | "mail" | "exit";
  text: string;
  tone: "neutral" | "accent" | "green" | "red";
}

export interface FleetView {
  ready: boolean;
  error: string | null;
  board: Ticket[];
  workers: WorkerCell[];
  feed: FleetEvent[];
  transcripts: Record<number, TranscriptLine[]>;
  config: FleetConfig | null;
}

const MAX_FEED = 300;
const MAX_TRANSCRIPT = 400;

function initialWorkers(): Record<number, WorkerCell> {
  return Object.fromEntries(
    WORKER_SLOTS.map((slot) => [slot, { slot, state: "idle" as WorkerState, ticket: null, activity: null }]),
  );
}

/// Fold one event into the worker map, returning a new map (immutable update).
function reduceWorker(
  workers: Record<number, WorkerCell>,
  event: FleetEvent,
): Record<number, WorkerCell> {
  if (event.type === "worker-state") {
    const prev = workers[event.slot] ?? { slot: event.slot, state: "idle", ticket: null, activity: null };
    return { ...workers, [event.slot]: { ...prev, state: event.to } };
  }
  if (event.type === "tool-activity") {
    const prev = workers[event.slot] ?? { slot: event.slot, state: "working", ticket: null, activity: null };
    return { ...workers, [event.slot]: { ...prev, ticket: event.ticket, activity: event.tool } };
  }
  return workers;
}

/// Derive a transcript line from an event, or `null` if the event isn't
/// worker-scoped. Mail counts only when addressed to a worker (so the orch's
/// steering shows up in that worker's transcript).
function transcriptLine(event: FleetEvent): TranscriptLine | null {
  switch (event.type) {
    case "worker-said":
      return { seq: event.seq, slot: event.slot, kind: "said", text: event.text, tone: "neutral" };
    case "tool-activity":
      return { seq: event.seq, slot: event.slot, kind: "tool", text: `→ ${event.tool}`, tone: "accent" };
    case "worker-exited":
      return {
        seq: event.seq,
        slot: event.slot,
        kind: "exit",
        text: event.ok ? "exited cleanly" : `died — ${event.detail || "no output"}`,
        tone: event.ok ? "green" : "red",
      };
    case "mail": {
      const m = event.to.match(/^worker-(\d+)$/);
      return m ? { seq: event.seq, slot: Number(m[1]), kind: "mail", text: `◀ mail from ${event.from}`, tone: "accent" } : null;
    }
    default:
      return null;
  }
}

/// Fold one event into the per-slot transcript map (immutable, bounded tail).
function reduceTranscript(
  transcripts: Record<number, TranscriptLine[]>,
  event: FleetEvent,
): Record<number, TranscriptLine[]> {
  const line = transcriptLine(event);
  if (!line) return transcripts;
  const prev = transcripts[line.slot] ?? [];
  return { ...transcripts, [line.slot]: [...prev, line].slice(-MAX_TRANSCRIPT) };
}

export function useFleet(): FleetView {
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [board, setBoard] = useState<Ticket[]>([]);
  const [workers, setWorkers] = useState<Record<number, WorkerCell>>(initialWorkers);
  const [feed, setFeed] = useState<FleetEvent[]>([]);
  const [transcripts, setTranscripts] = useState<Record<number, TranscriptLine[]>>({});
  const [config, setConfig] = useState<FleetConfig | null>(null);

  // Refetch the board off the store; deduped so a burst of moves is one round-trip.
  const refetchPending = useRef(false);
  const refetchBoard = useCallback(async () => {
    if (refetchPending.current) return;
    refetchPending.current = true;
    try {
      const next = await fetchBoard();
      setBoard(next);
    } catch (e) {
      setError(String(e));
    } finally {
      refetchPending.current = false;
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    (async () => {
      try {
        const snap = await bootstrap();
        if (cancelled) return;
        setBoard(snap.board);
        // Config is a snapshot of the live posture (target/branch/backend); fetch
        // once after bootstrap. A failure here must not sink the whole shell.
        try {
          const cfg = await fetchConfig();
          if (!cancelled) setConfig(cfg);
        } catch {
          /* top bar falls back to placeholders */
        }
        unlisten = await onFleetEvent((event) => {
          setFeed((f) => [event, ...f].slice(0, MAX_FEED));
          setWorkers((w) => reduceWorker(w, event));
          setTranscripts((t) => reduceTranscript(t, event));
          if (event.type === "ticket-state") void refetchBoard();
        });
        if (cancelled) {
          unlisten();
          return;
        }
        setReady(true);
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [refetchBoard]);

  return {
    ready,
    error,
    board,
    workers: WORKER_SLOTS.map((slot) => workers[slot]),
    feed,
    transcripts,
    config,
  };
}
