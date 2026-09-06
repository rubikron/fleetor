// A pane's head, and the mark that says which harness is behind it (#50, C56).
//
// **Extracted out of `TerminalPane` so it can be rendered without a pty.**
// `TerminalPane` mounts an xterm against a real terminal, so nothing can render it
// under `node`; the head is the half this ticket changes, and pulling it out is what
// lets `src-tauri/tests/pane_head_renders.rs` assert on markup React actually
// produced rather than on the source that was supposed to produce it (C63, C24).
// `PaneGauge` was extracted for the same reason and is used by both call sites here.
//
// **Nothing in this file knows a vendor's name.** The harness's name, its mark and
// its model all arrive on the wire, on the spawn event, as one `PaneIdentity` — so
// registering a third harness changes what these render and not how. A branch on the
// harness name would be the archaeology the seam exists to end (C57), and
// `src-tauri/tests/gate_pickers.rs` fails the build if one appears.

import { statusTone, STATUS_LABEL } from "../lib/statusTone";
import { PaneGauge } from "./PaneGauge";
import type { GaugeReading, PaneIdentity, PaneStatus } from "../fleet/types";

interface HarnessMarkProps {
  /// What this pane was placed as, or `undefined` for a pane that has not
  /// spawned. **Absent renders nothing** — there is no placeholder mark and no
  /// default harness, because a pane whose harness is unknown saying `claude` is
  /// the bug this ticket exists to fix: a wrong vendor is worse than no vendor.
  identity?: PaneIdentity;
  /// The block this mark belongs to, so the head and the tab size it themselves
  /// without either restating what a mark *is* (`PaneGauge`'s arrangement).
  block: string;
}

/// **The per-harness mark**, so a mixed fleet is told apart at a glance rather than
/// by reading.
///
/// The glyph is `HarnessSpec::mark` and comes off the wire; this component chooses
/// nothing about it beyond where it sits. A harness that supplies no mark — an older
/// run replayed out of the log — renders none rather than a substitute.
export function HarnessMark({ identity, block }: HarnessMarkProps) {
  if (!identity?.mark) return null;
  return (
    <span className={`harness-mark ${block}`} title={identity.harness} aria-hidden="true">
      {identity.mark}
    </span>
  );
}

interface PaneHeadProps {
  /// The pane's own name — `orchestrator`, `worker-2`. The harness is *not* part
  /// of it: the name is this interface's, the harness is the fleet's answer.
  label: string;
  status: PaneStatus;
  identity?: PaneIdentity;
  gauge?: GaugeReading;
  /// Rendered as a control only once the fleet has started.
  started?: boolean;
  onRestart?: () => void;
}

/// One pane's head: which pane, what it is running, how full it is, and how it is.
///
/// **The harness and the model are one fact with two halves and both come from the
/// spawn event.** Before #50 the head printed a hardcoded vendor name beside a model
/// read off the fleet-wide launch config, so a codex worker announced itself as
/// `worker-2 · claude deepseek-v4-flash` — the model right, the vendor wrong. An
/// unspawned pane now shows its name and its status and nothing else, which is the
/// same rule `PaneGauge` keeps about a figure nobody has read.
export function PaneHead({
  label,
  status,
  identity,
  gauge,
  started,
  onRestart,
}: PaneHeadProps) {
  return (
    <div className="pane__head">
      <span className={`dot dot--${statusTone(status)}`} />
      <span className="mono pane__title">{label}</span>
      {identity && (
        <span className="pane__harness">
          <HarnessMark identity={identity} block="pane__mark" />
          <span className="mono pane__meta">{identity.harness}</span>
          {identity.model && <span className="mono pane__meta">{identity.model}</span>}
        </span>
      )}
      <PaneGauge reading={gauge} block="pane__gauge" className="mono pane__meta" />
      <span className="grow" style={{ flex: "1 1 auto" }} />
      <span className="pane__status">{STATUS_LABEL[status]}</span>
      {started && onRestart && (
        <button className="pane__ctl" onClick={onRestart}>
          Restart
        </button>
      )}
    </div>
  );
}
