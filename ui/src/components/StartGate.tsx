// The spend gate: nothing runs until the operator starts the fleet.
//
// This is the one screen whose entire job is telling the operator what a click
// will cost, so it may not round anything up. It states the models that will
// actually run, the directory they will run in, and — when there is no key — that
// the workers will not start at all, rather than offering four seats that fail on
// spawn. The predecessor to this card rendered any worker backend that was not
// "flash" as "fake (free proof path)", promising a free path that no longer
// existed; that is the exact failure this file exists to not repeat.
//
// **Since WP-25 #35 it is also where each seat gets its harness and its model**
// (M1, M15, C23): one row for the orchestrator, one for the workers, and a
// disclosure that expands the workers row into four. Three things are deliberately
// not offered anywhere on this card and each is a recorded decision rather than a
// gap — **no provider picker** (the orchestrator inherits and displays, workers get
// FLEETOR's: C2 as amended by C9), **no harness for the two judges** (C15, and their
// specs have no field to hold one), and **no plan tier** (the vendor's diagnostic
// reports the account *shape*, not the tier: C14 as narrowed by C58).
//
// Every fact beside a model comes off `useSeatPickers`, which reads the backend's
// held probe and writes back through it. The summary below reads the same `seats`
// value the pickers write and the backend places against, so this card cannot
// promise a fleet that is not the one that will spawn (M15).

import { useState, useEffect, useRef } from "react";
import { pickTarget, setTarget } from "../fleet/api";
import {
  DEFAULT_YOUR_LOGIN,
  ORCH,
  WORKER_SLOTS,
  canTakeASeat,
  workerPane,
  type FleetConfig,
  type GateState,
  type HarnessOffer,
  type SeatChoice,
} from "../fleet/types";
import { offerFor, useSeatPickers } from "../ui/useSeatPickers";

const DEBOUNCE_MS = 600;

/// **What FLEETOR is and is not deciding about a model** (M16).
///
/// Said on the card rather than in a doc comment because both halves mislead an
/// operator who has not been told: the remembered choice goes stale the moment
/// somebody uses the harness's own model command, and what it remembers is what was
/// last *launched* rather than what a pane is *running*.
const WHAT_A_REMEMBERED_MODEL_IS =
  "FLEETOR picks the model a pane starts with and has no opinion after that — " +
  "changing one mid-run is the harness's own command. So what is remembered here " +
  "is what was last launched, not what a pane is running.";

interface StartGateProps {
  config: FleetConfig | null;
  onStart: () => void;
  onTargetChanged: () => void;
}

/// One harness's facts, as they read beside the model.
function factsFor(offer: HarnessOffer | undefined): string {
  if (offer === undefined) return "no harness by that name is registered in this build";
  if (offer.status !== "logged-in") return offer.reason ?? "this harness cannot take a seat";
  const version = offer.version ?? "an unreported version";
  const provider = offer.provider === null ? "" : `, provider ${offer.provider}`;
  return `${offer.name} ${version} — logged in: ${offer.account ?? "unknown shape"}${provider}`;
}

/// The label one harness wears in a picker: its name, and — where it cannot take a
/// seat — the short reason why, **inline on the option itself**.
///
/// The full sentence the vendor gave goes under the row. This is the half that has
/// to survive being collapsed into a dropdown, because an option that is greyed out
/// and says nothing is a supported feature looking unimplemented.
function optionLabel(offer: HarnessOffer): string {
  switch (offer.status) {
    case "logged-in":
      return offer.name;
    case "no-credential":
      return `${offer.name} — not logged in`;
    case "not-installed":
      return `${offer.name} — not installed`;
    case "unreadable":
      return `${offer.name} — login could not be read`;
  }
}

interface SeatRowProps {
  label: string;
  gate: GateState;
  seat: SeatChoice;
  /// The orchestrator carries M2's sentinel; a worker's unnamed model is the fleet's
  /// own, so the two rows say different things in the same place.
  sentinel: string;
  onHarness: (harness: string) => void;
  onModel: (model: string) => void;
  /// A short line under the picker, where a row needs one the facts do not carry.
  aside?: string;
}

