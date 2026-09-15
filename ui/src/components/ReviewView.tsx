import { useState } from "react";
import { TerminalPane } from "./TerminalPane";
import { CRITIC, EVALUATOR, type PaneId, type PaneStatus } from "../fleet/types";
import type { Theme } from "../ui/useTheme";

type ReviewTab = "critic" | "evaluator";

const EVALUATOR_SCROLLBACK = 20000;
const CRITIC_SCROLLBACK = 20000;

interface ReviewViewProps {
  started: boolean;
  criticStarted: boolean;
  onCriticStart: () => void;
  evaluatorAwake: boolean;
  statuses: Record<PaneId, PaneStatus>;
  fontSize: number;
  theme: Theme;
  onStatus: (pane: PaneId, status: PaneStatus) => void;
  interview: { open: boolean | null; toggle: () => void; error: string | null };
}

export function ReviewView({
  started,
  criticStarted,
  onCriticStart,
  evaluatorAwake,
  statuses,
  fontSize,
  theme,
  onStatus,
  interview,
}: ReviewViewProps) {
  const [tab, setTab] = useState<ReviewTab>("critic");
  return (
    <div className="review-view">
      <div className="feed-view__tabs" role="tablist">
        <button
          role="tab"
          aria-selected={tab === "critic"}
          className={`feed-view__tab ${tab === "critic" ? "feed-view__tab--active" : ""}`}
          onClick={() => setTab("critic")}
        >
          Critic
        </button>
        <button
          role="tab"
          aria-selected={tab === "evaluator"}
          className={`feed-view__tab ${tab === "evaluator" ? "feed-view__tab--active" : ""}`}
          onClick={() => setTab("evaluator")}
        >
          Evaluator
        </button>
      </div>

      <div className={tab === "critic" ? "" : "is-hidden"}>
        <div className="critic-view">
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
              onClick={onCriticStart}
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
              fontSize={fontSize}
              theme={theme}
              onStatus={onStatus}
            />
          </div>
        </div>
      </div>

      <div className={tab === "evaluator" ? "" : "is-hidden"}>
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
              fontSize={fontSize}
              theme={theme}
              onStatus={onStatus}
            />
          </div>
        </div>
      </div>
    </div>
  );
}
