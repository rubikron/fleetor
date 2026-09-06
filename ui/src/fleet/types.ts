// TS mirrors of the Rust wire types (fleetor-core::event / ::pane / ::message).
// These must match the serde output: the event tag is `type` in kebab-case, enums
// are kebab-case, and the backend adds `seq` to every streamed event. Kept in one
// place so a contract drift shows up as a type error, not a silent UI bug.

export type NoticeLevel = "info" | "warn" | "error";

/// A pane's name, exactly as `fleetor_core::pane::PaneId` serializes it — a bare
/// string, so the CLI argument, the DB payload, the event field and this type are
/// all the same thing with no second spelling to keep in sync.
export type PaneId = string;

export const ORCH: PaneId = "orch";

/// The human (WP-07). A participant in the record with **no terminal** — which
/// is the one fact everything else about them follows from: nothing spawns it,
/// nothing kills it, and a message addressed to it is `recorded` rather than
/// `accepted`. See `hasPty`.
export const OPERATOR: PaneId = "operator";

/// A terminal that is not part of the fleet (WP-15), mirroring
/// `fleetor_core::pane::PaneId::Evaluator`.
///
/// **The mirror image of `OPERATOR`.** The human is in the `fleet roster`
/// listing and has no terminal; this has a real terminal and is in no listing at
/// all — not `ROSTER`, not a broadcast's legs, not any brief's peer list. It is
/// addressable by name in both directions and enumerated by nothing.
export const EVALUATOR: PaneId = "evaluator";

/// The Critic (WP-20), mirroring `fleetor_core::pane::PaneId::Critic`.
///
/// A real terminal that is in no listing, like `EVALUATOR` — and outside the
/// fleet for a different reason: it is a **reader** of a run rather than a
/// participant in one. Nothing hides it. It is named in the rail, its brief is
/// `prompts/critic.md` for the operator to edit, and it exists whether or not
/// dev mode is on. It is also the one pane with no route back: placement hands
/// it no fleet socket, so `fleet send` inside it reaches nothing.
export const CRITIC: PaneId = "critic";

export const WORKER_SLOTS = [1, 2, 3, 4] as const;

export function workerPane(slot: number): PaneId {
  return `worker-${slot}`;
}

/// Every pane — what spawns, what a broadcast reaches, what has a tab. The
/// operator is deliberately not here: it is addressable, not runnable.
export const ROSTER: PaneId[] = [ORCH, ...WORKER_SLOTS.map(workerPane)];

/// Whether there is a terminal behind this name, mirroring
/// `fleetor_core::pane::PaneId::has_pty`.
///
/// **This is the derivation that decides `recorded` vs `accepted`.** It is not
/// stored on the event, because it cannot disagree with itself: a message whose
/// addressee has no pty was never written to one, so `accepted: false` on such
/// a row is the literal truth of that field and not a failure. Rendering it as
/// "undelivered" would report a delivery that was never attempted.
export function hasPty(pane: PaneId): boolean {
  return pane !== OPERATOR;
}

/// The suffix in a pane's event channel names (`pty://output/orch`,
/// `pty://output/2`, `pty://output/evaluator`). Single-sourced here because
/// listening on the wrong name renders nothing and reports no error — there is
/// no failure signal for it.
///
/// The Rust half is `pty.rs::channel_key`, and the two are pinned against each
/// other by `pty.rs`'s own uniqueness test. A worker's key is its bare slot
/// number; every other name is itself.
export function paneKey(pane: PaneId): string {
  const slot = paneSlot(pane);
  return slot === null ? pane : String(slot);
}

export function paneSlot(pane: PaneId): number | null {
  const m = /^worker-(\d+)$/.exec(pane);
  return m ? Number(m[1]) : null;
}

// --- the start gate's harness and model pickers (WP-25 #35) -------------------

/// **Which of four states a harness is in on this machine**, mirroring the words
/// `HarnessOffer::from` in `src-tauri/src/fleet.rs` decides.
///
/// The two lists are pinned against each other by `src-tauri/tests/gate_pickers.rs`,
/// for the reason `views.rs` pins its two: a fifth state added on one side and not
/// the other renders as nothing at all, exactly the way a view missing from the
/// restore list was silently unreachable.
///
/// `unreadable` is deliberately **not** a refusal. A vendor that changed its report
/// format is a working installation the gate could not parse, and disabling a seat
/// on the strength of a parse error is the failure the narrow refusal exists to
/// avoid.
export type HarnessStatus = "logged-in" | "no-credential" | "not-installed" | "unreadable";

/// The two states in which a harness may not take a seat.
///
/// **Disabled, never hidden** — a supported feature must never look unimplemented,
/// so the option stays in the list carrying `reason` beside it.
export const CANNOT_TAKE_A_SEAT: readonly HarnessStatus[] = ["no-credential", "not-installed"];

