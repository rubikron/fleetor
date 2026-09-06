// The start gate's harness and model pickers (WP-25 #35; M1, M2, M3, M15, C23).
//
// **One value, written in one direction.** The backend holds what a click will
// spawn; this hook renders it, and every change goes out through `setSeats` and
// comes back as the *stored* selection before it is displayed. That is M15's chain
// with no branch in it: the summary cannot describe a fleet the backend would not
// place, because the summary is reading what the backend answered.
//
// **What is remembered here and what is not.** localStorage holds the operator's
// last choice, keyed by seat *and by harness* (M3) — so a pane switched to codex
// and back restores the Claude Code model it had rather than a default. It does not
// hold a fleet: nothing is spawned from this file, and a machine that has forgotten
// its localStorage gets the unpicked fleet the backend already defaults to.
//
// **A remembered model that no longer exists is not handled here, and #36 kept it
// that way.** The fallback lives in `fleet.rs::settle_models`, on the side that has
// the live catalog and does the storing — so the value that falls back is the value
// that would have spawned, in one place, rather than a check here and an enforcement
// there. What this file validates is shape: strings where strings belong, and
// harness names that still resolve against the live list. What comes back from
// `setSeats` is the settled fleet, already fallen back and already judged.

import { useCallback, useEffect, useMemo, useState } from "react";
import { fetchGate, setSeats } from "../fleet/api";
import {
  ORCH,
  WORKER_SLOTS,
  canTakeASeat,
  paneSlot,
  workerPane,
  type FleetSeats,
  type GateState,
  type HarnessOffer,
  type PaneId,
  type SeatChoice,
} from "../fleet/types";

const STORAGE_KEY = "fleetor:seat-pickers";

/// The seats the gate offers a choice for, by the same names the roster uses.
///
/// **The two judges are absent and there is no key for them** (C15). A judge running
/// the same harness as the judged is a variable this arc does not introduce, and the
/// absence is structural on all three sides — this list, `FleetSeats` on the wire,
/// and `PaneSpec` in Rust, none of which has a field to hold one.
export const PICKABLE_SEATS: readonly PaneId[] = [ORCH, ...WORKER_SLOTS.map(workerPane)];

/// What the operator last chose, per seat and per harness.
///
/// Two maps rather than one, because they are asked at different moments: `harness`
/// answers "what was this seat on", and `model` answers "and when it was on *that*
/// harness, which model" — which is the whole of M3. A single map keyed by seat
/// alone would lose the second question, and switching a pane to codex and back
/// would reset it to a default rather than restoring what it had.
interface Remembered {
  harness: Record<string, string>;
  model: Record<string, Record<string, string | null>>;
}

const NOTHING_REMEMBERED: Remembered = { harness: {}, model: {} };

/// A stored value read back, with anything that is not the shape above dropped.
///
/// The same validated-read pattern `usePersistedNav` uses and for the same reason: a
/// corrupt, missing, or stale value must fall back to the current default and never
/// throw or wedge the app.
function readRemembered(): Remembered {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (raw === null) return NOTHING_REMEMBERED;
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) return NOTHING_REMEMBERED;
    const { harness, model } = parsed as Partial<Remembered>;
    const cleanHarness: Record<string, string> = {};
    const cleanModel: Record<string, Record<string, string | null>> = {};
    for (const seat of PICKABLE_SEATS) {
      const named = harness?.[seat];
      if (typeof named === "string" && named !== "") cleanHarness[seat] = named;
      const perHarness = model?.[seat];
      if (typeof perHarness !== "object" || perHarness === null) continue;
      const kept: Record<string, string | null> = {};
      for (const [name, value] of Object.entries(perHarness)) {
        if (value === null || typeof value === "string") kept[name] = value;
      }
      cleanModel[seat] = kept;
    }
    return { harness: cleanHarness, model: cleanModel };
  } catch {
    return NOTHING_REMEMBERED;
  }
}

function writeRemembered(remembered: Remembered): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(remembered));
  } catch {
    /* best-effort persistence only; the gate still works for this session */
  }
}

/// The model a seat falls back to when nothing is remembered for this harness.
///
/// **The two seats answer differently, and that follows from C9.** An orchestrator's
/// `null` is the `default (your login)` sentinel — it runs the operator's own login
/// and lets the vendor choose. A worker runs FLEETOR's provider, where there is no
/// vendor default to fall back to, so its unnamed model is the launch
/// configuration's.
function seatDefault(seat: PaneId, workerModelDefault: string): string | null {
  return seat === ORCH ? null : workerModelDefault;
}

