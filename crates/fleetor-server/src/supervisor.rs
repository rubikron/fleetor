//! The Phase 1 supervisor loop for one ticket, one worker.
//!
//! Lifecycle (handoff §5):
//!  1. spawn the agent, wait for its `init` event (→ `session_id`, worker Idle)
//!  2. `assign`: write the ticket as a user message to stdin (worker Working)
//!  3. read events until `result` (turn end), teeing tool calls to the event log
//!  4. ingest the report from the final message; if none, reprompt once
//!     ("you ended without a report") before failing
//!  5. resolve the ticket state from the report status and persist everything
//!
//! A per-ticket wall-clock budget guards against a headless worker wedging on a
//! permission prompt (handoff §5, BUILDING §8): a stretch of silence past the
//! deadline → kill + escalate.

use anyhow::{Context, Result};
use fleetor_cc::event::ContentBlock;
use fleetor_cc::{AgentProcess, Event, Recv, Session, SessionMsg};
use fleetor_core::event::{FleetEvent, NoticeLevel, TicketState, WorkerState};
use fleetor_core::report::{Report, ReportStatus};
use fleetor_core::{Store, Ticket};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Knobs for one supervised ticket run.
pub struct SuperviseOptions {
    /// Logical worker slot (1..=4) — labels events and the transcript path.
    pub slot: u8,
    /// Where to tee the raw transcript (`logs/worker-N/T-XXX.jsonl`). `None`
    /// disables on-disk capture (tests that don't care).
    pub raw_log: Option<PathBuf>,
    /// Max user-message turns before giving up (1 assignment + reprompts).
    /// Phase 1 uses the assignment plus a single no-report reprompt.
    pub max_turns: u32,
    /// If no event arrives within this window, treat the worker as wedged.
    pub idle_timeout: Duration,
}

impl SuperviseOptions {
    pub fn new(slot: u8, wall_secs: u64) -> Self {
        Self {
            slot,
            raw_log: None,
            max_turns: 2,
            idle_timeout: Duration::from_secs(wall_secs.max(1)),
        }
    }
}

/// How a supervised ticket ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// A report was filed; the ticket moved to the matching board state.
    Reported { status: ReportStatus },
    /// Turn(s) ended with no parseable report, even after the reprompt.
    NoReport,
    /// The report block was present but its JSON was invalid.
    BadReport { detail: String },
    /// Wall-clock budget exceeded — killed and escalated.
    TimedOut,
    /// The process closed its stream without ever emitting a `result`.
    Crashed,
}

/// Run one ticket end to end against `agent`, persisting to `store`.
pub fn run_ticket(
    agent: &dyn AgentProcess,
    ticket: &Ticket,
    store: &dyn Store,
    opts: &SuperviseOptions,
) -> Result<Outcome> {
    store.upsert_ticket(ticket)?;
    // Track the ticket's live board state so event `from` fields are accurate
    // across hops (the in-memory `ticket` is an immutable snapshot).
    let mut cur = ticket.state;
    let mut session = Session::spawn(agent, opts.raw_log.clone())
        .with_context(|| format!("spawning worker for {}", ticket.id))?;

    let deadline = Instant::now() + opts.idle_timeout;

    // (1) Assign immediately. Real `claude` in stream-json input mode emits its
    // `init` event only once it begins processing the first stdin message, so
    // gating the first write on `init` deadlocks (verified against real CC —
    // DECISIONS D-010). We write first and read the stream after; the `init`
    // event is consumed like any other in the turn loop.
    session.send_user(&ticket.assignment_message()).context("assigning ticket")?;
    cur = transition_ticket(store, &ticket.id, cur, TicketState::Assigned)?;
    cur = transition_ticket(store, &ticket.id, cur, TicketState::InProgress)?;
    emit(store, worker_change(opts.slot, WorkerState::Booting, WorkerState::Working));

    // (2)+(3) Turn loop with a single no-report reprompt.
    let mut turns_used = 1u32;
    loop {
        let text = match read_until_result(&mut session, ticket, opts, store, deadline)? {
            TurnEnd::Result { assistant_text } => assistant_text,
            TurnEnd::Timeout => {
                notice(store, NoticeLevel::Error, format!("{}: worker wedged; killing", ticket.id));
                return finish(store, &ticket.id, opts, &mut session, cur, Outcome::TimedOut, TicketState::Failed);
            }
            TurnEnd::Closed => {
                notice(store, NoticeLevel::Error, format!("{}: stream closed with no result", ticket.id));
                return finish(store, &ticket.id, opts, &mut session, cur, Outcome::Crashed, TicketState::Failed);
            }
        };

        match Report::from_transcript_text(&text) {
            Some(Ok(report)) => {
                store.save_report(&ticket.id, opts.slot, &report)?;
                emit(store, FleetEvent::ReportFiled {
                    ticket: ticket.id.clone(),
                    slot: opts.slot,
                    status: report.status,
                });
                let final_state = state_for(report.status);
                return finish(
                    store,
                    &ticket.id,
                    opts,
                    &mut session,
                    cur,
                    Outcome::Reported { status: report.status },
                    final_state,
                );
            }
            Some(Err(e)) => {
                // A block was present but malformed — don't loop on it.
                notice(store, NoticeLevel::Error, format!("{}: malformed report: {e}", ticket.id));
                return finish(
                    store,
                    &ticket.id,
                    opts,
                    &mut session,
                    cur,
                    Outcome::BadReport { detail: e.to_string() },
                    TicketState::Failed,
                );
            }
            None => {
                if turns_used >= opts.max_turns {
                    notice(store, NoticeLevel::Warn, format!("{}: no report after reprompt", ticket.id));
                    return finish(store, &ticket.id, opts, &mut session, cur, Outcome::NoReport, TicketState::Failed);
                }
                notice(store, NoticeLevel::Info, format!("{}: no report; reprompting", ticket.id));
                session.send_user(Ticket::NO_REPORT_REPROMPT).context("reprompting")?;
                turns_used += 1;
            }
        }
    }
}