export function canTakeASeat(harness: HarnessOffer): boolean {
  return !CANNOT_TAKE_A_SEAT.includes(harness.status);
}

/// One model a harness would accept, as the vendor names it. The slug goes on the
/// argv; the display name is what a person recognises.
export interface ModelOffer {
  slug: string;
  display_name: string;
}

/// One harness as the gate offers it — the facts that sit beside the model.
export interface HarnessOffer {
  name: string;
  invoked: string;
  status: HarnessStatus;
  /// The account *shape*, never a plan tier — `doctor` reports the shape and the
  /// tier lives inside the stored token (C14 as narrowed by C58).
  account: string | null;
  /// Why this harness cannot take a seat, in the vendor's own words.
  reason: string | null;
  /// **What a passing check does not prove.** Present exactly when this machine
  /// looks logged in, which is the only state where an unqualified green misleads.
  caveat: string | null;
  version: string | null;
  /// The provider the vendor resolved — a fact, never a control. There is no
  /// provider picker anywhere.
  provider: string | null;
  /// The vendor's own catalog, in the vendor's order. Empty is an answer: the gate
  /// offers no list and the operator types a name.
  models: ModelOffer[];
  /// Whether the PATH name and the vendor binary behind it agreed.
  readings_agree: boolean;
  resolved: string | null;
}

/// One seat's choice — a harness and the model it *starts* with.
///
/// `model: null` is the seat's own default: the orchestrator's
/// `default (your login)` sentinel, or a worker's launch-configured model.
export interface SeatChoice {
  harness: string;
  model: string | null;
}

/// What the operator picked, for every seat that may carry a choice.
///
/// Five seats. **The two judges are absent and there is no field for them** — a
/// judge running the same harness as the judged is a variable this arc does not
/// introduce, and the absence is structural on both sides of the wire.
export interface FleetSeats {
  orch: SeatChoice;
  workers: SeatChoice[];
}

// --- what a click will cost, and what would refuse it (WP-25 #36) -------------
//
// Every type below is computed in Rust (`src-tauri/src/fleet.rs`) and rendered
// here. Nothing in this interface re-derives them, and that is the point: the rule
// that disables the button and the rule `fleet_bootstrap` enforces have to be one
// rule, or the fleet refuses a start the gate said was fine — or the reverse, which
// is worse.

/// One seat the fleet will not start with, and why (story 11).
export interface StartRefusal {
  seat: string;
  harness: string;
  /// The vendor's own sentence about this machine — the operator's actionable line.
  reason: string;
}

/// What one harness will actually spend, in one role (story 12, C9).
export interface CostLine {
  /// `orchestrator`, `all 4 worker seats`, `worker seats 1 and 3`.
  seats: string;
  harness: string;
  sentence: string;
}

/// A model the harness's own catalog does not list, and what the seat fell back to
/// (story 14).
export interface ModelFallback {
  seat: string;
  harness: string;
  asked: string;
  fell_back_to: string;
}

/// **What a click will do**, read off the seats that will place.
export interface StartVerdict {
  /// Empty exactly when the fleet may start.
  refusals: StartRefusal[];
  cost: CostLine[];
  fallbacks: ModelFallback[];
}

/// **The whole of what the start gate renders from.**
///
/// One value, so the pickers and the summary cannot be reading two things: the
/// summary is generated from `seats`, and `seats` is what the backend places
/// against. Since #36 `fleet_set_seats` answers with this same type rather than
/// with the seats alone, so a pick replaces the whole screen with what the backend
/// stored instead of splicing a new selection into an older verdict.
export interface GateState {
  harnesses: HarnessOffer[];
  seats: FleetSeats;
  /// The model a worker runs when the operator names none.
  worker_model_default: string;
  verdict: StartVerdict;
}

/// The orchestrator's sentinel, and the one label it wears (M2).
///
/// It is a *seat* default rather than a model name — `null` on the wire — so it
/// keeps reachable exactly the command the orchestrator ran before this picker
/// existed: the operator's own login, naming no model at all.
export const DEFAULT_YOUR_LOGIN = "default (your login)";

/// What the shell knows about a pane. Deliberately not the Rust `PaneState`:
/// the shell learns liveness from its own pty channels (first output → live,
/// exit → dead), and "idle" means it has not been started, which the backend has
/// no name for because an unspawned pane simply is not in its registry.
export type PaneStatus = "idle" | "live" | "dead";

/// What someone *claims* a task block's state is, mirroring
/// `fleetor_core::task::TaskStatus`. Descriptive only — nothing in this UI or
/// behind it enforces a transition, and `done` is an unverified claim until
/// WP-06's review, so nothing here may render it as a verified fact.
export type TaskStatus = "planned" | "claimed" | "done" | "dropped";

