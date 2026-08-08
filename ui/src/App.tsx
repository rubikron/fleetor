// The shell: top bar, navigation spine, and a workspace that swaps between the
// terminals, the message record, the task board and the activity log. Every view
// stays mounted and is toggled with CSS — the terminals' xterm buffers must
// never be torn down (L7).
//
// UI-polish pass: the dashboard band that used to sit above the terminals — one
// cell per pane, duplicating the dot-plus-name-plus-status the worker tab strip
// already shows — is gone. Its unique data (per-pane model) now lives in each
// pane's own head; the tab strip already owned per-pane status.
//
// The spend gate is an **overlay on the whole workspace**, not a card inside the
// orchestrator pane. Starting the fleet now spawns five processes, four of which
// bill against a real key, so the one screen whose job is stating cost has to
// cover the thing it is gating.

import { useCallback, useEffect, useRef, useState } from "react";
import { TopBar } from "./components/TopBar";
import { Sidebar } from "./components/Sidebar";
import { MessageFeed } from "./components/MessageFeed";
import { TaskBoard } from "./components/TaskBoard";
import { EventFeed } from "./components/EventFeed";
import { SettingsPanel } from "./components/SettingsPanel";
import { TerminalGrid } from "./components/TerminalGrid";
import { StartGate } from "./components/StartGate";
import { RunHistory } from "./components/RunHistory";
import { DevModeBanner } from "./components/DevModeBanner";
import { useFleet } from "./fleet/useFleet";
import { useRuns } from "./fleet/useRuns";
import { useContextGauge } from "./fleet/useContextGauge";
import { useZoom } from "./ui/useZoom";
import { useSidebarCollapse } from "./ui/useSidebarCollapse";
import { usePersistedNav } from "./ui/usePersistedNav";
import { usePaneJump } from "./ui/usePaneJump";
import { useWindowState } from "./ui/useWindowState";
import { useTheme } from "./ui/useTheme";
import { useDevMode } from "./ui/useDevMode";
import { killPane } from "./fleet/api";
import { ORCH, paneSlot, type PaneId, type PaneStatus } from "./fleet/types";

function statusText(ready: boolean, error: string | null, count: number): string {
  if (error) return error;
  if (!ready) return "connecting…";
  return `fleet live · ${count} events`;
}

