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
} from "./types";

const FLEET_EVENT = "fleet://event";

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
