// Persists the operator's place in the shell — which top-level view they're
// on, and which worker tab is selected — across restarts. Same validated-read
// pattern as useSidebarCollapse.ts: a corrupt, missing, or out-of-range
// stored value must fall back to the current default and never throw or
// wedge the app (e.g. a worker slot left over from a build with a different
// WORKER_SLOTS list, or a view name that no longer exists).

import { useCallback, useEffect, useState } from "react";
import type { View } from "../components/Sidebar";
import { WORKER_SLOTS } from "../fleet/types";

const VIEW_STORAGE_KEY = "fleetor:view";
const WORKER_STORAGE_KEY = "fleetor:selected-worker";

const VIEWS: readonly View[] = ["fleet", "messages", "activity", "settings"];
const DEFAULT_VIEW: View = "fleet";
const DEFAULT_WORKER: number = WORKER_SLOTS[0];

function isValidView(value: unknown): value is View {
  return typeof value === "string" && (VIEWS as readonly string[]).includes(value);
}

function isValidWorkerSlot(value: unknown): value is number {
  return typeof value === "number" && (WORKER_SLOTS as readonly number[]).includes(value);
}

function readStoredView(): View {
  try {
    const raw = window.localStorage.getItem(VIEW_STORAGE_KEY);
    if (raw === null) return DEFAULT_VIEW;
    const parsed: unknown = JSON.parse(raw);
    return isValidView(parsed) ? parsed : DEFAULT_VIEW;
  } catch {
    return DEFAULT_VIEW;
  }
}

function writeStoredView(view: View): void {
  try {
    window.localStorage.setItem(VIEW_STORAGE_KEY, JSON.stringify(view));
  } catch {
    /* best-effort persistence only; nav still works for the session */
  }
}

function readStoredWorkerSlot(): number {
  try {
    const raw = window.localStorage.getItem(WORKER_STORAGE_KEY);
    if (raw === null) return DEFAULT_WORKER;
    const parsed: unknown = JSON.parse(raw);
    return isValidWorkerSlot(parsed) ? parsed : DEFAULT_WORKER;
  } catch {
    return DEFAULT_WORKER;
  }
}

function writeStoredWorkerSlot(slot: number): void {
  try {
    window.localStorage.setItem(WORKER_STORAGE_KEY, JSON.stringify(slot));
  } catch {
    /* best-effort persistence only; nav still works for the session */
  }
}

export interface PersistedNav {
  view: View;
  setView: (view: View) => void;
  selectedWorker: number;
  setSelectedWorker: (slot: number) => void;
}

export function usePersistedNav(): PersistedNav {
  const [view, setViewState] = useState<View>(readStoredView);
  const [selectedWorker, setSelectedWorkerState] = useState<number>(readStoredWorkerSlot);

  useEffect(() => {
    writeStoredView(view);
  }, [view]);
  useEffect(() => {
    writeStoredWorkerSlot(selectedWorker);
  }, [selectedWorker]);

  const setView = useCallback((next: View) => setViewState(next), []);
  const setSelectedWorker = useCallback((slot: number) => setSelectedWorkerState(slot), []);

  return { view, setView, selectedWorker, setSelectedWorker };
}
