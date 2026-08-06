// The UI half of WP-04's live gauge: a slow poll of `fleet roster`, reduced
// into a per-pane map the band/pane-head components read straight off.
//
// **On-demand, not a push.** The backend samples a worker's transcript (a
// filesystem read) only when asked — this timer is the "asked," on the cheap
// side the requirements doc recommended ("no hot loops over five JSONL
// files"). The orchestrator's own `fleet roster` calls are the other asker;
// both land on the identical backend answer.

import { useEffect, useState } from "react";
import { fetchRoster } from "./api";
import type { ContextGauge, PaneId } from "./types";

/// Slow on purpose: a live gauge that lagged a few seconds behind a fast-
/// moving pane is still useful, and every tick is a filesystem read per
/// worker on the Rust side.
const POLL_MS = 10_000;

/// Pane → its gauge, or absent. Absent is the default and the honest answer
/// for a pane nobody has sampled yet — never render a missing entry as 0%.
export type ContextGaugeMap = Partial<Record<PaneId, ContextGauge>>;

export function useContextGauge(active: boolean): ContextGaugeMap {
  const [gauges, setGauges] = useState<ContextGaugeMap>({});

  useEffect(() => {
    if (!active) return;
    let cancelled = false;

    const poll = async () => {
      try {
        const roster = await fetchRoster();
        if (cancelled) return;
        const next: ContextGaugeMap = {};
        for (const entry of roster) {
          if (entry.context) next[entry.pane] = entry.context;
        }
        setGauges(next);
      } catch {
        // A failed sample (fleet not bootstrapped yet, a transient IPC hiccup)
        // must read as "figures unavailable," never crash the shell over a
        // read-only instrument — the next tick tries again.
      }
    };

    void poll();
    const id = window.setInterval(() => void poll(), POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [active]);

  return gauges;
}