/// One task block, in the vision's own shape
/// (`fleetor_core::task::TaskBlock`). The id is not here: it lives on the event
/// that posted the block, so a post and its updates join on one field.
export interface TaskBlock {
  outcome: string;
  technical: string[];
  semantic: string[];
  worker: PaneId;
  instructions?: string | null;
  /// The block this one was cut out of, by id.
  parent?: string | null;
  /// The block this stream of work comes back together in, by id.
  converges_on?: string | null;
}

/// What one task event says: the block went up, or something was claimed about
/// it. Internally tagged on `change`, exactly as the Rust enum serializes.
export type TaskChange =
  | { change: "posted"; block: TaskBlock }
  | { change: "updated"; status?: TaskStatus | null; note?: string | null };

/// The append-only event, discriminated on `type`, each carrying its `seq`.
///
/// Six variants. Three are what `fleetor_core::FleetEvent` had after Phase 5 —
/// the ten that described the headless supervisor went with it — `command` is
/// D-045's, `task` is WP-05's and `handoff` is WP-13's. **The frontend renders an unknown `type` as
/// nothing at all**, so a backend variant that is not mirrored here is invisible
/// rather than broken, which is why this file moves in the same commit as
/// `event.rs`.
export type FleetEvent =
  | {
      seq: number;
      type: "message";
      id: string;
      from: PaneId;
      to: PaneId;
      body: string;
      group?: string | null;
      /// The bytes reached a live pty. **Never** render this as "delivered" —
      /// nothing on this side knows whether the agent read them (L3).
      ///
      /// It is `false` for two entirely different situations, and a renderer
      /// must tell them apart with `hasPty(to)`: a pane that refused (which is
      /// *undelivered*, and carries a `detail`), and the operator, who has no
      /// pty to accept anything (which is *recorded*, and carries none).
      accepted: boolean;
      detail?: string | null;
    }
  | {
      seq: number;
      type: "command";
      id: string;
      from: PaneId;
      to: PaneId;
      /// The slash command as it was typed into the terminal — `/clear`, or
      /// `/compact <what to keep>`. Always one of `ALLOWED_COMMANDS`.
      command: string;
      /// Why the sender decided to send it. Never empty, and never hidden: this
      /// is the reasoning chain the verb exists to keep.
      why: string;
      /// The bytes reached a live pty. **Never** render this as "executed" — the
      /// command may have been queued behind a turn, or landed after unsubmitted
      /// text and been swallowed as prose (`docs/notes/command-channel-notes.md`).
      accepted: boolean;
      detail?: string | null;
    }
  | {
      seq: number;
      type: "task";
      /// The **block's** id, shared by its post and every later update — the key
      /// the board replay folds on.
      task: string;
      /// Who made this claim. The board has no permission system; this field is
      /// the whole of the accountability, so it is never hidden.
      from: PaneId;
      /// When, in epoch ms. On the payload rather than only the DB row, because
      /// this UI replays the board from the event stream and never sees the row.
      at: number;
      change: TaskChange;
    }
  | {
      seq: number;
      type: "handoff";
      id: string;
      /// Who declared it. `orch` in practice — the CLI refuses the verb to
      /// anyone else — but the field is the accountability, not a decoration,
      /// so it is rendered rather than assumed.
      from: PaneId;
      /// What the fleet built, in the operator's own terms.
      built: string;
      /// How anyone could check it. Never empty, and never truncated: this is
      /// the half of the claim the operator can actually act on.
      evidence: string[];
      /// What is unfinished or uncertain. Absent when nothing was named — which
      /// means exactly that, never that nothing is open.
      open?: string[];
    }
  /// A pane moved through its lifecycle. `harness` and `model` are carried on the
  /// spawn and absent on every other transition (WP-25, M24): a run's manifest
  /// records which vendor ran in each seat, and a feed that could not say the same
  /// would be poorer than the archive of itself. `model` stays absent on an
  /// attended seat, which runs the operator's own login.
  | {
      seq: number;
      type: "pane-state";
      pane: PaneId;
      from: string;
      to: string;
      harness?: string;
      model?: string;
    }
  | { seq: number; type: "notice"; level: NoticeLevel; text: string };

export type MessageEvent = Extract<FleetEvent, { type: "message" }>;
export type CommandEvent = Extract<FleetEvent, { type: "command" }>;
export type TaskEvent = Extract<FleetEvent, { type: "task" }>;

export function isMessage(event: FleetEvent): event is MessageEvent {
  return event.type === "message";
}

export function isCommand(event: FleetEvent): event is CommandEvent {
  return event.type === "command";
}

export function isTask(event: FleetEvent): event is TaskEvent {
  return event.type === "task";
}

export interface BootSnapshot {
  latest_seq: number;
}

