import type { PaneStatus } from "../fleet/types";

// Shared between the mission control cards and every pane head so a pane's dot
// color and its status word never drift apart between the two places it's
// shown. Status is never colour alone (handoff §"Visual language") — every
// caller pairs the tone with STATUS_LABEL's text. Live is green: coral is for attention.
export function statusTone(status: PaneStatus): "green" | "red" | "muted" {
  if (status === "live") return "green";
  if (status === "dead") return "red";
  return "muted";
}

export const STATUS_LABEL: Record<PaneStatus, string> = {
  idle: "standby",
  live: "live",
  dead: "exited",
};
