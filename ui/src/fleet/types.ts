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
/// `pty://output/2`). Single-sourced here because listening on the wrong name
/// renders nothing and reports no error — there is no failure signal for it.
export function paneKey(pane: PaneId): string {
  return pane === ORCH ? "orch" : pane.replace(/^worker-/, "");
}

export function paneSlot(pane: PaneId): number | null {
  const m = /^worker-(\d+)$/.exec(pane);
  return m ? Number(m[1]) : null;
}

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
/// Five variants. Three are what `fleetor_core::FleetEvent` had after Phase 5 —
/// the ten that described the headless supervisor went with it — `command` is
/// D-045's and `task` is WP-05's. **The frontend renders an unknown `type` as
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
      /// text and been swallowed as prose (`docs/command-channel-notes.md`).
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
  | { seq: number; type: "pane-state"; pane: PaneId; from: string; to: string }
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