/// One seat's harness and model, with the harness's own facts beneath them.
function SeatRow({ label, gate, seat, sentinel, onHarness, onModel, aside }: SeatRowProps) {
  const offer = offerFor(gate, seat.harness);
  const models = offer?.models ?? [];
  const listId = `models-${label.replace(/\s+/g, "-")}`;
  const [typed, setTyped] = useState<string | null>(null);
  const shown = typed ?? seat.model ?? "";

  return (
    <li className="seat-row">
      <span className="k">{label}</span>
      <div className="seat-row__controls">
        <select
          className="seat-row__harness"
          aria-label={`${label} harness`}
          value={seat.harness}
          onChange={(e) => onHarness(e.target.value)}
        >
          {/* **Never filtered.** Every registered harness is an option; the ones
              this machine cannot run are disabled and say why, rather than being
              dropped from a list where their absence reads as "unsupported". */}
          {gate.harnesses.map((harness) => (
            <option
              key={harness.name}
              value={harness.name}
              disabled={!canTakeASeat(harness)}
              title={harness.reason ?? undefined}
            >
              {optionLabel(harness)}
            </option>
          ))}
        </select>
        <input
          className="seat-row__model"
          type="text"
          list={models.length > 0 ? listId : undefined}
          spellCheck={false}
          autoCorrect="off"
          autoCapitalize="off"
          placeholder={sentinel}
          aria-label={`${label} model`}
          value={shown}
          onChange={(e) => setTyped(e.target.value)}
          onBlur={() => {
            if (typed !== null) onModel(typed);
            setTyped(null);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") e.currentTarget.blur();
            if (e.key === "Escape") setTyped(null);
          }}
        />
        {/* The vendor's own catalog, in the vendor's own order — suggestions on a
            field the operator may still type into, because a harness that publishes
            no catalog offers an empty list and a name typed by hand is the only
            answer there is. */}
        {models.length > 0 && (
          <datalist id={listId}>
            {models.map((model) => (
              <option key={model.slug} value={model.slug}>
                {model.display_name}
              </option>
            ))}
          </datalist>
        )}
        <button
          type="button"
          className="seat-row__sentinel"
          disabled={seat.model === null}
          title={`Run this seat on ${sentinel}`}
          onClick={() => {
            setTyped(null);
            onModel("");
          }}
        >
          use default
        </button>
      </div>
      <p className="seat-row__facts">{factsFor(offer)}</p>
      {offer !== undefined && offer.resolved !== null && !offer.readings_agree && (
        <p className="seat-row__warn">
          the <span className="mono">{offer.invoked}</span> on your PATH is a wrapper: it and the
          vendor binary at <span className="mono">{offer.resolved}</span> report different
          configurations, so what a pane runs under is not what this row describes.
        </p>
      )}
      {aside !== undefined && <p className="seat-row__facts">{aside}</p>}
    </li>
  );
}

