// The render probe behind `tests/gate_seat_row_renders.rs` (WP-26; C63, C24).
//
// It imports the **real** `SeatRow` out of `ui/src/components/StartGate.tsx` — the
// one component both the orchestrator row and every worker row are — and renders it
// through React's own server renderer against synthetic harness data. Nothing here
// knows what the markup looks like; every string the Rust test asserts on has to
// come out of the component, which is what makes the assertions behavioural rather
// than a second spelling of the source. `gate_pickers.rs` already proves the JSX
// *says* the harness list is never filtered and that a disabled option carries its
// reason; this proves it renders that way, closing the exact gap #35 left open once
// before (see that file's own doc comment): a screen can pass every source-reading
// check and still be unreachable, or restructured into something that no longer
// mounts the way the source implies.
//
// **This is not a JavaScript test runner and must not become one** (C24). No
// describe/it, no assertion library, no watcher, no config file — the runner is
// `cargo test`, and this is a subprocess it shells out to, the same shape
// `tests/gauge_probe/render.tsx` and `tests/vendor_binary_tier.rs`'s python probes
// use. It renders and prints; it asserts nothing.

import { renderToStaticMarkup } from "react-dom/server";
import { SeatRow } from "../../../ui/src/components/StartGate";
import type { GateState, HarnessOffer } from "../../../ui/src/fleet/types";

/// A harness that can take a seat, with a catalog carrying one long model name —
/// the field the shipped card clipped to `deepsee`.
const RUNNABLE: HarnessOffer = {
  name: "alpha-cli",
  invoked: "alpha-cli",
  status: "logged-in",
  account: "Pro Plan",
  reason: null,
  caveat: null,
  version: "4.2.0",
  provider: "alpha-cloud",
  models: [
    { slug: "deepseek-v4-flash", display_name: "DeepSeek v4 Flash" },
    { slug: "deepseek-v4-flash-preview", display_name: "DeepSeek v4 Flash (preview)" },
  ],
  readings_agree: true,
  resolved: null,
};

/// A harness that cannot take a seat, carrying the vendor's own reason — the state
/// user story 9 says must stay in the list, disabled, rather than be dropped.
const REFUSED: HarnessOffer = {
  name: "beta-cli",
  invoked: "beta-cli",
  status: "no-credential",
  account: null,
  reason: "no OAuth token found in ~/.beta/auth.json — run `beta-cli login` and try again.",
  caveat: null,
  version: null,
  provider: null,
  models: [],
  readings_agree: true,
  resolved: null,
};

const GATE: GateState = {
  harnesses: [RUNNABLE, REFUSED],
  seats: {
    orch: { harness: RUNNABLE.name, model: null },
    workers: [
      { harness: RUNNABLE.name, model: null },
      { harness: RUNNABLE.name, model: null },
      { harness: REFUSED.name, model: null },
      { harness: RUNNABLE.name, model: null },
    ],
  },
  worker_model_default: "deepseek-v4-flash",
  verdict: { refusals: [], cost: [], fallbacks: [] },
};

const noop = () => {};

/// Wrapped in a `<ul>`: `SeatRow` renders an `<li>`, and `renderToStaticMarkup` on a
/// bare `<li>` is legal React but is not the DOM shape the real card mounts it into.
const row = (props: Parameters<typeof SeatRow>[0]) =>
  renderToStaticMarkup(
    <ul>
      <SeatRow {...props} />
    </ul>,
  );

process.stdout.write(
  JSON.stringify(
    {
      // The orchestrator's shape: an absent model shown as the sentinel
      // placeholder, never a value.
      orchestrator: row({
        label: "orchestrator",
        gate: GATE,
        seat: { harness: RUNNABLE.name, model: null },
        sentinel: "default (your login)",
        onHarness: noop,
        onModel: noop,
        aside: "Runs your own login, and the provider it resolved — inherited and displayed, never picked.",
      }),
      // A seat with an explicit model long enough that a fixed-width field used
      // to clip it — the full string has to survive into the `value` attribute.
      worker_named_model: row({
        label: "worker 1",
        gate: GATE,
        seat: { harness: RUNNABLE.name, model: "deepseek-v4-flash-preview" },
        sentinel: "deepseek-v4-flash",
        onHarness: noop,
        onModel: noop,
      }),
      // A seat currently on the harness that cannot take one — the state the
      // shipped card handled worst, per the redesign brief.
      worker_on_refused_harness: row({
        label: "worker 3",
        gate: GATE,
        seat: { harness: REFUSED.name, model: null },
        sentinel: "deepseek-v4-flash",
        onHarness: noop,
        onModel: noop,
      }),
      // The same row, so the probe cannot fake the refused case above by using a
      // gate with only one harness — this one proves the healthy harness would
      // still have rendered as an option had it been picked.
      worker_on_runnable_harness: row({
        label: "worker 4",
        gate: GATE,
        seat: { harness: RUNNABLE.name, model: null },
        sentinel: "deepseek-v4-flash",
        onHarness: noop,
        onModel: noop,
      }),
    },
    null,
    2,
  ),
);
