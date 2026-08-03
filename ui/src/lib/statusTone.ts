import type { PaneStatus } from "../fleet/types";

// Shared between the worker tab strip and every pane head so a pane's dot
// color and its status word never drift apart between the two places it's
// shown. Status is never colour alone (handoff §"Visual language") — every
// caller pairs the tone with STATUS_LABEL's text.
export function statusTone(status: PaneStatus): "accent" | "red" | "muted" {
  if (status === "live") return "accent";
  if (status === "dead") return "red";
  return "muted";
}

export const STATUS_LABEL: Record<PaneStatus, string> = {
  idle: "standby",
  live: "live",
  dead: "exited",
};
