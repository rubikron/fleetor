// History — every past run, and one of them opened read-only (WP-11, D-058).
//
// The view has two faces and never both at once: the catalogue, and one run's
// contents behind a banner. Opening a run does not navigate anywhere, because a
// past run is not a place the app can be — it is a file being read.
//
// **Nothing here can act on the live fleet.** The three components below are the
// same ones the live views use, handed archived arrays; there is no composer, no
// send, and no "resume". That is not an omission to fill in later: the panes
// that made a past run are gone and their context died with them, so anything
// shaped like continuing one would be inventing a fleet that never existed
// (D-030's regrowth warning).
//
// The two destructive-ish actions are handled differently on purpose. Rename is
// inline and instant — it is reversible by renaming again. Delete asks first,
// because a run is the only thing in `~/.fleetor` that cannot be regenerated.

import { useState } from "react";
import { EventFeed } from "./EventFeed";
import { MessageFeed } from "./MessageFeed";
import { TaskBoard } from "./TaskBoard";
import type { OpenRun, RunsView } from "../fleet/useRuns";
import type { RunRecord } from "../fleet/types";

type Tab = "messages" | "tasks" | "activity";

const TABS: { tab: Tab; label: string }[] = [
  { tab: "messages", label: "Messages" },
  { tab: "tasks", label: "Tasks" },
  { tab: "activity", label: "Activity" },
];

