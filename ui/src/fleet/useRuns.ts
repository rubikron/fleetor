// Your sessions (WP-11, D-058; D-099): the running one, those still archiving
// and the archived ones — a list and three verbs. Reopening one is a
// launch, which `App` owns (D-099) — the run's log becomes the live log, so
// Messages, Tasks and Activity show it through `useFleet`.

import { useCallback, useEffect, useState } from "react";
import { deleteRun, exportRun, listRuns, onFleetArchived, renameRun } from "./api";
import { stopListening } from "./listeners";
import type { RunRecord } from "./types";

export interface RunsView {
  runs: RunRecord[];
  error: string | null;
  refresh: () => void;
  rename: (id: string, label: string) => Promise<void>;
  remove: (id: string) => Promise<void>;
  /// Resolves to where it was saved, or `null` if the dialog was dismissed.
  save: (id: string) => Promise<string | null>;
  /// The last successful save, so the view can say where the file went rather
  /// than flashing a toast that is gone before it is read.
  saved: string | null;
}

export function useRuns(): RunsView {
  const [runs, setRuns] = useState<RunRecord[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
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

  // The run a launch left behind has finished archiving: its row changes
  // state. (`App` refreshes when the launch itself settles.)
  useEffect(() => {
    let cancelled = false;
    let stop: (() => void) | undefined;
    void onFleetArchived(refresh)
      .then((unlisten) => {
        if (cancelled) stopListening(unlisten, "fleet archived");
        else stop = unlisten;
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      stopListening(stop, "fleet archived");
    };
  }, [refresh]);

  const rename = useCallback(
    async (id: string, label: string) => {
      await renameRun(id, label);
      refresh();
    },
    [refresh],
  );

  const remove = useCallback(
    async (id: string) => {
      await deleteRun(id);
      refresh();
    },
    [refresh],
  );

  const save = useCallback(async (id: string) => {
    const where = await exportRun(id);
    if (where) setSaved(where);
    return where;
  }, []);

  return { runs, error, refresh, rename, remove, save, saved };
}
