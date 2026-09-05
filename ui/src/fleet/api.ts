// The thin Tauri boundary: `invoke` for actions, event listeners for the live
// feed and the pty streams. Every backend command is wrapped here once so no
// component ever holds a raw channel name or an argument spelling.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  paneKey,
  type BootSnapshot,
  type FleetConfig,
  type FleetEvent,
  type PaneEntry,
  type PaneId,
  type RunRecord,
} from "./types";

const FLEET_EVENT = "fleet://event";
// Must match `EVENT_EVALUATOR_WAKE` in src-tauri/src/fleet.rs. Listening on the
// wrong name renders nothing and reports no error (see types.ts:54).
const EVALUATOR_WAKE = "evaluator://wake";

/// Start (idempotent) the embedded fleet: store, event bus, hub socket.
export function bootstrap(): Promise<BootSnapshot> {
  return invoke<BootSnapshot>("fleet_bootstrap");
}

/// The live fleet configuration — what a click will run, and where.
export function fetchConfig(): Promise<FleetConfig> {
  return invoke<FleetConfig>("fleet_config");
}

/// Ask the operator for a repo to point the fleet at. Resolves to `null` if the
/// picker was dismissed.
export function pickTarget(): Promise<string | null> {
  return invoke<string | null>("fleet_pick_target");
}

/// Record a typed target folder. Rejects with a message the operator can act on
/// when the path is empty, missing, or not a directory; resolves with the
/// canonical path actually written, which may differ from what was typed
/// (`~` expanded, `..` and symlinks resolved).
export function setTarget(path: string): Promise<string> {
  return invoke<string>("fleet_set_target", { path });
}

/// Every pane and its state, each worker's context gauge attached when one
/// could be sampled (WP-04). The same `AppCommand::Roster` a `fleet roster`
/// from inside a pane reaches over the socket — this is the UI's on-demand
/// poll of it, not a second source of truth. Read-only: nothing here can fail
/// a `fleet send`, and a rejected call renders as "figures unavailable," not
/// a crash — see `useContextGauge`.
export function fetchRoster(): Promise<PaneEntry[]> {
  return invoke<PaneEntry[]>("fleet_roster");
}

// --- the operator's own messages (WP-07) --------------------------------------

/// What one composed message did, in the fleet's own three words.
export interface OperatorSend {
  /// `accepted` — the bytes reached a live pty, which is **not** a claim the
  /// agent read them (L3). `undelivered` — they did not, and `detail` says
  /// which pane and why. `recorded` — it entered the log and no pty exists.
  /// There is no fourth word, and none of them is "delivered".
  outcome: "accepted" | "undelivered" | "recorded";
  /// The message id, or a broadcast's shared group id.
  id: string;
  detail?: string;
}

/// Send a message **as the operator**, through the hub every pane uses.
///
/// `target` is a pane name, `"all"` for a broadcast, or `"reply"` for whoever
/// last messaged the operator — the fleet's three message verbs and no fourth
/// thing. Rejects with a sentence the operator can act on (an empty body, a
/// name that is not a participant, nobody to reply to).
export function sendAsOperator(target: PaneId | "all" | "reply", text: string): Promise<OperatorSend> {
  return invoke<OperatorSend>("fleet_send", { target, text });
}

// --- dev mode (WP-16) ---------------------------------------------------------
//
// The mode is stored on the Rust side (`dev_mode` in ~/.fleetor/config.json),
// not in localStorage where the theme and the nav selection live: later packages
// branch on it from code that has no webview, and two copies of a mode is one
// copy too many. Both calls work before the fleet is bootstrapped.

/// Whether the app is in dev mode.
export function fetchDevMode(): Promise<boolean> {
  return invoke<boolean>("dev_mode_get");
}

/// Turn dev mode on or off, persistently. Resolves with what is now *stored* —
/// render that, not the value that was asked for.
export function setDevMode(enabled: boolean): Promise<boolean> {
  return invoke<boolean>("dev_mode_set", { enabled });
}