/// Local time, because this is the one a human reads — the run's *id* is the UTC
/// spelling, and it stays on disk where a filesystem sorts it.
function when(ms?: number): string {
  if (!ms) return "no activity";
  return new Date(ms).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/// How long the run lasted. Absent for a run with nothing in its log — a single
/// event is a moment, not a duration, and rendering "0m" would imply otherwise.
function duration(run: RunRecord): string | null {
  if (!run.started_ms || !run.ended_ms) return null;
  const mins = Math.round((run.ended_ms - run.started_ms) / 60_000);
  if (mins < 1) return "under a minute";
  if (mins < 60) return `${mins}m`;
  return `${Math.floor(mins / 60)}h ${mins % 60}m`;
}

function size(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/// The run's own name for itself, and the target it worked on.
function RunRow({
  run,
  onOpen,
  onRename,
  onDelete,
  onSave,
}: {
  run: RunRecord;
  onOpen: () => void;
  onRename: (label: string) => void;
  onDelete: () => void;
  onSave: () => void;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const span = duration(run);

  const commit = () => {
    const next = (draft ?? "").trim();
    if (next && next !== run.label) onRename(next);
    setDraft(null);
  };

  return (
    <li className="run">
      <div className="run__main">
        {draft === null ? (
          <button className="run__label" onClick={onOpen} title="Open this run">
            {run.label}
          </button>
        ) : (
          <input
            className="run__rename"
            value={draft}
            autoFocus
            onChange={(e) => setDraft(e.target.value)}
            onBlur={commit}
            onKeyDown={(e) => {
              if (e.key === "Enter") commit();
              if (e.key === "Escape") setDraft(null);
            }}
          />
        )}
        <div className="run__meta">
          <span className="run__when">{when(run.started_ms)}</span>
          {span && <span className="run__chip">{span}</span>}
          <span className="run__chip">{run.messages} msg</span>
          <span className="run__chip">{run.tasks} task</span>
          {run.transcripts > 0 && (
            <span className="run__chip" title="worker session transcripts archived with this run">
              {run.transcripts} transcript{run.transcripts === 1 ? "" : "s"}
            </span>
          )}
          <span className="run__chip run__chip--quiet">{size(run.bytes)}</span>
        </div>
        {run.target && <div className="run__target">{run.target}</div>}
      </div>

      <div className="run__actions">
        {confirming ? (
          <>
            <span className="run__warn">Delete for good?</span>
            <button className="run__act run__act--danger" onClick={onDelete}>
              Delete
            </button>
            <button className="run__act" onClick={() => setConfirming(false)}>
              Keep
            </button>
          </>
        ) : (
          <>
            <button className="run__act" onClick={onSave} title="Save this run's log as JSON">
              Export
            </button>
            <button className="run__act" onClick={() => setDraft(run.label)}>
              Rename
            </button>
            <button className="run__act" onClick={() => setConfirming(true)}>
              Delete
            </button>
          </>
        )}
      </div>
    </li>
  );
}

/// One opened run. The banner is not decoration — every component below it looks
/// exactly like the live view of the same name, and this line is the only thing
/// saying otherwise.
function OpenedRun({
  open,
  onClose,
  onSave,
}: {
  open: OpenRun;
  onClose: () => void;
  onSave: () => void;
}) {
  const [tab, setTab] = useState<Tab>("messages");
  const { record } = open;

  return (
    <div className="run-open">
      <div className="run-open__banner">
        <button className="run-open__back" onClick={onClose}>
          ← History
        </button>
        <div className="run-open__title">
          <strong>{record.label}</strong>
          <span className="run-open__sub">
            {when(record.started_ms)} · {record.events} events · read-only
          </span>
        </div>
        <button className="run__act" onClick={onSave} title="Save this run's log as JSON">
          Export
        </button>
        <nav className="run-open__tabs" role="tablist">
          {TABS.map((t) => (
            <button
              key={t.tab}
              role="tab"
              aria-selected={tab === t.tab}
              className={`run-open__tab ${tab === t.tab ? "is-active" : ""}`}
              onClick={() => setTab(t.tab)}
            >
              {t.label}
            </button>
          ))}
        </nav>
      </div>

      {/* Mounted-and-hidden like every other view (building.md §7.5). */}
      <div className="run-open__stage">
        <div className={`stage-view ${tab === "messages" ? "" : "is-hidden"}`}>
          <MessageFeed messages={open.messages} commands={open.commands} />
        </div>
        <div className={`stage-view ${tab === "tasks" ? "" : "is-hidden"}`}>
          <TaskBoard tasks={open.tasks} />
        </div>
        <div className={`stage-view ${tab === "activity" ? "" : "is-hidden"}`}>
          <EventFeed feed={open.feed} />
        </div>
      </div>
    </div>
  );
}

export function RunHistory({ runs }: { runs: RunsView }) {
  if (runs.open) {
    return (
      <OpenedRun
        open={runs.open}
        onClose={runs.close}
        onSave={() => void runs.save(runs.open!.record.id)}
      />
    );
  }

  return (
    <div className="events-view">
      <div className="events-view__head">
        <h3>History</h3>
        <span className="label">
          every run before this one · newest first · read-only
        </span>
        <span style={{ flex: "1 1 auto" }} />
        <span className="label">
          {runs.runs.length} run{runs.runs.length === 1 ? "" : "s"}
        </span>
      </div>

      {runs.error && <div className="run-note run-note--warn">{runs.error}</div>}
      {runs.loading && <div className="run-note">Reading the run…</div>}
      {/* Where the file went, kept on screen rather than flashed: an export the
          operator cannot locate afterwards did not really happen. */}
      {runs.saved && <div className="run-note run-note--ok">Exported to {runs.saved}</div>}

      {runs.runs.length === 0 && !runs.error ? (
        <div className="run-note">
          No past runs yet. The run you are in now is archived here the next time the
          fleet starts — the boundary is cut at start, so a crash never loses one.
        </div>
      ) : (
        <ul className="run-list">
          {runs.runs.map((run) => (
            <RunRow
              key={run.id}
              run={run}
              onOpen={() => runs.openRun(run)}
              onRename={(label) => void runs.rename(run.id, label)}
              onDelete={() => void runs.remove(run.id)}
              onSave={() => void runs.save(run.id)}
            />
          ))}
        </ul>
      )}
    </div>
  );
}
