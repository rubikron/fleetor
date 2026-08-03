// The shell: top bar, navigation spine, the always-visible pane band, and a
// workspace that swaps between the terminals, the message record, the topology
// and the activity log. Every view stays mounted and is toggled with CSS — the
// terminals' xterm buffers must never be torn down (L7).
//
// The spend gate is an **overlay on the whole workspace**, not a card inside the
// orchestrator pane. Starting the fleet now spawns five processes, four of which
// bill against a real key, so the one screen whose job is stating cost has to
// cover the thing it is gating.

import { useCallback, useEffect, useState } from "react";
import { TopBar } from "./components/TopBar";
import { Sidebar, type View } from "./components/Sidebar";
import { DashboardBand } from "./components/DashboardBand";
import { MessageFeed } from "./components/MessageFeed";
import { EventFeed } from "./components/EventFeed";
import { TerminalGrid } from "./components/TerminalGrid";
import { StartGate } from "./components/StartGate";
import { FleetGraph } from "./views/FleetGraph";
import { useFleet } from "./fleet/useFleet";
import { killPane } from "./fleet/api";
import { ORCH, paneSlot, type PaneId, type PaneStatus } from "./fleet/types";

function statusText(ready: boolean, error: string | null, count: number): string {
  if (error) return error;
  if (!ready) return "connecting…";
  return `fleet live · ${count} events`;
}

export function App() {
  const [view, setView] = useState<View>("fleet");
  const [started, setStarted] = useState(false);
  const [selectedWorker, setSelectedWorker] = useState(1);
  const [statuses, setStatuses] = useState<Record<PaneId, PaneStatus>>({});
  const fleet = useFleet();

  const onStatus = useCallback((pane: PaneId, status: PaneStatus) => {
    setStatuses((prev) => (prev[pane] === status ? prev : { ...prev, [pane]: status }));
  }, []);

  // A pane is never unmounted, so a hidden one sits at 0×0 and its `refit` guard
  // correctly refuses to fit. Nudge on **both** a view change and a worker tab
  // change: switching worker 2 → 3 takes pane 3 from 0×0 to sized, and without
  // this it returns at a stale grid (L8).
  useEffect(() => {
    if (view !== "fleet") return;
    const id = window.setTimeout(() => window.dispatchEvent(new Event("resize")), 0);
    return () => window.clearTimeout(id);
  }, [view, selectedWorker]);

  const restart = useCallback((pane: PaneId) => {
    // Kill only. The pane's own spawn effect is keyed on `started`, so the tab
    // brings itself back with a fitted size rather than one guessed here.
    void killPane(pane).catch(() => {});
    setStatuses((prev) => ({ ...prev, [pane]: "dead" }));
  }, []);

  const openPane = useCallback((pane: PaneId) => {
    setView("fleet");
    const slot = paneSlot(pane);
    if (slot) setSelectedWorker(slot);
  }, []);

  return (
    <div className="app">
      <TopBar
        status={statusText(fleet.ready, fleet.error, fleet.feed.length)}
        config={fleet.config}
      />
      <div className="body">
        <Sidebar view={view} onSelect={setView} messageCount={fleet.messages.length} />
        <main className="workspace">
          <DashboardBand statuses={statuses} config={fleet.config} onOpen={openPane} />

          <div className="workspace__stage">
            {/* Terminals. Kept mounted; see L7. */}
            <div className={`stage-view ${view === "fleet" ? "" : "is-hidden"}`}>
              <TerminalGrid
                started={started}
                selected={selectedWorker}
                onSelect={setSelectedWorker}
                statuses={statuses}
                onStatus={onStatus}
                onRestart={restart}
              />
            </div>

            <div className={`stage-view ${view === "messages" ? "" : "is-hidden"}`}>
              <MessageFeed messages={fleet.messages} />
            </div>

            <div className={`stage-view ${view === "topology" ? "" : "is-hidden"}`}>
              <FleetGraph
                statuses={statuses}
                feed={fleet.feed}
                leadModel={fleet.config?.lead_model ?? "opus (operator)"}
              />
            </div>

            <div className={`stage-view ${view === "activity" ? "" : "is-hidden"}`}>
              <EventFeed feed={fleet.feed} />
            </div>

            {!started && (
              <StartGate
                config={fleet.config}
                onStart={() => {
                  setStatuses({ [ORCH]: "idle" });
                  setStarted(true);
                }}
                onTargetChanged={fleet.refreshConfig}
              />
            )}
          </div>
        </main>
      </div>
    </div>
  );
}
