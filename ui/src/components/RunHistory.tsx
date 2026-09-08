// History — your past sessions, and going back into one (WP-11, D-058; WP-27).
//
// **This file used to refuse the feature it now implements, and the refusal was
// right about the thing it was refusing.** It said there was no resume, "not an
// omission to fill in later", because the panes that made a past run are gone and
// anything shaped like continuing one would be inventing a fleet that never
// existed — D-030's regrowth warning. What that forbids is *FLEETOR*
// reconstructing a pane's context: parsing back redrawn ANSI, rebuilding a
// transcript the fleet never owned. That is still forbidden and still not done.
//
// What happens here instead is that the **vendor** reopens a session it wrote
// itself, addressed by an id it minted (checkpoint 15). FLEETOR invents nothing
// and parses no transcript. Measured before it was built: a plain reopen appends
// to the same file under the same session id on both registered harnesses, so
// there is not even a fork — `docs/notes/reopen-spike-notes.md`.
//
// **So History is a switcher, not a reader.** Clicking a row archives the fleet
// you have and brings that run's five panes back; you land in the ordinary pane
// view, and Messages, Tasks and Activity show that run because its log *is* the
// live log now (R1). That is why this file has no feed components in it any more:
// there is no second, archived way to render a run.
//
// The three actions are graded by how hard they are to undo. Opening is the row
// itself — reversible by opening the one you left, which is a row here the moment
// you leave it. Rename is inline and instant. Delete asks first, and now says how
// much it takes: a row is a lineage, so a run reopened twice is three archives
// behind one label (R15).

import { useState } from "react";
import type { RunsView } from "../fleet/useRuns";
import type { RunRecord } from "../fleet/types";

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

/// One past session: what it was, and whether you can go back into it.
function RunRow({
  run,
  opening,
  busy,
  onOpen,
  onRename,
  onDelete,
  onSave,
}: {
  run: RunRecord;
  opening: boolean;
  busy: boolean;
  onOpen: () => void;
  onRename: (label: string) => void;
  onDelete: () => void;
  onSave: () => void;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const span = duration(run);
  const blocked = run.cannot_reopen;

  const commit = () => {
    const next = (draft ?? "").trim();
    if (next && next !== run.label) onRename(next);
    setDraft(null);
  };

  return (
    <li className={`run ${blocked ? "run--closed" : ""}`}>
      <div className="run__main">
        {draft === null ? (
          blocked ? (
            // Not a button. A row that cannot be opened offers nothing to click,
            // rather than a control that explains itself only after it fails.
            <span className="run__label run__label--closed">{run.label}</span>
          ) : (
            <button
              className="run__label"
              onClick={onOpen}
              disabled={busy}
              title="Open this session — the fleet you have now is archived first"
            >
              {run.label}
            </button>
          )
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
          {/* Gold, because it is the thing that explains the totals beside it:
              a reopened run's log opens as a copy of its parent's (R1), so those
              counts span the lineage. Saying so is cheaper than a footnote. */}
          {run.reopened > 0 && (
            <span
              className="run__chip run__chip--gold"
              title="counts above span every sitting of this session"
            >
              reopened {run.reopened}&times;
            </span>
          )}
          {run.transcripts > 0 && (
            <span className="run__chip" title="pane sessions archived with this run">
              {run.transcripts} transcript{run.transcripts === 1 ? "" : "s"}
            </span>
          )}
          <span className="run__chip run__chip--quiet">{size(run.bytes)}</span>
        </div>
        {run.target && <div className="run__target">{run.target}</div>}
        {opening && <div className="run__why">Opening — archiving the current session first…</div>}
        {blocked && <div className="run__why">Can’t reopen — {blocked}</div>}
      </div>

      <div className="run__actions">
        {confirming ? (
          <>
            {/* A row is a lineage, so Delete can take more than one archive.
                Saying the number is the difference between an informed
                confirmation and a surprise (R15). */}
            <span className="run__warn">
              {run.reopened > 0
                ? `Delete all ${run.reopened + 1} sittings?`
                : "Delete for good?"}
            </span>
            <button className="run__act run__act--danger" onClick={onDelete}>
              Delete
            </button>
            <button className="run__act" onClick={() => setConfirming(false)}>
              Keep
            </button>
          </>
        ) : (
          <>
            <button className="run__act" onClick={onSave} title="Save this session's log as JSON">
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

export function RunHistory({ runs }: { runs: RunsView }) {
  const nothingOpens = runs.runs.length > 0 && runs.runs.every((r) => r.cannot_reopen);

  return (
    <div className="events-view">
      <div className="events-view__head">
        <h3>History</h3>
        <span className="label">your past sessions · newest first</span>
        <span style={{ flex: "1 1 auto" }} />
        <span className="label">
          {runs.runs.length} session{runs.runs.length === 1 ? "" : "s"}
        </span>
      </div>

      {runs.error && <div className="run-note run-note--warn">{runs.error}</div>}
      {/* Where the file went, kept on screen rather than flashed: an export the
          operator cannot locate afterwards did not really happen. */}
      {runs.saved && <div className="run-note run-note--ok">Exported to {runs.saved}</div>}

      {/* Said once, at the top, rather than repeated on every row. Every archive
          made before WP-27 is in this state, so on the first launch after it ships
          the whole list is in it — and a list where nothing opens needs one
          explanation, not N identical ones. */}
      {nothingOpens && (
        <div className="run-note">
          None of these can be reopened — they were archived before sessions were
          recorded. Their logs and transcripts are intact and still export. Sessions
          started from now on will reopen.
        </div>
      )}

      {runs.runs.length === 0 && !runs.error ? (
        <div className="run-note">
          No past sessions yet. The one you are in now appears here the next time the
          fleet starts — the boundary is cut at start, so a crash never loses one.
        </div>
      ) : (
        <ul className="run-list">
          {runs.runs.map((run) => (
            <RunRow
              key={run.id}
              run={run}
              opening={runs.opening === run.id}
              busy={runs.opening !== null}
              onOpen={() => void runs.reopen(run.id).catch(() => {})}
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
