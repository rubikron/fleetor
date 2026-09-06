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
import { ORCH, OUT_OF_SCOPE, type GaugeReading, type PaneId } from "./types";

/// Slow on purpose: a live gauge that lagged a few seconds behind a fast-
/// moving pane is still useful, and every tick is a filesystem read per
/// worker on the Rust side.
const POLL_MS = 10_000;

/// Pane → what its rail says. A missing entry means [`pending`]: nothing has
/// been read about that pane yet — never 0%, and never `unavailable`, which is
/// a claim about a reading that was actually attempted.
export type ContextGaugeMap = Partial<Record<PaneId, GaugeReading>>;

export type { GaugeReading } from "./types";
export { PENDING, OUT_OF_SCOPE } from "./types";

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
          // The orchestrator is on the roster and always carries
          // `context: None`, but that is scope rather than a failed reading —
          // recording it as `unavailable` would put the word on the one pane
          // it is untrue of.
          if (entry.pane === ORCH) {
            next[entry.pane] = OUT_OF_SCOPE;
            continue;
          }
          // The backend answered about this pane. A row with no context is a
          // reading that was attempted and produced nothing — which is what
          // `unavailable` means, and the only thing that entitles the rail to
          // say the word.
          next[entry.pane] = entry.context
            ? { kind: "sampled", gauge: entry.context }
            : { kind: "unavailable" };
        }
        setGauges(next);
      } catch {
        // A failed sample (fleet not bootstrapped yet, a transient IPC hiccup)
        // never crashes the shell over a read-only instrument — the next tick
        // tries again. The map is left exactly as it was, so before the first
        // successful poll every pane still reads `pending`: nothing has
        // answered, which is not the same claim as `unavailable`.
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
