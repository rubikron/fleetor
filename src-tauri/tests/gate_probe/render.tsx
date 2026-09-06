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
// **Since #51 it also renders the way out of a harness that cannot take a seat**
// (C72). Five of the six harnesses below are registered in no build and named in
// no `ui/` file — which is how "registering a third harness needs no `ui/` change"
// is asserted here rather than claimed: the card renders each one's own
// instruction without ever having been edited for any of them.
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
  guidance: null,
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
  /// The first of the three not-usable shapes: no credential at all, and the
  /// vendor's own login is what fixes it. One command, no second step.
  guidance: {
    sentence:
      "beta-cli keeps its credential in a place only its own login writes, and FLEETOR " +
      "writes no credential and asks you for none. In a terminal of your own, run:",
    command: "beta-cli login",
    then: null,
    variable: null,
  },
  version: null,
  provider: null,
  models: [],
  readings_agree: true,
  resolved: null,
};

/// The same shape, for a harness whose login is a command **inside its own
/// session** rather than a subcommand — the second slot, which one of the two
/// registered harnesses needs and the other does not.
const TWO_STEP: HarnessOffer = {
  ...REFUSED,
  name: "gamma-cli",
  invoked: "gamma-cli",
  reason: "no session recorded.",
  guidance: {
    sentence: "In a terminal of your own, run:",
    command: "gamma-cli",
    then: "/signin",
    variable: null,
  },
};

/// **The second not-usable shape**: a provider of the operator's own whose key is
/// not set. No login command helps, so the guidance names a variable and no
/// command at all — the distinction this ticket exists to make.
const PROVIDER_KEY: HarnessOffer = {
  ...REFUSED,
  name: "delta-cli",
  invoked: "delta-cli",
  reason: "the configured provider's credential is missing.",
  guidance: {
    sentence:
      "No login command will help here: this machine resolves delta-cli to the operator's " +
      "own, a provider of your own, and the key it names is not set. Set this in the " +
      "environment FLEETOR is launched from:",
    command: null,
    then: null,
    variable: "OPERATORS_SHELL_KEY",
  },
};

/// **The third shape**: a working installation whose diagnostic this build could
/// not parse. It is *not* a refusal — the seat is still offered — and telling this
/// operator to log in would be telling them to fix something that works.
const UNREADABLE: HarnessOffer = {
  ...REFUSED,
  name: "epsilon-cli",
  invoked: "epsilon-cli",
  status: "unreadable",
  reason: "its report is not readable as JSON.",
  guidance: {
    sentence:
      "Nothing to log in to: this is a working epsilon-cli installation whose diagnostic " +
      "this build could not read, not a logged-out one. A seat on it is still offered, and " +
      "logging in again would change nothing.",
    command: null,
    then: null,
    variable: null,
  },
};

/// A harness that cannot take a seat and about which the machine has **nothing to
/// suggest** — the not-installed state, where the reason already carries the only
/// actionable sentence there is. Its row must render no instruction rather than an
/// empty one.
const NO_GUIDANCE: HarnessOffer = {
  ...REFUSED,
  name: "zeta-cli",
  invoked: "zeta-cli",
  status: "not-installed",
  reason: "`zeta-cli` is not on the PATH this app was launched with.",
  guidance: null,
};

const GATE: GateState = {
  harnesses: [RUNNABLE, REFUSED, TWO_STEP, PROVIDER_KEY, UNREADABLE, NO_GUIDANCE],
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
      // The four guidance shapes (#51), each on a harness this build registers
      // nowhere and no `ui/` file names. A component that chose its wording by
      // asking which harness this is would render every one of these blank.
      guidance_login_command: row({
        label: "worker 1",
        gate: GATE,
        seat: { harness: REFUSED.name, model: null },
        sentinel: "deepseek-v4-flash",
        onHarness: noop,
        onModel: noop,
      }),
      guidance_two_step: row({
        label: "worker 1",
        gate: GATE,
        seat: { harness: TWO_STEP.name, model: null },
        sentinel: "deepseek-v4-flash",
        onHarness: noop,
        onModel: noop,
      }),
      guidance_provider_key: row({
        label: "worker 1",
        gate: GATE,
        seat: { harness: PROVIDER_KEY.name, model: null },
        sentinel: "deepseek-v4-flash",
        onHarness: noop,
        onModel: noop,
      }),
      guidance_unreadable: row({
        label: "worker 1",
        gate: GATE,
        seat: { harness: UNREADABLE.name, model: null },
        sentinel: "deepseek-v4-flash",
        onHarness: noop,
        onModel: noop,
      }),
      // A refused harness the machine has nothing to suggest about: the row must
      // carry no instruction rather than an empty one.
      guidance_absent: row({
        label: "worker 1",
        gate: GATE,
        seat: { harness: NO_GUIDANCE.name, model: null },
        sentinel: "deepseek-v4-flash",
        onHarness: noop,
        onModel: noop,
      }),
    },
    null,
    2,
  ),
);
