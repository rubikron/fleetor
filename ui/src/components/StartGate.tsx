// The spend gate: nothing runs until the operator starts the fleet.
//
// This is the one screen whose entire job is telling the operator what a click
// will cost, so it may not round anything up. It states the models that will
// actually run and the directory they will run in. The predecessor to this card rendered any worker backend that was not
// "flash" as "fake (free proof path)", promising a free path that no longer
// existed; that is the exact failure this file exists to not repeat.
//
// **Since WP-25 #35 it is also where each seat gets its harness and its model**
// (M1, M15, C23): one row for the orchestrator, one for the workers, and a
// disclosure that expands the workers row into four. Three things are deliberately
// not offered anywhere on this card and each is a recorded decision rather than a
// gap — **no provider picker** (every seat inherits its login's and displays it: C2
// as amended by C9), **no harness for the two judges** (C15, and their
// specs have no field to hold one), and **no plan tier** (the vendor's diagnostic
// reports the account *shape*, not the tier: C14 as narrowed by C58).
//
// Every fact beside a model comes off `useSeatPickers`, which reads the backend's
// held probe and writes back through it. The summary below reads the same `seats`
// value the pickers write and the backend places against, so this card cannot
// promise a fleet that is not the one that will spawn (M15).
//
// **Since WP-25 #36 the card also refuses** (stories 11, 12, 14). Three things on
// it are the backend's answers rather than this file's, arriving on the same
// `GateState` the rows render from — the per-harness cost lines, the model
// fallbacks, and the refusals that disable the Start button. None of the three is
// re-derived here: `fleet_bootstrap` refuses on the identical verdict, and a rule
// implemented on both sides of a wire is a rule that will eventually hold on one.

import { useState } from "react";
import {
  DEFAULT_YOUR_LOGIN,
  ORCH,
  WORKER_SLOTS,
  canTakeASeat,
  workerPane,
  type GateState,
  type HarnessOffer,
  type SeatChoice,
  type StartRefusal,
  type ModelOffer,
} from "../fleet/types";
import { offerFor, useSeatPickers } from "../ui/useSeatPickers";

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
  onStart: () => void;
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

/// The models one seat may run: its harness's own list, off the wire. **No vendor
/// name is written here**, which `tests/gate_pickers.rs` enforces.
function modelsFor(gate: GateState, seat: SeatChoice): ModelOffer[] {
  return offerFor(gate, seat.harness)?.models ?? [];
}

export interface SeatRowProps {
  label: string;
  gate: GateState;
  seat: SeatChoice;
  /// What an empty model field reads as: M2's sentinel, on every seat.
  sentinel: string;
  onHarness: (harness: string) => void;
  onModel: (model: string) => void;
  /// A short line under the picker, where a row needs one the facts do not carry.
  aside?: string;
  /// **Suppressed on every row but the first of its harness group** (C78). Four
  /// seats on two harnesses printed the same two sentences twice each, and
  /// identical text under identical rows trains the eye to skip all four.
  showFacts?: boolean;
}

