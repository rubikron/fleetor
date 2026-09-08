// Your past sessions, and reopening one (WP-11, D-058; WP-27, R5).
//
// **This hook used to have two states and now has one.** It kept a catalogue
// *and* one archived run's contents, split into the same four lists `useFleet`
// splits the live stream into, so the History view could render a past run with
// the live components. That whole second face is gone: reopening a run copies its
// log into the live slot (R1), so Messages, Tasks and Activity are already
// showing that run's content through `useFleet` — the ordinary live path, reading
// what is now the live log. There is no archived-run rendering left to do, which
// is why `OpenRun` and its `split` have no replacement rather than a smaller one.
//
// What remains is a list and four verbs. `reopen` is the only one that is not a
// small edit: it tears the current fleet down, archives it, and brings a past
// run's five panes back — so it resolves to a `BootSnapshot` exactly as starting
// a fleet does, because that is what it is.

import { useCallback, useEffect, useState } from "react";
import { deleteRun, exportRun, listRuns, renameRun, reopenRun } from "./api";
import type { RunRecord } from "./types";

export interface RunsView {
  runs: RunRecord[];
  error: string | null;
  /// The run currently being reopened, so the row can say so and the list can
  /// refuse a second click while five panes are being torn down and respawned.
  opening: string | null;
  /// Bumped once per successful reopen (WP-27, R2).
  ///
  /// **The terminal grid keys off this, and it is load-bearing rather than
  /// cosmetic.** A reopen kills all five panes and re-enters bootstrap, but
  /// `TerminalPane` spawns from a *mount* effect — so panes that stay mounted
  /// across the reopen are never respawned, and the operator is left looking at
  /// five dead terminals with RESTART buttons. Changing this remounts them, which
  /// is also correct on its own terms: the buffers belong to the run that just
  /// ended, not to the one being opened.
  generation: number;
  refresh: () => void;
  rename: (id: string, label: string) => Promise<void>;
  remove: (id: string) => Promise<void>;
  /// Reopen a past run. Rejects (and leaves the live fleet alone) when the run
  /// cannot be fully restored — the backend checks the manifest before it tears
  /// anything down (R8).
  reopen: (id: string) => Promise<void>;
  /// Resolves to where it was saved, or `null` if the dialog was dismissed.
  save: (id: string) => Promise<string | null>;
  /// The last successful save, so the view can say where the file went rather
  /// than flashing a toast that is gone before it is read.
  saved: string | null;
}

export function useRuns(): RunsView {
  const [runs, setRuns] = useState<RunRecord[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [opening, setOpening] = useState<string | null>(null);
  const [generation, setGeneration] = useState(0);
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

  // The failure path is the interesting one. A refusal arrives before any pane is
  // killed, so the operator is still in the fleet they were in — the error goes on
  // screen and nothing else changes.
  const reopen = useCallback(
    async (id: string) => {
      setOpening(id);
      setError(null);
      try {
        await reopenRun(id);
        setGeneration((g) => g + 1);
        refresh();
      } catch (e) {
        setError(String(e));
        throw e;
      } finally {
        setOpening(null);
      }
    },
    [refresh],
  );

  const save = useCallback(async (id: string) => {
    const where = await exportRun(id);
    if (where) setSaved(where);
    return where;
  }, []);

  return { runs, error, opening, generation, refresh, rename, remove, reopen, save, saved };
}
