// The spend gate: nothing runs until the operator starts the fleet.
//
// This is the one screen whose entire job is telling the operator what a click
// will cost, so it may not round anything up. It states the models that will
// actually run, the directory they will run in, and — when there is no key — that
// the workers will not start at all, rather than offering four seats that fail on
// spawn. The predecessor to this card rendered any worker backend that was not
// "flash" as "fake (free proof path)", promising a free path that no longer
// existed; that is the exact failure this file exists to not repeat.

import { useState, useEffect, useRef } from "react";
import { pickTarget, setTarget } from "../fleet/api";
import { WORKER_SLOTS, type FleetConfig } from "../fleet/types";

const DEBOUNCE_MS = 600;

interface StartGateProps {
  config: FleetConfig | null;
  onStart: () => void;
  onTargetChanged: () => void;
}

export function StartGate({ config, onStart, onTargetChanged }: StartGateProps) {
  const [picking, setPicking] = useState(false);
  const [pickError, setPickError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [draft, setDraft] = useState<string | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const targetPath = config?.target_path ?? "";
  const lead = config?.lead_model ?? "opus (operator)";
  const backend = config?.worker_backend ?? "…";
  const hasWorkers = backend !== "none" && backend !== "…";

  const shown = draft ?? targetPath;

  const commit = async (value: string) => {
    const trimmed = value.trim();
    if (!trimmed || trimmed === targetPath) return;
    setSaving(true);
    setPickError(null);
    try {
      await setTarget(trimmed);
      setDraft(null);
      onTargetChanged();
    } catch (e) {
      setPickError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const onInput = (value: string) => {
    setDraft(value);
    setPickError(null);
    if (timerRef.current) clearTimeout(timerRef.current);
    timerRef.current = setTimeout(() => void commit(value), DEBOUNCE_MS);
  };

  useEffect(() => {
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current);
    };
  }, []);

  const choose = async () => {
    setPicking(true);
    setPickError(null);
    try {
      const picked = await pickTarget();
      if (picked) {
        setDraft(null);
        onTargetChanged();
      }
    } catch (e) {
      setPickError(String(e));
    } finally {
      setPicking(false);
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
              onChange={(e) => onInput(e.target.value)}
              onWheel={(e) => {
                const el = e.currentTarget;
                const max = el.scrollWidth - el.clientWidth;
                if (max <= 0) return;
                const delta =
                  Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
                if (delta === 0) return;
                el.scrollLeft = Math.min(max, Math.max(0, el.scrollLeft + delta));
              }}
              onKeyDown={(e) => {
                if (e.key === "Escape") {
                  e.preventDefault();
                  setDraft(null);
                  setPickError(null);
                  if (timerRef.current) clearTimeout(timerRef.current);
                }
              }}
            />
          </li>
        </ul>
        {pickError && <p className="pane-gate__error">{pickError}</p>}
        <div className="pane-gate__actions">
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
      </div>
    </div>
  );
}
