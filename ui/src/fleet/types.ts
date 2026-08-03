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
export const WORKER_SLOTS = [1, 2, 3, 4] as const;

export function workerPane(slot: number): PaneId {
  return `worker-${slot}`;
}

export const ROSTER: PaneId[] = [ORCH, ...WORKER_SLOTS.map(workerPane)];

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

/// The append-only event, discriminated on `type`, each carrying its `seq`.
///
/// Three variants, because that is all `fleetor_core::FleetEvent` has after
/// Phase 5. The ten that described the headless supervisor — worker states,
/// ticket moves, tool activity, reports, gate results, review verdicts, mail
/// routing — went with it.
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
      accepted: boolean;
      detail?: string | null;
    }
  | { seq: number; type: "pane-state"; pane: PaneId; from: string; to: string }
  | { seq: number; type: "notice"; level: NoticeLevel; text: string };

export type MessageEvent = Extract<FleetEvent, { type: "message" }>;

export function isMessage(event: FleetEvent): event is MessageEvent {
  return event.type === "message";
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
