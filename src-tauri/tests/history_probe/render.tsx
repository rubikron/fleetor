// The render probe behind `tests/history_row_renders.rs` (WP-27 S2; R8, R19).
//
// It imports the **real** `RunHistory` out of `ui/src/components/RunHistory.tsx`
// and renders it through React's own server renderer against synthetic past
// sessions. Nothing here knows what the markup looks like; every string the Rust
// test asserts on has to come out of the component.
//
// **This is not a JavaScript test runner and must not become one** (C24). It
// renders and prints; it asserts nothing — the same shape as
// `tests/pane_probe/render.tsx`.

import { renderToStaticMarkup } from "react-dom/server";
import { RunHistory } from "../../../ui/src/components/RunHistory";
import type { RunRecord } from "../../../ui/src/fleet/types";
import type { RunsView } from "../../../ui/src/fleet/useRuns";

/// A session that reopens.
const OPENS: RunRecord = {
  id: "2026-09-10T10-00-00Z-aaaa",
  label: "parser rewrite",
  target: "/work/logstat",
  started_ms: 1_789_000_000_000,
  ended_ms: 1_789_003_600_000,
  events: 40,
  messages: 12,
  tasks: 3,
  bytes: 46_618,
  transcripts: 5,
  sessions: "2026-09-10T10-00-00Z-aaaa",
  reopened: 0,
};

/// A session one seat of which has lost its transcript since (R19).
const BLOCKED: RunRecord = {
  ...OPENS,
  id: "2026-09-10T11-00-00Z-bbbb",
  label: "flaky tests",
  cannot_reopen: "worker-2’s session is no longer on disk, so there is nothing to resume",
};

/// A session archived before sessions were recorded at all.
const LEGACY: RunRecord = {
  ...OPENS,
  id: "2026-09-01T09-00-00Z-cccc",
  label: "before sessions",
  sessions: undefined,
  cannot_reopen: "archived before sessions were recorded",
};

const view = (runs: RunRecord[]): RunsView => ({
  runs,
  error: null,
  opening: null,
  generation: 0,
  refresh: () => {},
  rename: async () => {},
  remove: async () => {},
  reopen: async () => {},
  save: async () => null,
  saved: null,
});

const noop = () => {};

process.stdout.write(
  JSON.stringify({
    // One row that opens beside one that cannot.
    mixed: renderToStaticMarkup(<RunHistory runs={view([OPENS, BLOCKED])} onOpened={noop} />),
    // Every row blocked, for different causes.
    nothing_opens: renderToStaticMarkup(
      <RunHistory runs={view([BLOCKED, LEGACY])} onOpened={noop} />,
    ),
  }),
);
