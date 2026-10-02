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
import { TaskBoard } from "./components/TaskBoard";
import { SettingsPanel } from "./components/SettingsPanel";
import { TerminalGrid } from "./components/TerminalGrid";
import { Homepage } from "./components/Homepage";
import { RunHistory } from "./components/RunHistory";
import { FeedView } from "./components/FeedView";
import { useFleet } from "./fleet/useFleet";
import { useRuns } from "./fleet/useRuns";
import { useContextGauge } from "./fleet/useContextGauge";
import { useZoom } from "./ui/useZoom";
import { useSidebarCollapse } from "./ui/useSidebarCollapse";
import { usePersistedNav } from "./ui/usePersistedNav";
import { usePaneJump } from "./ui/usePaneJump";
import { useWindowState } from "./ui/useWindowState";
import { useTheme } from "./ui/useTheme";
import { killPane } from "./fleet/api";
import { ORCH, type PaneId, type PaneStatus } from "./fleet/types";

export function App() {
  const { view, setView } = usePersistedNav();
  const [started, setStarted] = useState(false);
  const [statuses, setStatuses] = useState<Record<PaneId, PaneStatus>>({});
  const [selectedPane, setSelectedPane] = useState<PaneId>(ORCH);
  const [unreadPanes, setUnreadPanes] = useState<Set<PaneId>>(() => new Set());
  const fleet = useFleet();
  // Past runs. Independent of `fleet` on purpose: History reads files, so it
  // works on the start gate, before a fleet exists, which is exactly when the
  // operator is deciding what to do next.
  const runs = useRuns();
  // Read again whenever History is shown (D-085). The list was fetched once at
  // mount, so a session archived after that — by a fleet start, or anything else —
  // stayed invisible until a rename, delete or relaunch.
  const refreshRuns = runs.refresh;
  useEffect(() => {
    if (view === "history") refreshRuns();
  }, [view, refreshRuns]);
  // The WP-04 live gauge: polls only once the fleet is actually running —
  // before `started`, no pane exists to sample and every tick would just be
  // an empty roster.
  const gauges = useContextGauge(fleet.ready && started);
  const zoom = useZoom();
  const sidebar = useSidebarCollapse();
  const themeControls = useTheme();
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

  const selectPane = useCallback(
    (pane: PaneId) => {
      setSelectedPane(pane);
      setUnreadPanes((prev) => {
        if (!prev.has(pane)) return prev;
        const next = new Set(prev);
        next.delete(pane);
        return next;
      });
    },
    [],
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
      if (pane === selectedPane) return;
      setUnreadPanes((prev) => (prev.has(pane) ? prev : new Set(prev).add(pane)));
    },
    [selectedPane],
  );

  // Cmd+1..5 pane jumps (ORCH, worker-1..4 — see usePaneJump.ts). Switches to
  // the fleet view if it isn't already active, selects the worker tab (which
  // also clears its unread dot), and moves keyboard focus into that pane's
  // terminal.
  const jumpToPane = useCallback(
    (pane: PaneId) => {
      setView("fleet");
      selectPane(pane);
      paneFocusRegistry.current[pane]?.();
    },
    [setView, selectPane],
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
  }, [view, selectedPane, sidebar.collapsed]);

  const restart = useCallback((pane: PaneId) => {
    // Kill only. The pane's own spawn effect is keyed on `started`, so the tab
    // brings itself back with a fitted size rather than one guessed here.
    void killPane(pane).catch(() => {});
    setStatuses((prev) => ({ ...prev, [pane]: "dead" }));
  }, []);

  return (
    <div className="app">
      <TopBar
        config={fleet.config}
        error={fleet.error}
        zoom={zoom.zoom}
      />
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
            <div className={`stage-view ${view === "fleet" ? "" : "is-hidden"}`}>
              {/* Keyed on the reopen generation (WP-27, R2): a reopen kills every
                  pane and re-enters bootstrap, and `TerminalPane` spawns from a
                  mount effect — so without a remount the operator lands on five
                  dead terminals. This is the one place a stage-view is allowed to
                  unmount (building.md §7.5's exception), because the buffers being
                  discarded belong to the run that just ended. */}
              <TerminalGrid
                key={`fleet-${runs.generation}`}
                started={started}
                selected={selectedPane}
                onSelect={selectPane}
                statuses={statuses}
                unreadPanes={unreadPanes}
                onRegisterFocus={registerPaneFocus}
                panes={fleet.panes}
                fontSize={zoom.terminalFontSize}
                theme={themeControls.theme}
                onStatus={onStatus}
                onRestart={restart}
                gauges={gauges}
                messages={fleet.messages}
                tasks={fleet.tasks}
              />
            </div>

            <div className={`stage-view ${view === "home" ? "" : "is-hidden"}`}>
              <Homepage
                config={fleet.config}
                runs={runs.runs}
                onStart={() => {
                  setStatuses({ [ORCH]: "idle" });
                  setView("fleet");
                  void fleet.start().then(() => {
                    setStarted(true);
                    runs.refresh();
                  });
                }}
                onTargetChanged={fleet.refreshConfig}
              />
            </div>

            <div className={`stage-view ${view === "feed" ? "" : "is-hidden"}`}>
              <FeedView
                messages={fleet.messages}
                commands={fleet.commands}
                feed={fleet.feed}
              />
            </div>

            <div className={`stage-view ${view === "tasks" ? "" : "is-hidden"}`}>
              <TaskBoard tasks={fleet.tasks} />
            </div>

            <div className={`stage-view ${view === "history" ? "" : "is-hidden"}`}>
              <RunHistory runs={runs} onOpened={() => setView("fleet")} />
            </div>

            <div className={`stage-view ${view === "settings" ? "" : "is-hidden"}`}>
              <SettingsPanel
                theme={themeControls.theme}
                onToggleTheme={themeControls.toggle}
              />
            </div>


          </div>
        </main>
      </div>
    </div>
  );
}
