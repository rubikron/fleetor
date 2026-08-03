// The spend gate: nothing runs until the operator starts the fleet.
//
// This is the one screen whose entire job is telling the operator what a click
// will cost, so it may not round anything up. It states the models that will
// actually run, the directory they will run in, and — when there is no key — that
// the workers will not start at all, rather than offering four seats that fail on
// spawn. The predecessor to this card rendered any worker backend that was not
// "flash" as "fake (free proof path)", promising a free path that no longer
// existed; that is the exact failure this file exists to not repeat.

import { useState } from "react";
import { pickTarget, setTarget } from "../fleet/api";
import { WORKER_SLOTS, type FleetConfig } from "../fleet/types";

interface StartGateProps {
  config: FleetConfig | null;
  onStart: () => void;
  onTargetChanged: () => void;
}

export function StartGate({ config, onStart, onTargetChanged }: StartGateProps) {
  const [picking, setPicking] = useState(false);
  const [pickError, setPickError] = useState<string | null>(null);
  /// What is in the box. Held separately from `config.target_path` so a
  /// half-typed path is never written, and so an in-progress edit is not wiped
  /// by a config refresh landing mid-keystroke.
  const [draft, setDraft] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const targetPath = config?.target_path ?? "";
  const lead = config?.lead_model ?? "opus (operator)";
  const backend = config?.worker_backend ?? "…";
  const hasWorkers = backend !== "none" && backend !== "…";

  // `draft === null` means "showing whatever the backend reports", so a config
  // refresh flows straight through. Once the operator types, the draft owns the
  // field until it is committed or abandoned.
  const shown = draft ?? targetPath;
  const isDirty = draft !== null && draft.trim() !== targetPath;

  const choose = async () => {
    setPicking(true);
    setPickError(null);
    try {
      const picked = await pickTarget();
      if (picked) {
        setDraft(null); // fall back to whatever the refreshed config reports
        onTargetChanged();
      }
    } catch (e) {
      setPickError(String(e));
    } finally {
      setPicking(false);
    }
  };

  /// Commit a typed path. The backend is the only thing that decides whether a
  /// path is usable, so its rejection message is shown verbatim rather than
  /// second-guessed here.
  const commit = async () => {
    if (draft === null || !isDirty) {
      setDraft(null);
      return;
    }
    setSaving(true);
    setPickError(null);
    try {
      await setTarget(draft);
      setDraft(null); // show the canonical path the backend resolved
      onTargetChanged();
    } catch (e) {
      setPickError(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="pane-gate pane-gate--workspace">
      <div className="pane-gate__card">
        <h3 className="pane-gate__title">Start the fleet</h3>
        <p className="pane-gate__body">
          Spawns your real <span className="mono">claude</span> as the orchestrator
          {hasWorkers ? ` and ${WORKER_SLOTS.length} worker terminals` : ""}, all wired to the
          hub. It will spend tokens.
        </p>
        <ul className="pane-gate__facts">
          <li>
            <span className="k">orchestrator</span>
            <span className="v v--gold">{lead}</span>
          </li>
          <li>
            <span className="k">workers</span>
            <span className={`v ${hasWorkers ? "v--gold" : ""}`}>
              {hasWorkers
                ? `${WORKER_SLOTS.length} × ${backend}`
                : "none — no API key found, the orchestrator runs alone"}
            </span>
          </li>
          <li>
            <span className="k">target</span>
            {/* An input, not a span: it is editable *and* it scrolls its own
                overflow horizontally, so a long path neither runs off the card
                nor has to be truncated away from the operator. */}
            <input
              className="v pane-gate__path"
              type="text"
              value={shown}
              spellCheck={false}
              autoCorrect="off"
              autoCapitalize="off"
              placeholder="/path/to/repo"
              aria-label="Target folder"
              aria-invalid={pickError !== null}
              disabled={saving}
              onChange={(e) => setDraft(e.target.value)}
              onBlur={() => void commit()}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  e.currentTarget.blur(); // commit runs on the resulting blur
                } else if (e.key === "Escape") {
                  e.preventDefault();
                  setDraft(null); // abandon the edit, snap back to the real value
                  setPickError(null);
                }
              }}
            />
          </li>
        </ul>
        {pickError && <p className="pane-gate__error">{pickError}</p>}
        <div className="pane-gate__actions">
          {isDirty && !pickError && (
            <span className="pane-gate__hint">press enter to set · esc to cancel</span>
          )}
          <button
            className="pane-gate__alt"
            onClick={() => void choose()}
            disabled={picking || saving}
          >
            {picking ? "Choosing…" : "Choose folder…"}
          </button>
          <button className="pane-gate__go" onClick={onStart}>
            Start fleet
          </button>
        </div>
        <p className="pane-gate__note">
          A new folder takes effect the next time the fleet starts — panes already have a working
          directory.
        </p>
      </div>
    </div>
  );
}
