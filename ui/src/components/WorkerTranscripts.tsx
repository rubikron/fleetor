// The Workers view: each headless worker's transcript — its words, tool calls,
// received mail, and (crucially) its exit/crash reason. Fed by the live event
// stream via useFleet; the store replays history so a freshly-opened tab is full.
// Reuses the Event log's row styling so it reads as one system.

import { useState } from "react";
import type { TranscriptLine, WorkerCell } from "../fleet/useFleet";

interface WorkerTranscriptsProps {
  transcripts: Record<number, TranscriptLine[]>;
  workers: WorkerCell[];
}

export function WorkerTranscripts({ transcripts, workers }: WorkerTranscriptsProps) {
  const [slot, setSlot] = useState<number>(workers[0]?.slot ?? 1);
  const lines = transcripts[slot] ?? [];

  return (
    <div className="events-view">
      <div className="events-view__head">
        <h3>Workers</h3>
        <span className="label">headless worker transcripts</span>
        <span className="grow" style={{ flex: "1 1 auto" }} />
        <div className="sidebar__group" role="tablist" aria-label="Worker slots" style={{ flexDirection: "row", gap: 4 }}>
          {workers.map((w) => {
            const count = transcripts[w.slot]?.length ?? 0;
            return (
              <button
                key={w.slot}
                role="tab"
                aria-selected={w.slot === slot}
                className={`nav ${w.slot === slot ? "nav--active" : ""}`}
                onClick={() => setSlot(w.slot)}
              >
                <span>worker-{w.slot}</span>
                {count > 0 && <span className="badge">{count}</span>}
              </button>
            );
          })}
        </div>
      </div>
      {lines.length === 0 ? (
        <div className="feed feed--empty">
          No transcript for worker-{slot} yet. Assign it a ticket — its words, tool calls, and any
          startup crash (with the stderr reason) will appear here.
        </div>
      ) : (
        <div className="feed">
          {lines.map((line) => (
            <div key={line.seq} className={`line line--${line.tone}`}>
              <span className="mono line__seq">{line.seq}</span>
              <span className={`mono line__kind line__kind--${line.tone}`}>{line.kind}</span>
              <span className="line__text">{line.text}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
