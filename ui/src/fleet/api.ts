// The thin Tauri boundary: `invoke` for actions, an event listener for the live
// feed. Every backend command from src-tauri/src/fleet.rs is wrapped here once so
// components never touch the raw string channel names.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { BootSnapshot, FleetEvent, Ticket } from "./types";

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

/// Kick the scripted 4e-1 demo lifecycle (no tokens, no processes).
export function runDemo(): Promise<void> {
  return invoke("fleet_demo");
}

/// Subscribe to the live event stream. Returns an unlisten fn for cleanup.
export function onFleetEvent(handler: (event: FleetEvent) => void): Promise<UnlistenFn> {
  return listen<FleetEvent>(FLEET_EVENT, (e) => handler(e.payload));
}
