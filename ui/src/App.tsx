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
import { useCriticInterview } from "./ui/useCriticInterview";
import { TerminalPane } from "./components/TerminalPane";
import { killPane, onEvaluatorWake } from "./fleet/api";
import { CRITIC, EVALUATOR, ORCH, paneSlot, type PaneId, type PaneStatus } from "./fleet/types";

// Deeper than a worker's 2,000 and deeper than orch's 10,000: this pane reads a
// whole run's archive and prints a retro at the end of it, and the operator
// scrolls back through the reasoning rather than through the last few turns.
const EVALUATOR_SCROLLBACK = 20000;

// The Critic's, for the identical reason (WP-20): it reads a whole run's
// archive, prints findings grouped under six headings with a citation on each,
// and the operator scrolls back through them to check the ones they care about.
const CRITIC_SCROLLBACK = 20000;

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

  // **The evaluator's wake** (D-073). Latched, and it only ever goes true: the
  // terminal below is mounted for the life of the app either way, and this flag
  // decides whether it may spawn a pty (TerminalPane's `started`, the same gate
  // the start gate uses) and whether the operator sees the terminal or the
  // sentence explaining what it is waiting for. Nothing here can start the
  // evaluator — only Rust decides a handoff cleared readiness, and it says so
  // with this event.
  const [evaluatorAwake, setEvaluatorAwake] = useState(false);
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void onEvaluatorWake(() => setEvaluatorAwake(true)).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // **The Critic starts when the operator says so** (WP-20, D-076), which is the
  // one place it differs from the evaluator's view above and the difference is
  // the whole point: the evaluator's sequencing is evidence and must not be
  // anticipated, while the Critic answers an ordinary question the operator asks
  // whenever they want it answered. Latched like the wake, and for the same
  // reason — the terminal below is mounted for the life of the app either way,
  // and this flag only decides whether it may spawn a pty.
  const [criticStarted, setCriticStarted] = useState(false);

  // **The interview gate** (WP-21 stage A). Rust holds it, because it decides
  // whether a `fleet send` from inside the Critic resolves at all; this is a
  // view of that state, on the `useDevMode` shape, with no optimistic flip.
  // Keyed on the fleet being up: with no run there is nothing to interview, and
  // that is also when the control is disabled below.
  const interview = useCriticInterview(started);

  // Dev mode can be turned off while `evaluator` is the persisted view, which
  // would leave the operator on a view with no row in the rail to leave by.
  // `=== false` and not `!== true`: while the backend has not answered, the
  // right move is to do nothing rather than to bounce off the view.
  useEffect(() => {
    if (devMode.enabled === false && view === "evaluator") setView("fleet");
  }, [devMode.enabled, view, setView]);

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
          devMode={devMode.enabled}
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
                panes={fleet.panes}
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

            {/* The Critic (WP-20, D-076). A view like every other: always
                mounted, toggled with `.is-hidden`, never unmounted (§7 rule 5).

                Present with dev mode off, because it is a product feature rather
                than an instrument of an experiment — and it has a Start button,
                which the view below deliberately does not. The two sit next to
                each other and answer different questions: this one reports what
                the fleet *did*, cited from the run's own archive, and has no
                view at all on whether the code is right.

                Nothing here can send: the pane is spawned with no fleet socket,
                so a `fleet send` typed into it reaches nothing. Anything worth
                acting on the operator forwards from the composer they already
                have (the arc's D6), which keeps the human as the only writer. */}
            <div className={`stage-view ${view === "critic" ? "" : "is-hidden"}`}>
              <div className="critic-view">
                {/* **The interview gate** (WP-21 stage A). Always in the tree,
                    never conditionally rendered: the view is hidden with CSS and
                    nothing inside it may leave the tree, or the terminal below
                    goes with it (§7 rule 5).

                    The cost is *printed*, not hovered. WP-21 makes "the operator
                    can tell, before pressing the control, that it will spend the
                    fleet's turns" an acceptance criterion, and the arc already
                    records what a tooltip buys: ticket 08's Critic/Evaluator
                    distinction is "a hover tooltip plus each view's own lede",
                    and it is named there as the weak part. A hover cannot be
                    seen on the way to the button and does not exist for a
                    keyboard press at all. So the sentence sits beside the
                    control, in the token the palette reserves for cost.

                    The state is said in words as well as in the verb, because
                    "Open interview" alone reads as either the state or the
                    action depending on who is looking. */}
                <div className="critic-view__interview">
                  <span
                    className={`critic-view__gate critic-view__gate--${
                      started ? (interview.open === null ? "unknown" : interview.open ? "open" : "closed") : "norun"
                    }`}
                  >
                    Interview:{" "}
                    {started
                      ? interview.open === null
                        ? "checking…"
                        : interview.open
                          ? "open"
                          : "closed"
                      : "no run"}
                  </span>
                  <button
                    type="button"
                    className="critic-view__interview-toggle"
                    onClick={interview.toggle}
                    disabled={interview.open === null}
                    title={
                      started
                        ? "Open puts the Critic on the fleet's address book; closed, a send from it fails to resolve"
                        : "Start the fleet first — there is no run to interview"
                    }
                  >
                    {interview.open === true ? "Close interview" : "Open interview"}
                  </button>
                  <p className="critic-view__cost">
                    Opening this spends the fleet&rsquo;s turns: every question the Critic asks costs{" "}
                    <code>orch</code> or a worker a turn it would have spent on the run, and asking
                    mid-run can perturb it. Closed, the Critic is not an address and can spend
                    nothing.
                  </p>
                  <p className="critic-view__interview-error">{interview.error}</p>
                </div>
                <div className={`critic-view__waiting ${criticStarted ? "is-hidden" : ""}`}>
                  <p className="critic-view__lede">The Critic reads the run in progress.</p>
                  <p className="critic-view__note">
                    It reports what the fleet did — idle panes, a block marked done before its
                    check ran, a message that got no reply — with a timestamp, a file and a line
                    from the archive behind every finding. A claim it cannot point at is not a
                    finding, and it never says whether the work was any good: it has no way to
                    know. It is a real terminal, so argue with a finding or ask it to look again.
                  </p>
                  <button
                    type="button"
                    className="critic-view__start"
                    onClick={() => setCriticStarted(true)}
                    disabled={!started}
                    title={started ? undefined : "Start the fleet first — there is no run yet"}
                  >
                    Start
                  </button>
                </div>
                <div className={`critic-view__terminal ${criticStarted ? "" : "is-hidden"}`}>
                  <TerminalPane
                    pane={CRITIC}
                    label="critic"
                    scrollback={CRITIC_SCROLLBACK}
                    started={criticStarted}
                    status={statuses[CRITIC] ?? "idle"}
                    fontSize={zoom.terminalFontSize}
                    theme={themeControls.theme}
                    onStatus={onStatus}
                  />
                </div>
              </div>
            </div>

            {/* The evaluator (D-073), which used to be a second OS window. It is
                a view like every other: always mounted, toggled with
                `.is-hidden`, never unmounted (§7 rule 5). What the window bought
                — a root that does not race the main one for the fleet — was
                never a reason for a *window*, only for that root not being a
                second `App`, and one view inside one root has no race at all.

                The terminal is mounted from launch and hidden until the wake,
                rather than mounted at the wake. Both survive a view switch, but
                only this one is impossible to get wrong later: there is no
                mount-once latch to reason about, and `started` — TerminalPane's
                own spawn gate — is what keeps a pty from existing before the
                handoff. Note the absence of a button. The evaluator wakes when
                `orch` hands off; the sentence below is the whole control
                surface, on purpose. */}
            <div className={`stage-view ${view === "evaluator" ? "" : "is-hidden"}`}>
              <div className="evaluator-view">
                <div className={`evaluator-view__waiting ${evaluatorAwake ? "is-hidden" : ""}`}>
                  <p className="evaluator-view__lede">The evaluator wakes on a handoff.</p>
                  <p className="evaluator-view__note">
                    When <code>orch</code> reports the mission met, this becomes a terminal and
                    the retro starts here. There is no button — the sequencing is the design,
                    and a run it could be started ahead of would not be evidence of anything.
                  </p>
                </div>
                <div className={`evaluator-view__terminal ${evaluatorAwake ? "" : "is-hidden"}`}>
                  <TerminalPane
                    pane={EVALUATOR}
                    label="evaluator"
                    scrollback={EVALUATOR_SCROLLBACK}
                    started={evaluatorAwake}
                    status={statuses[EVALUATOR] ?? "idle"}
                    fontSize={zoom.terminalFontSize}
                    theme={themeControls.theme}
                    onStatus={onStatus}
                  />
                </div>
              </div>
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
