// The shell layout (handoff §11): top bar, navigation spine, the always-visible
// dashboard band, and a content area that swaps between the board+terminal view
// and the full event log. The band and top bar never move; only the workspace
// pane below them changes with the selected view.

import { useEffect, useState } from "react";
import { TopBar } from "./components/TopBar";
import { Sidebar, type View } from "./components/Sidebar";
import { DashboardBand } from "./components/DashboardBand";
import { TicketBoard } from "./components/TicketBoard";
import { EventFeed } from "./components/EventFeed";
import { TerminalPane } from "./components/TerminalPane";
import { assign, runDemo } from "./fleet/api";
import { useFleet } from "./fleet/useFleet";
import type { Ticket } from "./fleet/types";

let assignSeq = 0;

function newTicket(): Ticket {
  assignSeq += 1;
  const id = `T-${900 + assignSeq}`;
  return {
    id,
    title: "manual ticket from the shell",
    body: "Assigned by hand to prove the invoke → store → event → UI round-trip.",
    files_owned: ["src/lib.rs"],
    slot: ((assignSeq - 1) % 4) + 1,
    state: "backlog",
    budget: { wall_secs: 300, max_tokens: null },
  };
}

export function App() {
  const [view, setView] = useState<View>("board");
  const fleet = useFleet();

  // The terminal is never unmounted (below) so its xterm buffer survives a tab
  // switch. When the board tab comes back into view, nudge a resize so the pane
  // refits to its now-visible box and `claude` repaints its alternate screen.
  useEffect(() => {
    if (view !== "board") return;
    const id = window.setTimeout(() => window.dispatchEvent(new Event("resize")), 0);
    return () => window.clearTimeout(id);
  }, [view]);

  return (
    <div className="app">
      <TopBar />
      <div className="body">
        <Sidebar view={view} onSelect={setView} unread={fleet.feed.length} />
        <main className="workspace">
          <DashboardBand workers={fleet.workers} board={fleet.board} />

          <div className="actions">
            <button className="btn btn--primary" onClick={() => void runDemo()} disabled={!fleet.ready}>
              Run demo lifecycle
            </button>
            <button className="btn" onClick={() => void assign(newTicket())} disabled={!fleet.ready}>
              Assign a ticket
            </button>
            <span className="actions__status">
              {fleet.error ? (
                <span className="text-red">{fleet.error}</span>
              ) : fleet.ready ? (
                <span className="text-green">fleet live · {fleet.feed.length} events</span>
              ) : (
                "connecting…"
              )}
            </span>
          </div>

          {/* Both views stay mounted; we toggle visibility with CSS so the
              terminal's xterm instance and buffer are never torn down on a tab
              switch (unmounting it leaves a blank pane the running pty won't
              repaint). */}
          <div className={`split ${view === "board" ? "" : "is-hidden"}`}>
            <TerminalPane />
            <div className="board-pane">
              <TicketBoard board={fleet.board} />
            </div>
          </div>
          <div className={`events-view ${view === "events" ? "" : "is-hidden"}`}>
            <EventFeed feed={fleet.feed} />
          </div>
        </main>
      </div>
    </div>
  );
}