export function StartGate({ config, onStart, onTargetChanged }: StartGateProps) {
  const [picking, setPicking] = useState(false);
  const [pickError, setPickError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [draft, setDraft] = useState<string | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pickers = useSeatPickers();

  const targetPath = config?.target_path ?? "";
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

  const gate = pickers.gate;
  // **Every seat named here comes off `gate.seats`**, which is what the backend
  // stored and what `spawn_pane` places against (M15).
  const workers = gate?.seats.workers ?? [];
  const firstWorker = workers[0];
  const workerDefault = gate?.worker_model_default ?? "…";
  // The caveat is a property of a *login*, so it is said once for the harnesses this
  // fleet is actually about to spend, and not once per seat.
  const spending = gate === null ? [] : selectedHarnesses(gate);
  const caveats = spending.filter((offer) => offer.caveat !== null);

  return (
    <div className="pane-gate pane-gate--workspace">
      <div className="pane-gate__card">
        <h3 className="pane-gate__title">Start the fleet</h3>
        <p className="pane-gate__body">
          Spawns the orchestrator{hasWorkers ? ` and ${WORKER_SLOTS.length} worker terminals` : ""},
          each on the harness named below, all wired to the hub. It will spend tokens.
        </p>
        <ul className="pane-gate__facts">
          {gate === null ? (
            <li>
              <span className="k">harnesses</span>
              <span className="v">
                {pickers.loading ? "asking each harness about this machine…" : "unavailable"}
              </span>
            </li>
          ) : (
            <>
              <SeatRow
                label="orchestrator"
                gate={gate}
                seat={gate.seats.orch}
                sentinel={DEFAULT_YOUR_LOGIN}
                onHarness={(harness) => pickers.chooseHarness(ORCH, harness)}
                onModel={(model) => pickers.chooseModel(ORCH, model)}
                aside="Runs your own login, and the provider it resolved — inherited and displayed, never picked."
              />
              {!hasWorkers ? (
                <li>
                  <span className="k">workers</span>
                  <span className="v">none — no API key found, the orchestrator runs alone</span>
                </li>
              ) : pickers.expanded ? (
                <>
                  {WORKER_SLOTS.map((slot) => {
                    const seat = workers[slot - 1];
                    if (seat === undefined) return null;
                    return (
                      <SeatRow
                        key={slot}
                        label={`worker ${slot}`}
                        gate={gate}
                        seat={seat}
                        sentinel={workerDefault}
                        onHarness={(harness) => pickers.chooseHarness(workerPane(slot), harness)}
                        onModel={(model) => pickers.chooseModel(workerPane(slot), model)}
                      />
                    );
                  })}
                </>
              ) : (
                firstWorker !== undefined && (
                  <SeatRow
                    label={`${WORKER_SLOTS.length} workers`}
                    gate={gate}
                    seat={firstWorker}
                    sentinel={workerDefault}
                    onHarness={pickers.chooseWorkersHarness}
                    onModel={pickers.chooseWorkersModel}
                    aside="Fenced, on FLEETOR's own provider and key — never your login."
                  />
                )
              )}
              {hasWorkers && (
                <li className="seat-disclosure">
                  <span className="k" />
                  <button
                    type="button"
                    className="seat-disclosure__toggle"
                    aria-expanded={pickers.expanded}
                    disabled={!pickers.workersAgree}
                    title={
                      pickers.workersAgree
                        ? undefined
                        : "the four workers differ, so one row cannot speak for them"
                    }
                    onClick={() => pickers.setExpanded(!pickers.expanded)}
                  >
                    {pickers.expanded
                      ? "▾ one harness for all four"
                      : "▸ give each worker its own harness"}
                  </button>
                </li>
              )}
            </>
          )}
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
        {pickers.error !== null && <p className="pane-gate__error">{pickers.error}</p>}
        {/* **What a passing login check does not prove.** A green row here means a
            credential resolved and its provider answered — not that the provider
            accepted it — and the moment that matters is exactly the moment
            everything looks fine, so it is said on the card and not only on the
            feed. */}
        {caveats.map((offer) => (
          <p key={offer.name} className="pane-gate__note">
            <span className="mono">{offer.name}</span>: {offer.caveat}
          </p>
        ))}
        <p className="pane-gate__note">{WHAT_A_REMEMBERED_MODEL_IS}</p>
        <div className="pane-gate__actions">
          <button
            className="pane-gate__alt"
            title="Ask every harness again — for a login you completed in another terminal."
            onClick={pickers.recheck}
            disabled={pickers.rechecking || pickers.loading}
          >
            {pickers.rechecking ? "Re-checking…" : "Re-check logins"}
          </button>
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

/// Each distinct harness this fleet is about to spend, once.
function selectedHarnesses(gate: GateState): HarnessOffer[] {
  const names = new Set<string>([gate.seats.orch.harness]);
  for (const worker of gate.seats.workers) names.add(worker.harness);
  const offers: HarnessOffer[] = [];
  for (const name of names) {
    const offer = offerFor(gate, name);
    if (offer !== undefined) offers.push(offer);
  }
  return offers;
}
