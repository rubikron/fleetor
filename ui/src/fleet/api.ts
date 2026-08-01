// The thin Tauri boundary: `invoke` for actions, an event listener for the live
// feed. Every backend command from src-tauri/src/fleet.rs is wrapped here once so
// components never touch the raw string channel names.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { BootSnapshot, FleetConfig, FleetEvent, Ticket } from "./types";

const FLEET_EVENT = "fleet://event";

/// Start (idempotent) the embedded fleet and get the board snapshot to seed from.
export function bootstrap(): Promise<BootSnapshot> {
  return invoke<BootSnapshot>("fleet_bootstrap");
}

/// Refetch the full board — the source of truth after any ticket move.
export function fetchBoard(): Promise<Ticket[]> {
  return invoke<Ticket[]>("fleet_board");
}

/// Put a ticket on the board as assigned (a real store write that streams back).
export function assign(ticket: Ticket): Promise<void> {
  return invoke("fleet_assign", { ticket });
}

/// The live fleet configuration (real target/branch/worker backend) for the top bar.
export function fetchConfig(): Promise<FleetConfig> {
  return invoke<FleetConfig>("fleet_config");
}

/// Spawn the lead `claude` in the pty as the fleet orchestrator. Spends tokens —
/// the caller gates this behind an explicit confirm.
export function spawnLead(rows: number, cols: number): Promise<void> {
  return invoke("pty_spawn", { rows, cols });
}

/// Subscribe to the live event stream. Returns an unlisten fn for cleanup.
export function onFleetEvent(handler: (event: FleetEvent) => void): Promise<UnlistenFn> {
  return listen<FleetEvent>(FLEET_EVENT, (e) => handler(e.payload));
}
