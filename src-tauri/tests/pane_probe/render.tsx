// The render probe behind `tests/pane_head_renders.rs` (#50; C63, C24, C56).
//
// It imports the **real** `PaneHead` and `HarnessMark` out of
// `ui/src/components/PaneHead.tsx` — the head every one of the five panes renders,
// and the mark the worker tab strip renders beside it — and renders them through
// React's own server renderer against synthetic identities. Nothing here knows what
// the markup looks like; every string the Rust test asserts on has to come out of
// the components, which is what makes those assertions behavioural rather than a
// second spelling of the source. `gate_pickers.rs` already proves the pane chrome
// *spells* no vendor's name; this proves the head renders the one it was handed.
//
// **Every harness below is invented, and that is the acceptance criterion.**
// `alpha-cli`, `beta-cli`, `gamma-cli` and `delta-cli` are registered in no build —
// this file is the demonstration that registering a third harness needs no change
// under `ui/`, because the components render four they have never heard of. A
// component that switched on the harness name would render nothing for all four.
//
// **This is not a JavaScript test runner and must not become one** (C24). No
// describe/it, no assertion library, no watcher, no config file — the runner is
// `cargo test`, and this is a subprocess it shells out to, the same shape
// `tests/gate_probe/render.tsx` and `tests/gauge_probe/render.tsx` use. It renders
// and prints; it asserts nothing.

import { renderToStaticMarkup } from "react-dom/server";
import { HarnessMark, PaneHead } from "../../../ui/src/components/PaneHead";
import type { PaneIdentity } from "../../../ui/src/fleet/types";

/// A worker seat: a harness, its mark, and the model it was pointed at.
const SPAWNED: PaneIdentity = {
  harness: "alpha-cli",
  mark: "AC",
  model: "deepseek-v4-flash",
};

/// An attended seat, which runs the operator's own login and names no model (M2).
const ATTENDED: PaneIdentity = { harness: "beta-cli", mark: "BC" };

/// **A harness this build has never registered.** The whole of criterion 3: if the
/// head needed to know a harness to render it, this one would come out empty.
const THIRD: PaneIdentity = { harness: "gamma-cli", mark: "GC", model: "orion-2-thinking" };

/// A spawn event that carried a name and no mark — an older run replayed out of the
/// log. The harness is still named; nothing stands in for the absent mark.
const NO_MARK: PaneIdentity = { harness: "delta-cli", model: "delta-1" };

const noop = () => {};

process.stdout.write(
  JSON.stringify(
    {
      // A live worker: name, mark, harness and model.
      spawned: renderToStaticMarkup(
        <PaneHead
          label="worker-2"
          status="live"
          identity={SPAWNED}
          gauge={{ kind: "unavailable" }}
          started
          onRestart={noop}
        />,
      ),
      // The pane before anything spawned it. There is no identity to render and
      // none is invented.
      unspawned: renderToStaticMarkup(<PaneHead label="worker-3" status="idle" />),
      // The orchestrator's shape: a harness, and no model at all. Its gauge is
      // `out-of-scope`, which is the orchestrator's real reading and renders
      // nothing (C69) — so every fact in this head is one the identity supplied.
      attended: renderToStaticMarkup(
        <PaneHead
          label="orchestrator"
          status="live"
          identity={ATTENDED}
          gauge={{ kind: "out-of-scope" }}
        />,
      ),
      // A harness registered in no build, rendered by a component that has never
      // been edited for it.
      third_harness: renderToStaticMarkup(
        <PaneHead label="worker-4" status="live" identity={THIRD} />,
      ),
      // A named harness that supplied no mark.
      no_mark: renderToStaticMarkup(
        <PaneHead label="worker-1" status="live" identity={NO_MARK} />,
      ),
      // The mark's other home: the worker tab strip, which renders it alone.
      tab_mark: renderToStaticMarkup(<HarnessMark identity={THIRD} block="tab__mark" />),
      tab_mark_unspawned: renderToStaticMarkup(<HarnessMark block="tab__mark" />),
    },
    null,
    2,
  ),
);
