// Straight from `fleet/types`, never from `fleet/useContextGauge`: this
// module is imported by a component that is server-rendered in a test, and the
// hook module pulls in the Tauri IPC bridge.
import { PENDING, type ContextGauge, type GaugeReading } from "../fleet/types";

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

// --- what the rail actually says, per reading (#48) ----------------------------

/// The qualifier every state wears, so the three visible answers occupy one
/// shape in the pane head and the tab strip rather than three. Without it
/// `unavailable` on its own is a word next to a status word, and nothing on
/// screen says which instrument it belongs to.
const QUALIFIER = "ctx";

/// One rendered gauge: the text, its hover, and the tone class both call sites
/// suffix onto their own block name. `null` means *render nothing* — the only
/// reading that earns silence.
export interface GaugeView {
  text: string;
  title: string;
  tone: "gold" | "muted" | "absent";
}

/// A reading, resolved to what the operator sees. **The one place the words
/// live**, for the same reason the tone and the label already lived here: the
/// pane head and the tab strip must never disagree about whether a pane's
/// usage could be read.
///
/// `undefined` resolves to [`PENDING`] rather than to nothing, which is the
/// whole of #48 in one line — an unread pane is now a state with a name
/// instead of an absent map entry that every caller re-interpreted.
///
/// **No branch here computes a figure.** `sampled` formats one the backend
/// read; the other three carry no number at all, and there is no arithmetic in
/// this function to produce one (C24, C65, M22).
export function gaugeView(reading: GaugeReading | undefined): GaugeView | null {
  const r = reading ?? PENDING;
  switch (r.kind) {
    case "sampled":
      return {
        text: `${QUALIFIER} ${gaugeLabel(r.gauge)}`,
        title: gaugeTitle(r.gauge),
        tone: gaugeTone(r.gauge.pct),
      };
    case "unavailable":
      // Says the word. The CLI's roster renders this same state as `—`
      // (`an_unsampled_pane_renders_an_em_dash_not_a_zero`); a rail with room
      // for a word uses the word C12 and M22 chose.
      return {
        text: `${QUALIFIER} unavailable`,
        title:
          "context unavailable — this pane's usage could not be read. " +
          "FLEETOR shows no figure rather than an estimated one.",
        tone: "absent",
      };
    case "pending":
      // Deliberately not a word and deliberately not `—`: this pane has not
      // been asked yet, which is a state that resolves itself on the next
      // poll, unlike `unavailable`.
      return {
        text: `${QUALIFIER} …`,
        title: "not sampled yet — nothing has read this pane's usage; the next roster poll will.",
        tone: "muted",
      };
    case "out-of-scope":
      return null;
  }
}
