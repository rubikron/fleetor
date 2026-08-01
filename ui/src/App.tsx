// The shell layout (handoff §11): top bar (with the demoted dev toolbar), the
// navigation spine, the always-visible dashboard band, and a content area that
// swaps between the Tickets view (terminal ↔ board, resizable + collapsible), the
// Event log, and the live Fleet topology. All three views stay mounted and are
// toggled with CSS so the terminal's xterm buffer is never torn down on a tab
// switch (the 4e-1 fix); the board tab nudges a resize on return so the terminal
// refits and `claude` repaints.

import { useEffect, useRef, useState } from "react";
import { Panel, PanelGroup, PanelResizeHandle, type ImperativePanelHandle } from "react-resizable-panels";
import { TopBar } from "./components/TopBar";
import { Sidebar, type View } from "./components/Sidebar";
import { DashboardBand } from "./components/DashboardBand";
import { TicketBoard } from "./components/TicketBoard";
import { EventFeed } from "./components/EventFeed";
import { TerminalPane } from "./components/TerminalPane";
import { FleetGraph } from "./views/FleetGraph";
import { useFleet } from "./fleet/useFleet";

function statusText(ready: boolean, error: string | null, count: number): string {
  if (error) return error;
  if (!ready) return "connecting…";
  return `fleet live · ${count} events`;
}

export function App() {
  const [view, setView] = useState<View>("board");
  const [boardCollapsed, setBoardCollapsed] = useState(false);
  const [started, setStarted] = useState(false);
  const boardPanel = useRef<ImperativePanelHandle>(null);
  const fleet = useFleet();

  // The terminal is never unmounted; when the board tab returns to view, nudge a
  // resize so the pane refits its now-visible box and `claude` repaints.
  useEffect(() => {
    if (view !== "board") return;
    const id = window.setTimeout(() => window.dispatchEvent(new Event("resize")), 0);
    return () => window.clearTimeout(id);
  }, [view]);

  return (
    <div className="app">
      <TopBar
        status={statusText(fleet.ready, fleet.error, fleet.feed.length)}
        config={fleet.config}
      />
      <div className="body">
        <Sidebar view={view} onSelect={setView} unread={fleet.feed.length} />
        <main className="workspace">
          <DashboardBand
            workers={fleet.workers}
            board={fleet.board}
            live={started}
            config={fleet.config}
            onNavigate={setView}
          />

          {/* Tickets view: terminal ↔ board, resizable + collapsible. Kept mounted. */}
          <div className={`split-wrap ${view === "board" ? "" : "is-hidden"}`} style={{ flex: "1 1 auto", minHeight: 0, display: "flex" }}>
            <PanelGroup direction="horizontal" autoSaveId="fleetor-shell-split" className="split">
              <Panel defaultSize={64} minSize={32} className="pane-slot">
                <TerminalPane started={started} onStart={() => setStarted(true)} config={fleet.config} />
              </Panel>
              <PanelResizeHandle className="divider">
                <span className="divider__grip" />
              </PanelResizeHandle>
              <Panel
                ref={boardPanel}
                defaultSize={36}
                minSize={22}
                collapsible
                collapsedSize={4}
                onCollapse={() => setBoardCollapsed(true)}
                onExpand={() => setBoardCollapsed(false)}
                className="pane-slot"
              >
                {boardCollapsed ? (
                  <div className="board-rail" role="button" tabIndex={0} onClick={() => boardPanel.current?.expand()}>
                    <span className="board-rail__label">board ▸</span>
                  </div>
                ) : (
                  <div className="board-pane">
                    <div className="pane__head">
                      <span className="mono">board</span>
                      <span className="grow" />
                      <button className="pane__ctl" onClick={() => boardPanel.current?.collapse()}>
                        Collapse
                      </button>
                    </div>
                    <TicketBoard board={fleet.board} />
                  </div>
                )}
              </Panel>
            </PanelGroup>
          </div>

          {/* Event log view */}
          <div className={view === "events" ? "" : "is-hidden"} style={{ flex: "1 1 auto", minHeight: 0, display: "flex" }}>
            <EventFeed feed={fleet.feed} />
          </div>

          {/* Fleet topology view */}
          <div className={view === "fleet" ? "" : "is-hidden"} style={{ flex: "1 1 auto", minHeight: 0, display: "flex" }}>
            <FleetGraph workers={fleet.workers} feed={fleet.feed} />
          </div>
        </main>
      </div>
    </div>
  );
}
