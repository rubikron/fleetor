// TEMPORARY diagnostic — a dev-only frame-time probe.
//
// This exists to answer one question with a number instead of a guess: when
// the sidebar rail animates, are frames actually being dropped, and how badly?
// "It doesn't feel smooth" is not something you can optimize against — the
// remaining cost could be layout, paint, or React re-rendering the tree, and
// those have completely different fixes.
//
// Read it like this, per collapse or expand:
//   - `mean` near the display's frame interval (16.7ms at 60Hz, 8.3ms on a
//     ProMotion panel) means the animation is keeping up. The problem is
//     elsewhere — most likely the duration or the easing curve.
//   - `worst` far above `mean` means one expensive frame, usually a synchronous
//     layout or a repaint of something large. Look for what runs once per
//     transition rather than what runs per frame.
//   - a high `over16` count means sustained per-frame cost — the tree is being
//     re-laid-out or repainted every frame. That is a containment or renderer
//     problem, not a timing one.
//
// **Delete this file, its call in App.tsx, and the `.frame-probe` rule in
// styles.css once the question is settled.** It is gated behind
// `import.meta.env.DEV` so it cannot reach a production build, but that is a
// safety net, not a reason to leave it lying around.

import { useEffect, useRef, useState } from "react";

/// One frame at 60Hz. Frames longer than this missed a vsync deadline on a
/// standard display; on a 120Hz panel the real budget is half this, so the
/// count below is a conservative floor rather than an exact drop count.
const FRAME_BUDGET_MS = 1000 / 60;

/// How long to sample after the trigger fires. The rail animates for 140ms and
/// the debounced refit lands ~60ms after it settles, so half a second covers
/// the whole gesture with room to spare.
const SAMPLE_WINDOW_MS = 500;

export interface FrameReport {
  label: string;
  frames: number;
  meanMs: number;
  worstMs: number;
  /// Frames that exceeded the 60Hz budget.
  over16: number;
}

/// Samples frame intervals for a fixed window each time `trigger` changes.
/// Returns the most recent report, or null before the first one.
export function useFrameProbe(trigger: unknown, label: string): FrameReport | null {
  const [report, setReport] = useState<FrameReport | null>(null);
  const isFirstRun = useRef(true);

  useEffect(() => {
    // The initial mount has no transition to measure — only profile changes.
    if (isFirstRun.current) {
      isFirstRun.current = false;
      return;
    }

    const deltas: number[] = [];
    const started = performance.now();
    let last = started;
    let raf = 0;
    let stopped = false;

    const tick = (now: number) => {
      deltas.push(now - last);
      last = now;

      if (now - started < SAMPLE_WINDOW_MS) {
        raf = requestAnimationFrame(tick);
        return;
      }
      if (stopped) return;

      // Drop the first interval: it spans the state change itself, so it
      // measures React's render pass rather than a frame of the animation.
      const samples = deltas.slice(1);
      if (samples.length === 0) return;

      const total = samples.reduce((sum, d) => sum + d, 0);
      setReport({
        label,
        frames: samples.length,
        meanMs: total / samples.length,
        worstMs: Math.max(...samples),
        over16: samples.filter((d) => d > FRAME_BUDGET_MS).length,
      });
    };

    raf = requestAnimationFrame(tick);

    return () => {
      stopped = true;
      cancelAnimationFrame(raf);
    };
  }, [trigger, label]);

  return report;
}