/// One archived run, mirroring `src-tauri::runs::RunRecord` (WP-11, D-058).
///
/// Every field except `label` is derived from the run's own log and would come
/// back identically if the index were deleted; `label` is the only thing a human
/// authored, which is the whole reason the index file exists.
///
/// `started_ms` / `ended_ms` are the log's first and last timestamps, so they
/// are absent for a run whose log is empty — a start-then-quit. Rendering
/// `null` as an epoch date would date every empty run to 1970.
export interface RunRecord {
  id: string;
  label: string;
  target?: string;
  started_ms?: number;
  ended_ms?: number;
  events: number;
  messages: number;
  tasks: number;
  bytes: number;
  /// Worker session transcripts archived with the run. `orch` never contributes
  /// one — its transcript lives in the operator's own config dir, outside
  /// `~/.fleetor`, and this app does not reach in there.
  transcripts: number;
}

/// The live fleet configuration: what a click will actually run, and where.
export interface FleetConfig {
  target: string;
  target_path: string;
  branch: string;
  /// The model in the worker seats, or `"none"` when no key is available and
  /// only the orchestrator can start.
  worker_backend: string;
  lead_model: string;
  gate: string;
}

/// A read-only estimate of how much of a pane's context window is in use,
/// mirroring `fleetor_core::pane::ContextGauge` (WP-04). Present only when
/// something has actually sampled the pane's transcript — see `PaneEntry`.
export interface ContextGauge {
  used_tokens: number;
  window_tokens: number;
  /// Pre-divided on the Rust side so this UI and the `fleet` CLI never round
  /// a percent differently.
  pct: number;
}

/// What this pane's rail has to say about its context, as four distinct
/// answers rather than a gauge-or-nothing (#48).
///
/// **Absence was three different facts wearing one costume.** Before this,
/// the map held `ContextGauge | undefined` and every consumer guarded on bare
/// truthiness, so a pane whose usage *could not be read* rendered exactly what
/// a pane nobody had asked about yet rendered, which is exactly what the
/// orchestrator — which has no gauge by construction — rendered: nothing. The
/// rule that predates this arc is that an unavailable gauge **says**
/// unavailable (D-054, M22, C12), and silence does not say it: it is
/// indistinguishable from "nothing to report", which is what a healthy idle
/// pane looks like too.
///
/// Nothing here is synthesized. `unavailable` carries no number and none is
/// computed for it — `Option::None` on the Rust side still means unavailable,
/// never zero (C24, C65).
export type GaugeReading =
  /// The backend answered with a figure it read off the pane's own vendor.
  | { kind: "sampled"; gauge: ContextGauge }
  /// The backend answered *about this pane* and had no figure: `context: None`
  /// on its roster row. The same state the CLI renders as `—`.
  | { kind: "unavailable" }
  /// Nothing has answered about this pane yet — the fleet is not polling, or
  /// the first poll has not landed. Distinct from `unavailable`, because
  /// "not asked yet" and "asked, no reading" are different facts and the
  /// operator can act on only one of them (wait vs. don't trust the rail).
  | { kind: "pending" }
  /// This pane has no gauge to be unavailable — the orchestrator, whose
  /// transcript is the operator's own and deliberately out of scope (WP-04).
  /// Renders nothing at all, which is the one place silence is honest.
  | { kind: "out-of-scope" };

/// The reading for a pane nothing has answered about yet. Exported so a
/// consumer holding `undefined` resolves it through one shared constant
/// instead of each deciding for itself what absence meant.
export const PENDING: GaugeReading = { kind: "pending" };

/// The orchestrator's reading, which is not a reading. Held here beside the
/// others so the one pane that legitimately shows nothing says so in the same
/// vocabulary as the panes that show something.
export const OUT_OF_SCOPE: GaugeReading = { kind: "out-of-scope" };

/// The backend's own lifecycle state for a pane, exactly as
/// `fleetor_core::pane::PaneState` serializes it — **not** the same set as
/// `PaneStatus` above, which the shell derives from its own pty channels and
/// which has an `"idle"` this one does not. `fleet roster`'s payload only, so
/// far; the band still gets liveness from the pty stream, not this.
///
/// `"present"` is the operator's, and only ever the operator's (WP-07): there
/// is no process, so none of the other three can be true of them. A borrowed
/// `"live"` would be the roster's version of rendering `accepted` for a message
/// no pty received.
export type RosterPaneState = "spawning" | "live" | "dead" | "present";

/// One `fleet roster` row, mirroring `fleetor_core::pane::PaneEntry`.
/// `context` is `undefined` whenever the backend's JSON omitted the key —
/// **never render a missing field as zero.** An unsampled pane (every pane at
/// spawn; the orchestrator, always) means *unknown*, not "definitely empty."
export interface PaneEntry {
  pane: PaneId;
  state: RosterPaneState;
  context?: ContextGauge;
}
