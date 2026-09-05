// The Critic's interview, as the UI sees it (WP-21 stage A).
//
// Deliberately the `useDevMode.ts` shape, and for the same reason: the flag
// lives on the Rust side, because it decides whether `fleet send orch` from
// inside the Critic *resolves*, and that question is asked by code with no
// webview. This hook is a view of that state, never a second copy of it.
//
// Three states, and the third is the one that matters most here: open, closed,
// and not-yet-answered. Rendering "closed" while the answer is in flight would
// tell the operator no turns are being spent at a moment when they might be —
// the opposite of what this control exists to say. So the switch waits for a
// real answer instead, and is inert until it has one.
//
// **No optimistic flip.** The switch moves when the write lands, not when it is
// requested. An interview that shows "open" over a run where the gate did not
// actually open is a control lying about what the fleet's turns are being spent
// on, which is worse than a control that did not move.
//
// One deliberate divergence from `useDevMode`: dev mode is answerable from
// launch, but the interview is a property of a *run*. So the read is keyed on
// `active` — the fleet being up — and re-asks when a run appears, rather than
// asking once at mount and caching whatever a runless backend said.

import { useCallback, useEffect, useState } from "react";
import { fetchCriticInterview, setCriticInterview } from "../fleet/api";

export interface CriticInterviewControls {
  /// `null` until the backend has answered — neither open nor closed yet.
  open: boolean | null;
  /// Persist the opposite of the current state. A failure leaves the switch
  /// where it was and puts a sentence in `error`; it never optimistically
  /// renders a gate that did not actually move.
  toggle: () => void;
  /// Why the last toggle did not take, if it did not.
  error: string | null;
}

function why(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : "the interview could not be changed";
}

/// `active` is whether there is a run to interview. While it is false the hook
/// holds `null`: there is nothing to ask about, and claiming "closed" would be
/// asserting a fact about a run that does not exist.
export function useCriticInterview(active: boolean): CriticInterviewControls {
  const [open, setOpen] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!active) {
      setOpen(null);
      setError(null);
      return;
    }
    let live = true;
    fetchCriticInterview()
      .then((value) => {
        if (live) setOpen(value);
      })
      .catch((e: unknown) => {
        if (!live) return;
        // An unanswerable gate is closed on the Rust side too — but say why, or
        // a switch that will not move looks like a broken switch.
        setOpen(false);
        setError(why(e));
      });
    return () => {
      live = false;
    };
  }, [active]);

  const toggle = useCallback(() => {
    if (open === null) return; // nothing to flip yet
    setError(null);
    setCriticInterview(!open)
      .then((stored) => setOpen(stored))
      .catch((e: unknown) => setError(why(e)));
  }, [open]);

  return { open, toggle, error };
}