function seatOf(seats: FleetSeats, seat: PaneId): SeatChoice {
  const slot = paneSlot(seat);
  return (slot === null ? seats.orch : seats.workers[slot - 1]) ?? seats.orch;
}

function withSeat(seats: FleetSeats, seat: PaneId, choice: SeatChoice): FleetSeats {
  const slot = paneSlot(seat);
  if (slot === null) return { ...seats, orch: choice };
  const workers = seats.workers.map((was, at) => (at === slot - 1 ? choice : was));
  return { ...seats, workers };
}

export interface SeatPickers {
  /// Everything the gate renders from, or `null` until the first probe answers.
  gate: GateState | null;
  /// The first probe is still running. It costs a subprocess per harness and about
  /// 1.4 s, so the gate says so rather than rendering an empty list as an answer.
  loading: boolean;
  /// The re-check button is running.
  rechecking: boolean;
  /// A refused selection or a failed probe, in words the operator can act on.
  error: string | null;
  /// **Ask every harness again** — the operator has logged in from another terminal
  /// and no held reading can contain that.
  recheck: () => void;
  /// Put one seat on a harness, restoring the model it last had *on that harness*.
  chooseHarness: (seat: PaneId, harness: string) => void;
  /// Name the model one seat starts with. Empty means the seat's own default.
  chooseModel: (seat: PaneId, model: string) => void;
  /// Put all four workers on one harness — the collapsed row, where the common case
  /// costs one selection rather than four.
  chooseWorkersHarness: (harness: string) => void;
  /// Name the model all four workers start with.
  chooseWorkersModel: (model: string) => void;
  /// Whether the workers row is showing four seats or one.
  expanded: boolean;
  setExpanded: (expanded: boolean) => void;
  /// Whether the four workers are currently on the same harness and model. A
  /// collapsed row may only speak for them when they agree.
  workersAgree: boolean;
}

