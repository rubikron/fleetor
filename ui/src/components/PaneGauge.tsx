// One pane's context reading, wherever it is shown (#48).
//
// **Why a component and not two ternaries.** The gauge appears twice — in a
// pane head and in the worker tab strip — and before this each site guarded on
// bare truthiness and spelled its own markup. That is how "unavailable" came to
// be rendered as nothing in both places at once: there was no renderer to teach,
// only two guards that both fell through. `contextGaugeTone.ts` already exists
// for exactly this argument about colour and text; this is the same argument
// about the element.
//
// **Silence is a decision made once, here.** `gaugeView` returns `null` for the
// orchestrator alone — the pane with no gauge by construction. Every other
// reading, including the ones carrying no number, renders something the operator
// can read.

import { gaugeView } from "../lib/contextGaugeTone";
import type { GaugeReading } from "../fleet/types";

interface PaneGaugeProps {
  /// This pane's reading. `undefined` is not a third way of saying nothing — it
  /// resolves to `pending` inside `gaugeView`, so a caller that has not been
  /// told anything yet still says so.
  reading: GaugeReading | undefined;
  /// Which block name the tone class hangs off — `pane__gauge--muted` in a pane
  /// head, `tab__gauge--muted` in the tab strip. The two sites keep their own
  /// typography and share everything else.
  block: "pane__gauge" | "tab__gauge";
  /// Extra classes the call site already put on its span, kept so this change
  /// is a swap of the element rather than a restyle of it.
  className?: string;
}

export function PaneGauge({ reading, block, className }: PaneGaugeProps) {
  const view = gaugeView(reading);
  if (!view) return null;
  return (
    <span
      className={`${className ? `${className} ` : ""}${block} ${block}--${view.tone}`}
      title={view.title}
    >
      {view.text}
    </span>
  );
}