// --- the Critic's interview (WP-21 stage A) -----------------------------------
//
// Whether the Critic is currently an *address*. Closed, `fleet send orch` from
// inside it fails at resolution the way a send to a pane that does not exist
// fails — nothing is accepted and then dropped. Open, the same call reaches the
// pane and spends its turn.
//
// The state lives on the Rust side because it is a property of the run, not of
// this webview: it survives a `/clear` and a pane restart, and code with no
// webview decides whether a send resolves. So these two calls are a *view* of
// it, exactly as `dev_mode_get`/`dev_mode_set` are a view of the mode — never a
// second copy.

/// Whether the operator currently has the interview open.
export function fetchCriticInterview(): Promise<boolean> {
  return invoke<boolean>("critic_interview_is_open");
}

/// Open or close the interview. Resolves with what is now *stored* — render
/// that, not the value that was asked for. Both edges write a `notice` to the
/// run's event log on the Rust side: this changes what is possible.
export function setCriticInterview(open: boolean): Promise<boolean> {
  return invoke<boolean>("critic_interview_open", { open });
}

// --- panes --------------------------------------------------------------------

/// Spawn one pane's `claude` under a pty. Spends tokens — every caller is behind
/// the explicit start gate.
export function spawnPane(pane: PaneId, rows: number, cols: number): Promise<void> {
  return invoke("pty_spawn", { pane, rows, cols });
}

/// Relay operator keystrokes to a pane.
export function writePane(pane: PaneId, data: string): Promise<void> {
  return invoke("pty_write", { pane, data });
}

export function resizePane(pane: PaneId, rows: number, cols: number): Promise<void> {
  return invoke("pty_resize", { pane, rows, cols });
}

/// Stop one pane, leaving its tab in place — the escape hatch when a pane wedges.
export function killPane(pane: PaneId): Promise<void> {
  return invoke("pty_kill", { pane });
}

// --- past runs (WP-11) --------------------------------------------------------
//
// Read-or-relabel only. There is deliberately no "resume this run" call: the
// panes that made it are gone and their context went with them.

/// Every archived run, newest first. Works before the fleet is started.
export function listRuns(): Promise<RunRecord[]> {
  return invoke<RunRecord[]>("runs_list");
}

/// One archived run's whole log. Takes the same `after` cursor the live replay
/// does, so the History views can be fed by the identical splitting logic.
export function runEvents(id: string, after = 0): Promise<FleetEvent[]> {
  return invoke<FleetEvent[]>("run_events", { id, after });
}

/// Give a run a name that means something. The only mutable thing about an archive.
export function renameRun(id: string, label: string): Promise<void> {
  return invoke("run_rename", { id, label });
}

/// Delete a run and its directory. There is no undo — the caller confirms.
export function deleteRun(id: string): Promise<void> {
  return invoke("run_delete", { id });
}

/// Save a run's JSON export. Opens a native save dialog on the Rust side, so no
/// dialog plugin is needed here. Resolves to `null` when the operator dismissed
/// it — a cancel, not a failure, and it must not be shown as one.
export function exportRun(id: string): Promise<string | null> {
  return invoke<string | null>("run_export", { id });
}

// --- streams ------------------------------------------------------------------

/// Subscribe to the live event stream. Returns an unlisten fn for cleanup.
export function onFleetEvent(handler: (event: FleetEvent) => void): Promise<UnlistenFn> {
  return listen<FleetEvent>(FLEET_EVENT, (e) => handler(e.payload));
}

/// Subscribe to one pane's output. Per-pane channels are a correctness boundary
/// before a performance one: this listener *cannot* be woken by another pane's
/// bytes, because it never hears that name.
export function onPaneOutput(pane: PaneId, handler: (base64: string) => void): Promise<UnlistenFn> {
  return listen<string>(`pty://output/${paneKey(pane)}`, (e) => handler(e.payload));
}

/// Subscribe to one pane's exit.
export function onPaneExit(pane: PaneId, handler: () => void): Promise<UnlistenFn> {
  return listen(`pty://exit/${paneKey(pane)}`, () => handler());
}

/// The evaluator woke (D-073). Fires once per handoff that cleared the Rust
/// side's readiness check — dev mode on, a grader compiled in, and a prepared
/// mission — which is why this is its own event rather than something derived
/// from the `FleetEvent::Handoff` the feed already carries: most handoffs must
/// wake nothing at all, and only Rust knows which ones.
export function onEvaluatorWake(handler: () => void): Promise<UnlistenFn> {
  return listen(EVALUATOR_WAKE, () => handler());
}
