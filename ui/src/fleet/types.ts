// TS mirrors of the Rust wire types (fleetor-core::event / ::ticket). These must
// match the serde output: enums are kebab-case, the event tag is `type`, and the
// backend adds `seq` to every streamed event. Kept in one place so a contract
// drift shows up as a type error, not a silent UI bug.

export type WorkerState = "booting" | "idle" | "working" | "blocked" | "dead";

export type TicketState =
  | "backlog"
  | "assigned"
  | "in-progress"
  | "in-review"
  | "done"
  | "blocked"
  | "failed";

export type GateOutcome = "pass" | "fail";
export type ReviewOutcome = "approved" | "changes-requested";
export type NoticeLevel = "info" | "warn" | "error";
export type ReportStatus = "done" | "blocked" | "needs-decision" | "failed";

export interface Ticket {
  id: string;
  title: string;
  body: string;
  files_owned: string[];
  slot: number | null;
  state: TicketState;
  budget: { wall_secs: number; max_tokens: number | null };
}

// The append-only event, discriminated on `type`, each carrying its `seq`.
export type FleetEvent =
  | { seq: number; type: "worker-state"; slot: number; from: WorkerState; to: WorkerState }
  | { seq: number; type: "ticket-state"; ticket: string; from: TicketState; to: TicketState }
  | { seq: number; type: "tool-activity"; slot: number; ticket: string; tool: string }
  | { seq: number; type: "report-filed"; ticket: string; slot: number; status: ReportStatus }
  | { seq: number; type: "mail"; id: string; from: string; to: string; kind: string }
  | { seq: number; type: "gate-result"; ticket: string; slot: number; outcome: GateOutcome }
  | { seq: number; type: "review-result"; ticket: string; reviewer_slot: number; outcome: ReviewOutcome }
  | { seq: number; type: "notice"; level: NoticeLevel; text: string };

export interface BootSnapshot {
  board: Ticket[];
  latest_seq: number;
}

/// The live fleet configuration the top bar shows (real, not placeholders).
export interface FleetConfig {
  target: string;
  branch: string;
  worker_backend: string;
  lead_model: string;
  gate: string;
}

/// The worker slots the dashboard band always shows (handoff §11).
export const WORKER_SLOTS = [1, 2, 3, 4] as const;

/// Board columns, left→right, matching the handoff board order.
export const BOARD_COLUMNS: { state: TicketState; label: string }[] = [
  { state: "backlog", label: "Backlog" },
  { state: "assigned", label: "Assigned" },
  { state: "in-progress", label: "In progress" },
  { state: "in-review", label: "In review" },
  { state: "done", label: "Done" },
];