export function App() {
  const { view, setView, selectedWorker, setSelectedWorker } = usePersistedNav();
  const [started, setStarted] = useState(false);
  const [statuses, setStatuses] = useState<Record<PaneId, PaneStatus>>({});
  // Worker slots that have produced output since the operator last selected
  // that tab — presence only, no count, gold not coral (coral is reserved
  // for the focused-pane frame in TerminalPane.tsx — see item 1 there).
  const [unreadWorkers, setUnreadWorkers] = useState<Set<number>>(() => new Set());
  const fleet = useFleet();
  // Past runs. Independent of `fleet` on purpose: History reads files, so it
  // works on the start gate, before a fleet exists, which is exactly when the
  // operator is deciding what to do next.
  const runs = useRuns();
  // The WP-04 live gauge: polls only once the fleet is actually running —
  // before `started`, no pane exists to sample and every tick would just be
  // an empty roster.
  const gauges = useContextGauge(fleet.ready && started);
  const zoom = useZoom();
  const sidebar = useSidebarCollapse();
  const themeControls = useTheme();
  // WP-16. Read from ~/.fleetor/config.json rather than localStorage, because
  // later packages branch on the same flag from the Rust side. While the mode is
  // off, the settings row below is the only trace of it in the whole app.
  const devMode = useDevMode();
  // Restores the window's saved size/position, then keeps them current. Pure
  // side effect on the OS window — see useWindowState.ts for why a saved
  // position is re-validated against the connected monitors before use.
  useWindowState();

  // One `focus()` callback per pane, registered by TerminalPane itself once
  // it mounts (see its onFocusReady prop). A plain ref, not state — jumping
  // to a pane never needs to re-render App on its own.
  const paneFocusRegistry = useRef<Partial<Record<PaneId, () => void>>>({});
  const registerPaneFocus = useCallback((pane: PaneId, focus: () => void) => {
    paneFocusRegistry.current[pane] = focus;
  }, []);

  // Selecting a worker tab is what "viewing" it means for the unread dot —
  // clear it here rather than in TerminalGrid, since App.tsx already owns
  // both selectedWorker and unreadWorkers.
  const selectWorker = useCallback(
    (slot: number) => {
      setSelectedWorker(slot);
      setUnreadWorkers((prev) => {
        if (!prev.has(slot)) return prev;
        const next = new Set(prev);
        next.delete(slot);
        return next;
      });
    },
    [setSelectedWorker],
  );

  // TerminalPane calls onStatus(pane, "live") on *every* output chunk, not
  // just on a real status change — setStatuses below already early-returns
  // when the status is unchanged, but this callback still fires each time.
  // That gives a per-pane "did output just happen" signal for free, with no
  // change to TerminalPane's mount effect: a hidden (non-selected) worker
  // that just produced output gets marked unread; the currently selected
  // worker and the orchestrator (which has no tab) do not.
  const onStatus = useCallback(
    (pane: PaneId, status: PaneStatus) => {
      setStatuses((prev) => (prev[pane] === status ? prev : { ...prev, [pane]: status }));
      const slot = paneSlot(pane);
      if (slot === null || slot === selectedWorker) return;
      setUnreadWorkers((prev) => (prev.has(slot) ? prev : new Set(prev).add(slot)));
    },
    [selectedWorker],
  );

  // Cmd+1..5 pane jumps (ORCH, worker-1..4 — see usePaneJump.ts). Switches to
  // the fleet view if it isn't already active, selects the worker tab (which
  // also clears its unread dot), and moves keyboard focus into that pane's
  // terminal.
  const jumpToPane = useCallback(
    (pane: PaneId) => {
      setView("fleet");
      const slot = paneSlot(pane);
      if (slot !== null) selectWorker(slot);
      paneFocusRegistry.current[pane]?.();
    },
    [setView, selectWorker],
  );
  usePaneJump(jumpToPane);

  // A pane is never unmounted, so a hidden one sits at 0×0 and its `refit` guard
  // correctly refuses to fit. Nudge on a view change, a worker tab change, and a
  // sidebar collapse toggle: switching worker 2 → 3 takes pane 3 from 0×0 to
  // sized, and collapsing the sidebar resizes the workspace's flex sibling —
  // both leave a pane at a stale grid without this (L8). The visible pane's own
  // ResizeObserver (TerminalPane.tsx) should already catch a collapse-driven
  // resize on its own, since that observer fires on any box-size change
  // regardless of cause; this is the same belt-and-suspenders nudge App.tsx
  // already used for view/worker changes, covering hidden panes too.
  useEffect(() => {
    if (view !== "fleet") return;
    const id = window.setTimeout(() => window.dispatchEvent(new Event("resize")), 0);
    return () => window.clearTimeout(id);
  }, [view, selectedWorker, sidebar.collapsed]);

  const restart = useCallback((pane: PaneId) => {
    // Kill only. The pane's own spawn effect is keyed on `started`, so the tab
    // brings itself back with a fitted size rather than one guessed here.
    void killPane(pane).catch(() => {});
    setStatuses((prev) => ({ ...prev, [pane]: "dead" }));
  }, []);

  return (
    <div className="app">
      <TopBar
        status={statusText(fleet.ready, fleet.error, fleet.feed.length)}
        config={fleet.config}
        zoom={zoom.zoom}
      />
      {devMode.enabled === true && <DevModeBanner />}
      <div className="body">
        <Sidebar
          view={view}
          onSelect={setView}
          messageCount={fleet.messages.length}
          collapsed={sidebar.collapsed}
          onToggleCollapse={sidebar.toggle}
        />
        <main className="workspace">
          <div className="workspace__stage">
            {/* Terminals. Kept mounted; see L7. Hidden before start so empty
                frames don't bleed through the gate's backdrop. */}
            <div className={`stage-view ${view === "fleet" && started ? "" : "is-hidden"}`}>
              <TerminalGrid
                started={started}
                selected={selectedWorker}
                onSelect={selectWorker}
                statuses={statuses}
                unreadWorkers={unreadWorkers}
                onRegisterFocus={registerPaneFocus}
                config={fleet.config}
                fontSize={zoom.terminalFontSize}
                theme={themeControls.theme}
                onStatus={onStatus}
                onRestart={restart}
                gauges={gauges}
              />
            </div>

            <div className={`stage-view ${view === "messages" ? "" : "is-hidden"}`}>
              <MessageFeed messages={fleet.messages} commands={fleet.commands} />
            </div>

            {/* The board (WP-05). Mounted like every other view — `.is-hidden`,
                never conditional rendering (§7.5). Nothing here writes to it:
                the fleet maintains it through `fleet task`. */}
            <div className={`stage-view ${view === "tasks" ? "" : "is-hidden"}`}>
              <TaskBoard tasks={fleet.tasks} />
            </div>

            <div className={`stage-view ${view === "activity" ? "" : "is-hidden"}`}>
              <EventFeed feed={fleet.feed} />
            </div>

            {/* Past runs (WP-11). Its own copies of the three components, fed
                from archived logs — so no live list can ever be handed a past
                run's events, and no past run can be sent to. */}
            <div className={`stage-view ${view === "history" ? "" : "is-hidden"}`}>
              <RunHistory runs={runs} />
            </div>

            <div className={`stage-view ${view === "settings" ? "" : "is-hidden"}`}>
              <SettingsPanel
                theme={themeControls.theme}
                onToggleTheme={themeControls.toggle}
                devMode={devMode}
              />
            </div>


            {!started && (
              <StartGate
                config={fleet.config}
                onStart={() => {
                  setStatuses({ [ORCH]: "idle" });
                  setView("fleet");
                  void fleet.start().then(() => setStarted(true));
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