enum TurnEnd {
    Result { assistant_text: String },
    Timeout,
    Closed,
}

/// Read the current turn's events until a `result`, collecting assistant text
/// and emitting a `ToolActivity` event per tool call. Honors the wall-clock
/// deadline across the whole run.
fn read_until_result(
    session: &mut Session,
    ticket: &Ticket,
    opts: &SuperviseOptions,
    store: &dyn Store,
    deadline: Instant,
) -> Result<TurnEnd> {
    let mut assistant_text = String::new();
    loop {
        let remaining = deadline.checked_duration_since(Instant::now());
        let Some(remaining) = remaining else {
            return Ok(TurnEnd::Timeout);
        };
        match session.recv(remaining.min(Duration::from_millis(500))) {
            Recv::Got(SessionMsg::Event(ev)) => match ev {
                Event::Assistant(m) => {
                    for block in &m.message.content {
                        match block {
                            ContentBlock::Text(t) => {
                                assistant_text.push_str(t);
                                assistant_text.push('\n');
                            }
                            ContentBlock::ToolUse(tu) => emit(store, FleetEvent::ToolActivity {
                                slot: opts.slot,
                                ticket: ticket.id.clone(),
                                tool: tu.name.clone(),
                            }),
                            _ => {}
                        }
                    }
                }
                Event::Result(_) => return Ok(TurnEnd::Result { assistant_text }),
                // Init (already consumed) / User tool-results / Other: ignore.
                _ => {}
            },
            Recv::Got(SessionMsg::BadLine(_)) => {} // logged to disk already
            Recv::Timeout => {
                if Instant::now() >= deadline {
                    return Ok(TurnEnd::Timeout);
                }
                // else: a short poll tick; keep waiting.
            }
            Recv::Closed => return Ok(TurnEnd::Closed),
        }
    }
}

fn state_for(status: ReportStatus) -> TicketState {
    match status {
        ReportStatus::Done => TicketState::Done,
        ReportStatus::Blocked => TicketState::Blocked,
        ReportStatus::NeedsDecision => TicketState::Blocked,
        ReportStatus::Failed => TicketState::Failed,
    }
}

/// Common terminal path: mark the worker dead, move the ticket, reap, return.
#[allow(clippy::too_many_arguments)]
fn finish(
    store: &dyn Store,
    ticket_id: &str,
    opts: &SuperviseOptions,
    session: &mut Session,
    from: TicketState,
    outcome: Outcome,
    final_state: TicketState,
) -> Result<Outcome> {
    transition_ticket(store, ticket_id, from, final_state)?;
    emit(store, worker_change(opts.slot, WorkerState::Working, WorkerState::Dead));
    session.close_stdin();
    session.kill();
    Ok(outcome)
}

/// Persist a board transition and log it; returns the new state so callers can
/// keep their `from` cursor accurate.
fn transition_ticket(
    store: &dyn Store,
    ticket_id: &str,
    from: TicketState,
    to: TicketState,
) -> Result<TicketState> {
    store.set_ticket_state(ticket_id, to)?;
    emit(store, FleetEvent::TicketState { ticket: ticket_id.to_string(), from, to });
    Ok(to)
}

fn worker_change(slot: u8, from: WorkerState, to: WorkerState) -> FleetEvent {
    FleetEvent::WorkerState { slot, from, to }
}

fn notice(store: &dyn Store, level: NoticeLevel, text: String) {
    emit(store, FleetEvent::Notice { level, text });
}

/// Append an event, swallowing (but flagging) a store error so supervision
/// never dies on a logging failure.
fn emit(store: &dyn Store, event: FleetEvent) {
    if let Err(e) = store.append_event(&event) {
        eprintln!("warn: failed to persist event {}: {e}", event.kind());
    }
}