/// One seat's harness and model, with the harness's own facts beneath them.
///
/// **Exported so a render probe can mount it directly** (C63) — the orchestrator
/// row and every worker row are this one component, so proving it actually
/// produces the harness `<select>`, the model field and the facts line is proving
/// the whole card's seat-picking half is reachable, not just that the source says
/// the right thing.
export function SeatRow({
  label,
  gate,
  seat,
  sentinel,
  onHarness,
  onModel,
  aside,
  showFacts = true,
}: SeatRowProps) {
  const offer = offerFor(gate, seat.harness);
  const models = modelsFor(gate, seat);
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
        <div className="seat-row__field">
          {models.length > 0 ? (
            <select
              className="seat-row__model"
              aria-label={`${label} model`}
              value={seat.model ?? ""}
              onChange={(e) => onModel(e.target.value)}
            >
              <option value="">{sentinel}</option>
              {seat.model !== null && !models.some((m) => m.slug === seat.model) && (
                <option value={seat.model}>{seat.model}</option>
              )}
              {models.map((model) => (
                <option key={model.slug} value={model.slug}>
                  {model.display_name}
                </option>
              ))}
            </select>
          ) : (
            <>
              <input
                className="seat-row__model"
                type="text"
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
              {seat.model !== null && (
                <button
                  type="button"
                  className="seat-row__aff"
                  aria-label={`reset ${label} model`}
                  title={`Run this seat on ${sentinel}`}
                  onClick={() => {
                    setTyped(null);
                    onModel("");
                  }}
                >
                  ↺
                </button>
              )}
            </>
          )}
        </div>
      </div>
      {showFacts && <p className="seat-row__facts">{factsFor(offer)}</p>}
      {/* **The way out, beside what is wrong** (#51). The line above is the vendor's
          own sentence about the state and it is correct and it is a dead end; this
          is the move. Everything in it — the sentence, the command, the variable —
          arrives on the wire from `HarnessSpec::login`, so nothing here knows which
          harness it is describing or which of the three not-usable shapes it is in.

          **There is no field to paste a credential into, here or anywhere.** Only
          the vendor's own login writes where the vendor reads, so FLEETOR names the
          command and the operator runs it. The `Re-check logins` button below asks
          every harness again, which is how a login done in another terminal lands
          without restarting the app. */}
      {offer?.guidance != null && (
        <p className="seat-row__howto">
          {offer.guidance.sentence}
          {offer.guidance.command !== null && (
            <>
              {" "}
              <span className="mono">{offer.guidance.command}</span>
              {offer.guidance.then !== null && (
                <>
                  {" then "}
                  <span className="mono">{offer.guidance.then}</span>
                </>
              )}
            </>
          )}
          {offer.guidance.variable !== null && (
            <>
              {" "}
              <span className="mono">{offer.guidance.variable}</span>
            </>
          )}
        </p>
      )}
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

export function StartGate({ onStart }: StartGateProps) {
  const pickers = useSeatPickers();

  const gate = pickers.gate;
  // **Every seat named here comes off `gate.seats`**, which is what the backend
  // stored and what `spawn_pane` places against (M15).
  const workers = gate?.seats.workers ?? [];
  const firstWorker = workers[0];
  // The caveat is a property of a *login*, so it is said once for the harnesses this
  // fleet is actually about to spend, and not once per seat.
  const spending = gate === null ? [] : selectedHarnesses(gate);
  const caveats = spending.filter((offer) => offer.caveat !== null);
  // **Read, never re-derived.** Whether this fleet may start is the backend's
  // answer, arriving on the same value the rows render from; computing it here from
  // `canTakeASeat` would be a second implementation of a rule that has to hold in
  // one place (#36).
  const refusals = gate?.verdict.refusals ?? [];

  return (
    <div className="pane-gate pane-gate--workspace">
      <div className="pane-gate__card">
        <h3 className="pane-gate__title">Start the fleet</h3>
        <p className="pane-gate__body">
          Spawns the orchestrator and {WORKER_SLOTS.length} worker terminals,
          each on the harness named below, all wired to the hub. It will spend tokens.
        </p>
        {/* **How many seats are about to spend the plan** (#49). Separate from the
            cost sentences because those are per-harness and this is per-fleet: a
            mixed fleet has two of them and still exactly one answer to "how many".
            Rendered only when it is true — a line reading "0 seats" every launch is
            a line an operator stops reading. */}
        {gate?.verdict.plan_seats != null && (
          <p className="pane-gate__note pane-gate__note--warn">{gate.verdict.plan_seats}</p>
        )}
        {/* **What it will spend, per harness and per seat** (#36, story 12, C9).
            Written by the backend from the same `seats` the pickers wrote, so this
            card cannot promise a fleet that is not the one that will spawn (M15). */}
        {gate !== null && gate.verdict.cost.length > 0 && (
          <ul className="pane-gate__cost">
            {gate.verdict.cost.map((line) => (
              <li key={`${line.harness}-${line.seats}`}>{line.sentence}</li>
            ))}
          </ul>
        )}
        <ul className="pane-gate__facts">
          {gate === null ? (
            <li className="pane-gate__fact">
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
              {/* **The disclosure now lives under the label it controls** (defect
                  6): a "workers" header row, so the toggle is never a line an
                  operator finds floating under four rows with no clear owner. It
                  also says in words whether the four currently agree, since
                  "disabled, with a tooltip on hover" is not itself discoverable. */}
              <li className="seat-group-header">
                <span className="k">workers</span>
                <div className="seat-group-header__row">
                  <span className="seat-group-header__note">
                    {pickers.expanded
                      ? pickers.workersAgree
                        ? "shown separately"
                        : "shown separately — they differ"
                      : "one row speaks for all four"}
                  </span>
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
                </div>
              </li>
              {pickers.expanded ? (
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
                        sentinel={DEFAULT_YOUR_LOGIN}
                        onHarness={(harness) => pickers.chooseHarness(workerPane(slot), harness)}
                        onModel={(model) => pickers.chooseModel(workerPane(slot), model)}
                        // **Once per harness group, not once per seat** (C78).
                        // Four seats on two harnesses printed the same two
                        // sentences twice each; identical text under identical
                        // rows trains the eye to skip all four.
                        showFacts={workers[slot - 2]?.harness !== seat.harness}
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
                    sentinel={DEFAULT_YOUR_LOGIN}
                    onHarness={pickers.chooseWorkersHarness}
                    onModel={pickers.chooseWorkersModel}
                    aside="Fenced, on your own login — copied into each pane at spawn, never written back."
                  />
                )
              )}
            </>
          )}
        </ul>
        {pickers.error !== null && <p className="pane-gate__error">{pickers.error}</p>}
        {/* **What a passing login check does not prove.** A green row here means a
            credential resolved and its provider answered — not that the provider
            accepted it — and the moment that matters is exactly the moment
            everything looks fine, so it is said on the card and not only on the
            feed. */}
        {caveats.map((offer) => (
          <p key={offer.name} className="pane-gate__note pane-gate__note--warn">
            <span className="mono">{offer.name}</span>: {offer.caveat}
          </p>
        ))}
        {/* **A remembered model the vendor no longer lists** (#36, story 14). It
            already fell back — `fleet_set_seats` settled the seat before it stored
            it, so the row above shows the default and a retired id cannot reach a
            `--model` flag. This is the notice that keeps that from being a silent
            substitution: the operator asked for something specific and got something
            else, which they have to be told. */}
        {(gate?.verdict.fallbacks ?? []).map((fallen) => (
          <p key={`${fallen.seat}-${fallen.asked}`} className="pane-gate__note pane-gate__note--warn">
            <span className="mono">{fallen.asked}</span> is not in{" "}
            <span className="mono">{fallen.harness}</span>&apos;s model list on this machine, so{" "}
            {fallen.seat} fell back to <span className="mono">{fallen.fell_back_to}</span>. Pick one
            from the list, or re-check logins if you have just changed your catalog.
          </p>
        ))}
        {/* **Why the fleet will not start** (#36, story 11). Named per seat rather
            than as one "cannot start", because the operator's next action is to
            change *that row* — and the sentence is the vendor's own, since a login
            problem is fixed with the vendor's own command. The button below is
            disabled on the same verdict, and `fleet_bootstrap` refuses on it too:
            the interface is the courtesy, the backend is the rule. */}
        {(gate?.verdict.refusals ?? []).map((refused) => (
          <p key={refused.seat} className="pane-gate__error">
            {refused.seat} is on <span className="mono">{refused.harness}</span> —{" "}
            {refused.reason} Nothing will spawn until that seat can take one: log in and press
            Re-check logins, or put it on another harness.
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
          {/* **Refuse to start** (#36, story 11). Disabled, never hidden, and it
              says why on hover — an operator whose button vanished would have no way
              to tell a refusal from a bug. `fleet_bootstrap` refuses on the same
              verdict, so this is the courtesy and not the enforcement. */}
          <button
            className="pane-gate__go"
            onClick={onStart}
            disabled={refusals.length > 0}
            title={refusals.length === 0 ? undefined : refusals.map(oneLine).join("; ")}
          >
            Start fleet
          </button>
        </div>
      </div>
    </div>
  );
}

/// One refusal as a single line, for the disabled button's tooltip.
function oneLine(refused: StartRefusal): string {
  return `${refused.seat} is on ${refused.harness} — ${refused.reason}`;
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
