import { useState, useEffect, useRef } from "react";
import { pickTarget, setTarget } from "../fleet/api";
import {
  DEFAULT_YOUR_LOGIN,
  ORCH,
  WORKER_SLOTS,
  workerPane,
  type FleetConfig,
  type RunRecord,
} from "../fleet/types";
import { useSeatPickers } from "../ui/useSeatPickers";
import { SeatRow, CredentialSourcePicker } from "./StartGate";

const DEBOUNCE_MS = 600;

interface HomepageProps {
  config: FleetConfig | null;
  runs: RunRecord[];
  onStart: () => void;
  onTargetChanged: () => void;
}

export function Homepage({ config, runs, onStart, onTargetChanged }: HomepageProps) {
  const [configExpanded, setConfigExpanded] = useState(false);
  const [picking, setPicking] = useState(false);
  const [pickError, setPickError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [draft, setDraft] = useState<string | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pickers = useSeatPickers();

  const targetPath = config?.target_path ?? "";
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

  const gate = pickers.gate;
  const workers = gate?.seats.workers ?? [];
  const workerDefault = gate?.worker_model_default ?? "…";
  const workerHarnesses = [...new Set(workers.map((s) => s.harness))];
  const refusals = gate?.verdict.refusals ?? [];

  const configSummary = gate
    ? `${gate.seats.orch.harness} orch · ${workerHarnesses.join(", ")} workers · ${workers.length} seats`
    : pickers.loading
      ? "loading…"
      : "unavailable";

  return (
    <div className="homepage">
      <div className="homepage__sessions">
        <h3 className="homepage__heading">Recent sessions</h3>
        {runs.length === 0 ? (
          <p className="homepage__empty">No past sessions yet.</p>
        ) : (
          <ul className="homepage__run-list">
            {runs.slice(0, 10).map((run) => (
              <li key={run.id} className="homepage__run">
                <span className="homepage__run-label">{run.label || run.id.slice(0, 8)}</span>
                <span className="homepage__run-meta mono">
                  {run.messages} msgs · {run.events} events
                  {run.target ? ` · ${run.target}` : ""}
                </span>
              </li>
            ))}
          </ul>
        )}
      </div>
      <div className="homepage__launch">
        <span className="brand" style={{ fontSize: "1.5rem" }}>FLEETOR</span>
        {config && (
          <span className="homepage__branch mono">{config.branch}</span>
        )}
        <div className="homepage__target">
          <input
            className="homepage__target-input mono"
            type="text"
            value={shown}
            spellCheck={false}
            autoCorrect="off"
            autoCapitalize="off"
            placeholder="/path/to/repo"
            aria-label="Target folder"
            disabled={saving}
            onChange={(e) => onInput(e.target.value)}
          />
          <button
            className="homepage__target-pick"
            onClick={() => void choose()}
            disabled={picking || saving}
          >
            {picking ? "…" : "Choose folder…"}
          </button>
        </div>
        {pickError && <p className="homepage__error">{pickError}</p>}

        <button
          className="homepage__config-toggle"
          onClick={() => setConfigExpanded(!configExpanded)}
          aria-expanded={configExpanded}
        >
          {configExpanded ? "▾" : "▸"} {configSummary}
        </button>

        {configExpanded && gate && (
          <ul className="homepage__config pane-gate__facts">
            <SeatRow
              label="orchestrator"
              gate={gate}
              seat={gate.seats.orch}
              sentinel={DEFAULT_YOUR_LOGIN}
              onHarness={(h) => pickers.chooseHarness(ORCH, h)}
              onModel={(m) => pickers.chooseModel(ORCH, m)}
            />
            <CredentialSourcePicker
              chosen={pickers.workersCredential}
              withALogin={gate.harnesses_with_a_login}
              hasFleetKey={gate.has_fleet_key}
              spending={workerHarnesses}
              onChoose={pickers.chooseWorkersCredential}
            />
            {WORKER_SLOTS.map((slot) => {
              const seat = workers[slot - 1];
              if (!seat) return null;
              return (
                <SeatRow
                  key={slot}
                  label={`worker ${slot}`}
                  gate={gate}
                  seat={seat}
                  sentinel={seat.credential === "plan" ? DEFAULT_YOUR_LOGIN : workerDefault}
                  onHarness={(h) => pickers.chooseHarness(workerPane(slot), h)}
                  onModel={(m) => pickers.chooseModel(workerPane(slot), m)}
                  onCredential={(c) => pickers.chooseCredential(workerPane(slot), c)}
                  showFacts={workers[slot - 2]?.harness !== seat.harness}
                />
              );
            })}
            <li className="pane-gate__fact">
              <span className="k">harness</span>
              <span className="v">change in Settings</span>
            </li>
          </ul>
        )}

        {pickers.error && <p className="homepage__error">{pickers.error}</p>}

        {(gate?.verdict.refusals ?? []).map((r) => (
          <p key={r.seat} className="homepage__error">
            {r.seat}: {r.reason}
          </p>
        ))}

        <div className="homepage__actions">
          <button
            className="homepage__start"
            onClick={onStart}
            disabled={refusals.length > 0}
          >
            New fleet
          </button>
          <button
            className="homepage__recheck"
            onClick={pickers.recheck}
            disabled={pickers.rechecking || pickers.loading}
          >
            {pickers.rechecking ? "Re-checking…" : "Re-check logins"}
          </button>
        </div>
      </div>
    </div>
  );
}
