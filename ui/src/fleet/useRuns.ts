// Past runs, read (WP-11, D-058).
//
// The deliberate mirror of `useFleet`: that hook subscribes to a live stream and
// splits it into four lists, this one fetches a finished log and splits it the
// same way. Producing the identical shape is the whole trick — it means
// `MessageFeed`, `TaskBoard` and `EventFeed` render a past run with no
// past-run-aware code in any of them.
//
// **Two states that must not be confused.** `runs` is the catalogue and is
// always safe to show. `open` is one run's contents, and it is `null` both
// before a run is chosen and while one is loading — `loading` is what tells
// those apart, because a spinner and an empty archive look identical otherwise.
//
// Nothing here writes to a run except `rename`. There is no append, no resume,
// and no path by which a past run's events reach the live views: the History
// view owns its own copies of the three components, so live data and archived
// data never share a list.

import { useCallback, useEffect, useState } from "react";
import { deleteRun, listRuns, renameRun, runEvents } from "./api";
import {
  isCommand,
  isMessage,
  isTask,
  type CommandEvent,
  type FleetEvent,
  type MessageEvent,
  type RunRecord,
  type TaskEvent,
} from "./types";

/// One archived run's log, split the way `useFleet` splits the live one.
export interface OpenRun {
  record: RunRecord;
  feed: FleetEvent[];
  messages: MessageEvent[];
  commands: CommandEvent[];
  tasks: TaskEvent[];
}

export interface RunsView {
  runs: RunRecord[];
  error: string | null;
  open: OpenRun | null;
  loading: boolean;
  refresh: () => void;
  openRun: (record: RunRecord) => void;
  close: () => void;
  rename: (id: string, label: string) => Promise<void>;
  remove: (id: string) => Promise<void>;
}

/// Newest-first, matching `useFleet`'s lists so the components below receive
/// what they already expect. They each sort by `seq` internally anyway; this is
/// about the two paths staying the same shape, not about the sort.
function split(record: RunRecord, events: FleetEvent[]): OpenRun {
  const newestFirst = [...events].reverse();
  return {
    record,
    feed: newestFirst,
    messages: newestFirst.filter(isMessage),
    commands: newestFirst.filter(isCommand),
    tasks: newestFirst.filter(isTask),
  };
}

export function useRuns(): RunsView {
  const [runs, setRuns] = useState<RunRecord[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState<OpenRun | null>(null);
  const [loading, setLoading] = useState(false);
  const [nonce, setNonce] = useState(0);

  useEffect(() => {
    let cancelled = false;
    listRuns()
      .then((rows) => {
        if (cancelled) return;
        setRuns(rows);
        setError(null);
      })
      .catch((e) => !cancelled && setError(String(e)));
    return () => {
      cancelled = true;
    };
  }, [nonce]);

  const refresh = useCallback(() => setNonce((n) => n + 1), []);

  const openRun = useCallback((record: RunRecord) => {
    setLoading(true);
    setOpen(null);
    runEvents(record.id)
      .then((events) => setOpen(split(record, events)))
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, []);

  const close = useCallback(() => setOpen(null), []);

  const rename = useCallback(
    async (id: string, label: string) => {
      await renameRun(id, label);
      // Keep an open run's banner honest rather than waiting for the refetch.
      setOpen((o) => (o && o.record.id === id ? { ...o, record: { ...o.record, label } } : o));
      refresh();
    },
    [refresh],
  );

  const remove = useCallback(
    async (id: string) => {
      await deleteRun(id);
      setOpen((o) => (o && o.record.id === id ? null : o));
      refresh();
    },
    [refresh],
  );

  return { runs, error, open, loading, refresh, openRun, close, rename, remove };
}
