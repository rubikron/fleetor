// The render probe behind `tests/gauge_unavailable_renders.rs` (#48, C63).
//
// It imports the **real** `PaneGauge` out of `ui/src/`, renders it once per
// reading through React's own server renderer, and prints the resulting markup
// as JSON on stdout. Nothing here knows what any of those readings look like:
// every string the Rust test asserts on has to come out of the component, which
// is what makes the assertions behavioural rather than a second spelling of the
// source.
//
// **This is not a JavaScript test runner and must not become one** (C24). There
// is no describe/it, no assertion library, no watcher and no config file — the
// runner is `cargo test`, and this is a subprocess it shells out to, the same
// shape `tests/vendor_binary_tier.rs` uses for its python probes. It asserts
// nothing; it renders and prints.

import { renderToStaticMarkup } from "react-dom/server";
import { PaneGauge } from "../../../ui/src/components/PaneGauge";
import type { GaugeReading } from "../../../ui/src/fleet/types";

/// A figure the backend actually produced, with a percent nothing rounds here.
/// The numbers are arbitrary; what matters is that the rendered text contains
/// them only if the component put them there.
const SAMPLED: GaugeReading = {
  kind: "sampled",
  gauge: { used_tokens: 42_000, window_tokens: 100_000, pct: 42 },
};

/// Rendered exactly as the two call sites render it: a pane head passes its own
/// `mono pane__meta` classes, the tab strip passes none.
const pane = (reading: GaugeReading | undefined) =>
  renderToStaticMarkup(
    <PaneGauge reading={reading} block="pane__gauge" className="mono pane__meta" />,
  );
const tab = (reading: GaugeReading | undefined) =>
  renderToStaticMarkup(<PaneGauge reading={reading} block="tab__gauge" />);

process.stdout.write(
  JSON.stringify(
    {
      pane_sampled: pane(SAMPLED),
      pane_unavailable: pane({ kind: "unavailable" }),
      pane_pending: pane({ kind: "pending" }),
      pane_out_of_scope: pane({ kind: "out-of-scope" }),
      // A call site that has been told nothing at all. Must not be silence.
      pane_undefined: pane(undefined),
      tab_unavailable: tab({ kind: "unavailable" }),
      tab_sampled: tab(SAMPLED),
    },
    null,
    2,
  ),
);
