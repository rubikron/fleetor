import type { ContextGauge } from "../fleet/types";

// Shared between the pane head and the worker tab strip so a gauge's color
// and its text never drift apart between the two places it's shown — the
// same reason statusTone.ts exists for pane state. Mirrors the backend's own
// NOTICE_THRESHOLD_PCT (`src-tauri/src/context_gauge.rs`): informational only,
// never a color standing in for a threshold that acts.
const NOTICE_THRESHOLD_PCT = 80;

/// `gold` once a pane nears/crosses the point the backend would have sent its
/// one-time Notice about; `muted` otherwise. Never `accent` (coral) — that
/// hue is reserved for "needs you," and an approximate, honestly-labeled
/// number is awareness, not an alarm (`building.md` §7).
export function gaugeTone(pct: number): "gold" | "muted" {
  return pct >= NOTICE_THRESHOLD_PCT ? "gold" : "muted";
}

/// The compact label every caller shows: `≈NN%`. The `≈` is not decoration —
/// it is the honesty label every displayed figure has to carry (WP-04
/// semantic criteria), since this is read from a transcript that may lag.
export function gaugeLabel(gauge: ContextGauge): string {
  return `≈${gauge.pct}%`;
}

/// The longer hover text: what the number means and where it came from.
export function gaugeTitle(gauge: ContextGauge): string {
  return `≈${gauge.used_tokens.toLocaleString()} / ${gauge.window_tokens.toLocaleString()} tokens — from its transcript, may lag`;
}