export function useSeatPickers(): SeatPickers {
  const [gate, setGate] = useState<GateState | null>(null);
  const [loading, setLoading] = useState(true);
  const [rechecking, setRechecking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [manuallyExpanded, setManuallyExpanded] = useState(false);

  // The first read, and the restore that follows it. The remembered selection is
  // pushed to the backend rather than only displayed, because the backend is what
  // places — a restored choice the gate showed and never sent would be the exact
  // gate-promises-one-fleet-spawns-another M15 refuses.
  useEffect(() => {
    let live = true;
    void (async () => {
      try {
        const answered = await fetchGate();
        if (!live) return;
        const restored = restore(answered, readRemembered());
        // The whole answer replaces the whole answer. `setSeats` returns a
        // `GateState`, so what is displayed after a restore is the backend's own
        // reading of the restored fleet — including whether it may start at all and
        // whether a remembered model survived the vendor's current catalog (#36).
        const settled = restored === null ? answered : await setSeats(restored);
        if (!live) return;
        setGate(settled);
      } catch (e) {
        if (live) setError(String(e));
      } finally {
        if (live) setLoading(false);
      }
    })();
    return () => {
      live = false;
    };
  }, []);

  const push = useCallback(
    (next: FleetSeats) => {
      // Optimistic only until the backend answers, and then replaced by what it
      // stored. The two agree in every ordinary case; where they do not, what is
      // displayed is what will spawn.
      setGate((current) => (current === null ? current : { ...current, seats: next }));
      void setSeats(next)
        .then((stored) => {
          setError(null);
          // The backend's whole answer, not a splice. A model the vendor's catalog
          // no longer lists has already fallen back inside `stored`, and the cost
          // lines and the refusal describe *that* fleet (#36) — merging only
          // `seats` would leave the summary a round trip behind the thing it is
          // summarising.
          setGate(stored);
        })
        .catch((e: unknown) => setError(String(e)));
    },
    [],
  );

  const remember = useCallback((seat: PaneId, choice: SeatChoice) => {
    const remembered = readRemembered();
    remembered.harness[seat] = choice.harness;
    remembered.model[seat] = { ...remembered.model[seat], [choice.harness]: choice.model };
    writeRemembered(remembered);
  }, []);

  const change = useCallback(
    (seats: PaneId[], choose: (was: SeatChoice, seat: PaneId) => SeatChoice) => {
      setGate((current) => {
        if (current === null) return current;
        let next = current.seats;
        for (const seat of seats) {
          const chosen = choose(seatOf(next, seat), seat);
          remember(seat, chosen);
          next = withSeat(next, seat, chosen);
        }
        push(next);
        return { ...current, seats: next };
      });
    },
    [push, remember],
  );

  const onHarness = useCallback(
    (seats: PaneId[], harness: string) => {
      const workerModelDefault = gate?.worker_model_default ?? "";
      const remembered = readRemembered();
      change(seats, (was, seat) => {
        if (was.harness === harness) return was;
        // **M3, and this is the whole of it.** The model this seat last ran on
        // *this harness* comes back; a seat that has never been on it takes the
        // seat's own default rather than carrying the previous harness's model
        // across, which would name a model the new vendor has never heard of.
        const perHarness = remembered.model[seat] ?? {};
        const model =
          harness in perHarness ? perHarness[harness] : seatDefault(seat, workerModelDefault);
        return { harness, model };
      });
    },
    [change, gate],
  );

  const chooseHarness = useCallback(
    (seat: PaneId, harness: string) => onHarness([seat], harness),
    [onHarness],
  );
  const chooseWorkersHarness = useCallback(
    (harness: string) => onHarness([...WORKER_SLOTS.map(workerPane)], harness),
    [onHarness],
  );

  const onModel = useCallback(
    (seats: PaneId[], typed: string) => {
      const workerModelDefault = gate?.worker_model_default ?? "";
      const trimmed = typed.trim();
      change(seats, (was, seat) => ({
        harness: was.harness,
        // An empty field is the seat's own default, said once here rather than as an
        // empty string travelling to a `--model` flag.
        model: trimmed === "" ? seatDefault(seat, workerModelDefault) : trimmed,
      }));
    },
    [change, gate],
  );

  const chooseModel = useCallback((seat: PaneId, model: string) => onModel([seat], model), [
    onModel,
  ]);
  const chooseWorkersModel = useCallback(
    (model: string) => onModel([...WORKER_SLOTS.map(workerPane)], model),
    [onModel],
  );

  const recheck = useCallback(() => {
    setRechecking(true);
    setError(null);
    void fetchGate(true)
      .then((answered) => setGate(answered))
      .catch((e: unknown) => setError(String(e)))
      .finally(() => setRechecking(false));
  }, []);

  const workersAgree = useMemo(() => {
    const workers = gate?.seats.workers ?? [];
    return workers.every(
      (seat) => seat.harness === workers[0]?.harness && seat.model === workers[0]?.model,
    );
  }, [gate]);

  return {
    gate,
    loading,
    rechecking,
    error,
    recheck,
    chooseHarness,
    chooseModel,
    chooseWorkersHarness,
    chooseWorkersModel,
    // **A collapsed row may only speak for four seats that agree.** A mixed fleet
    // opens itself rather than being summarised by a control that names one of the
    // four and silently rewrites the other three on the next keystroke.
    expanded: manuallyExpanded || !workersAgree,
    setExpanded: setManuallyExpanded,
    workersAgree,
  };
}

/// The remembered selection applied to what the machine currently offers, or `null`
/// when nothing was remembered and the backend's own default already stands.
///
/// **A remembered harness that cannot take a seat is still restored**, and that is
/// deliberate. Quietly moving the operator onto a working harness would be the gate
/// choosing a fleet nobody picked; what the row does instead is show the selection
/// disabled with the vendor's reason beside it, which is the one state from which an
/// operator can act. Refusing to *start* such a fleet is #36's.
///
/// A remembered harness this build no longer registers is dropped, because there is
/// no row to show it on.
function restore(gate: GateState, remembered: Remembered): FleetSeats | null {
  const known = new Set(gate.harnesses.map((harness) => harness.name));
  let seats = gate.seats;
  let changed = false;
  for (const seat of PICKABLE_SEATS) {
    const harness = remembered.harness[seat];
    if (harness === undefined || !known.has(harness)) continue;
    const perHarness = remembered.model[seat] ?? {};
    const model =
      harness in perHarness ? perHarness[harness] : seatDefault(seat, gate.worker_model_default);
    const was = seatOf(seats, seat);
    if (was.harness === harness && was.model === model) continue;
    seats = withSeat(seats, seat, { harness, model });
    changed = true;
  }
  return changed ? seats : null;
}

/// The offer for one harness by name, or `undefined` if this build no longer has it.
export function offerFor(gate: GateState, name: string): HarnessOffer | undefined {
  return gate.harnesses.find((harness) => harness.name === name);
}

/// Whether the seat currently selected on `name` could actually spawn.
export function seatIsRunnable(gate: GateState, name: string): boolean {
  const offer = offerFor(gate, name);
  return offer !== undefined && canTakeASeat(offer);
}
