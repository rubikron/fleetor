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
use fleetor_core::{Party, Store, Ticket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A handle the dynamic runner keeps for each live worker so it can interrupt an
/// in-flight turn from the async side (Phase 4f). [`run_ticket`] publishes the
/// worker's pid the moment its process spawns; [`WorkerControl::kill`] signals
/// that pid, which closes the worker's stream and unwinds this sync loop cleanly
/// (the runner then relabels the outcome as [`Outcome::Interrupted`]).
#[derive(Default)]
pub struct WorkerControl {
    /// The worker's process id, or 0 before its process has spawned.
    pid: AtomicU32,
    /// Set when the runner deliberately killed this worker (interrupt/restart), so
    /// the resulting stream-close is relabelled [`Outcome::Interrupted`], not
    /// [`Outcome::Crashed`].
    interrupted: AtomicBool,
}

impl WorkerControl {
    fn publish_pid(&self, pid: u32) {
        self.pid.store(pid, Ordering::SeqCst);
    }

    /// Signal the worker's process to die and mark the kill as deliberate. A no-op
    /// before the process spawns; a stale pid is a harmless `ESRCH`.
    pub fn kill(&self) {
        self.interrupted.store(true, Ordering::SeqCst);
        let pid = self.pid.load(Ordering::SeqCst);
        if pid != 0 {
            // SAFETY: a plain `kill(2)` syscall with an owned pid; the worst case
            // for a reused/stale pid is `ESRCH`, which we ignore.
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGKILL);
            }
        }
    }

    /// Whether [`kill`](Self::kill) was called on this worker.
    pub fn was_interrupted(&self) -> bool {
        self.interrupted.load(Ordering::SeqCst)
    }
}

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
    /// An optional interrupt handle (Phase 4f). When set, `run_ticket` publishes
    /// the worker's pid here so the runner can kill an in-flight turn. `None` for
    /// the static supervisor / quality paths, which nobody interrupts externally.
    pub control: Option<Arc<WorkerControl>>,
}

impl SuperviseOptions {
    pub fn new(slot: u8, wall_secs: u64) -> Self {
        Self {
            slot,
            raw_log: None,
            max_turns: 2,
            idle_timeout: Duration::from_secs(wall_secs.max(1)),
            control: None,
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
    /// The lead yanked this worker's turn (Phase 4f `interrupt`/`worker_restart`):
    /// the runner killed the process deliberately, so its stream-close is *not* a
    /// crash. The runner stamps this in place of `Crashed` for a kill it initiated.
    Interrupted,
}

/// Run one ticket end to end against `agent`, persisting to `store`. Phase 1:
/// the first filed report is terminal (no gate, no review — that is Phase 3's
/// [`crate::quality`] loop, which reuses the [`assign`] / [`drive_to_report`]
/// primitives below to keep the worker session alive across bounces).
pub fn run_ticket(
    agent: &dyn AgentProcess,
    ticket: &Ticket,
    store: &dyn Store,
    opts: &SuperviseOptions,
) -> Result<Outcome> {
    store.upsert_ticket(ticket)?;
    let mut session = Session::spawn(agent, opts.raw_log.clone())
        .map_err(|e| {
            emit(store, FleetEvent::WorkerExited {
                slot: opts.slot,
                ticket: ticket.id.clone(),
                ok: false,
                detail: format!("failed to spawn worker: {e:#}"),
            });
            e
        })
        .with_context(|| format!("spawning worker for {}", ticket.id))?;
    // Publish the pid so the runner can interrupt this turn from the async side.
    if let Some(control) = &opts.control {
        control.publish_pid(session.pid());
    }
    let deadline = Instant::now() + opts.idle_timeout;

    // A worker that dies on startup fails here (its first stdin write hits a broken
    // pipe). Surface its stderr — otherwise the ticket just hangs at `Assigned` with
    // no visible reason (the crash that was previously invisible).
    let cur = assign(&mut session, ticket, store, opts).map_err(|e| {
        let tail = session.stderr_tail();
        let detail = if tail.is_empty() {
            format!("worker failed before starting: {e:#}")
        } else {
            format!("worker failed before starting: {e:#}\n--- stderr ---\n{tail}")
        };
        emit(store, FleetEvent::WorkerExited {
            slot: opts.slot,
            ticket: ticket.id.clone(),
            ok: false,
            detail,
        });
        e
    })?;

    // The base supervisor prefers report-over-MCP (4b): the worker files via the
    // `fleet.report` tool, the hub persists + emits `ReportFiled`, and we read
    // that as the terminal signal. The transcript-scrape stays the backstop.
    match drive_to_report(&mut session, ticket, opts, store, deadline, true)? {
        ReportStep::Report(report) => {
            store.save_report(&ticket.id, opts.slot, &report)?;
            emit(store, FleetEvent::ReportFiled {
                ticket: ticket.id.clone(),
                slot: opts.slot,
                status: report.status,
            });
            let final_state = state_for(report.status);
            finish(store, &ticket.id, opts, &mut session, cur, Outcome::Reported { status: report.status }, final_state)
        }
        ReportStep::Terminal { outcome, final_state } => {
            finish(store, &ticket.id, opts, &mut session, cur, outcome, final_state)
        }
    }
}

/// A **standby** pool worker (Phase 4i): spawn the session once and keep it alive
/// and idle, delivering any queued mail to its stdin (the D-015 path) and driving
/// the resulting turn — transcript events flow through [`read_until_result`]
/// (D-028). Unlike [`run_ticket`] it never reports-and-dies; it lives until
/// interrupted (app shutdown / `worker_restart`). An idle standby worker spends no
/// tokens. Assignment *is* messaging here: the orch sends the worker a task and it
/// acts on the delivered turn.
pub fn run_standby_worker(
    agent: &dyn AgentProcess,
    slot: u8,
    store: &dyn Store,
    control: Arc<WorkerControl>,
    raw_log: Option<PathBuf>,
) -> Result<Outcome> {
    const POLL: Duration = Duration::from_millis(200);
    const TURN_BUDGET: Duration = Duration::from_secs(300);

    let ticket = Ticket::new(format!("worker-{slot}"), "standby", "Standby fleet worker.");
    let opts = SuperviseOptions {
        slot,
        raw_log: raw_log.clone(),
        max_turns: 1,
        idle_timeout: TURN_BUDGET,
        control: Some(control.clone()),
    };

    let mut session = Session::spawn(agent, raw_log)
        .map_err(|e| {
            emit(store, FleetEvent::WorkerExited {
                slot,
                ticket: ticket.id.clone(),
                ok: false,
                detail: format!("failed to spawn standby worker: {e:#}"),
            });
            e
        })
        .with_context(|| format!("spawning standby worker-{slot}"))?;
    control.publish_pid(session.pid());
    emit(store, worker_change(slot, WorkerState::Booting, WorkerState::Idle));

    let to = Party::Worker(slot);
    let mut crashed = false;
    while !control.was_interrupted() {
        let mail = store.take_mail(&to).unwrap_or_default();
        if mail.is_empty() {
            std::thread::sleep(POLL);
            continue;
        }
        // Deliver the message as a fresh turn (D-015 idle→stdin), then drive it —
        // `read_until_result` streams the worker's words + tools to the transcript.
        emit(store, worker_change(slot, WorkerState::Idle, WorkerState::Working));
        if session.send_user(&fleetor_core::frame_mail_for_injection(&mail)).is_err() {
            crashed = true;
            break;
        }
        let deadline = Instant::now() + TURN_BUDGET;
        match read_until_result(&mut session, &ticket, &opts, store, deadline)? {
            TurnEnd::Result { .. } => {}
            TurnEnd::Timeout => notice(
                store,
                NoticeLevel::Warn,
                format!("worker-{slot}: turn timed out; back to standby"),
            ),
            TurnEnd::Closed => {
                crashed = true;
                break;
            }
        }
        emit(store, worker_change(slot, WorkerState::Working, WorkerState::Idle));
    }

    // A stream-close we caused by killing the worker at shutdown is not a crash.
    let genuine_crash = crashed && !control.was_interrupted();
    let outcome = if genuine_crash { Outcome::Crashed } else { Outcome::Interrupted };
    emit(store, FleetEvent::WorkerExited {
        slot,
        ticket: ticket.id.clone(),
        ok: !genuine_crash,
        detail: if genuine_crash { session.stderr_tail() } else { String::new() },
    });
    emit(store, worker_change(slot, WorkerState::Working, WorkerState::Dead));
    session.close_stdin();
    session.kill();
    Ok(outcome)
}

/// Assign the ticket: write it to stdin and move the board to `InProgress`.
///
/// Real `claude` in stream-json input mode emits its `init` event only once it
/// begins processing the first stdin message, so gating the first write on
/// `init` deadlocks (verified against real CC — DECISIONS D-010). We write first
/// and read the stream after; `init` is consumed like any other turn event.
/// Returns the live board state so callers keep their `from` cursor accurate.
pub(crate) fn assign(
    session: &mut Session,
    ticket: &Ticket,
    store: &dyn Store,
    opts: &SuperviseOptions,
) -> Result<TicketState> {
    session.send_user(&ticket.assignment_message()).context("assigning ticket")?;
    let cur = transition_ticket(store, &ticket.id, ticket.state, TicketState::Assigned)?;
    let cur = transition_ticket(store, &ticket.id, cur, TicketState::InProgress)?;
    emit(store, worker_change(opts.slot, WorkerState::Booting, WorkerState::Working));
    Ok(cur)
}

/// One reported turn: read events until `result`, ingest the report, reprompting
/// once (up to `opts.max_turns`) when none is filed. Does **not** kill the
/// session — the caller (Phase 1 or the quality loop) decides what happens next.
/// On a wedge / crash / malformed / no-report the notice is emitted here and a
/// [`ReportStep::Terminal`] is returned.
///
/// When `prefer_mcp` is set (the base supervisor — 4b/D-018), the worker's
/// report-over-MCP is **primary**: the hub persists it and emits `ReportFiled`,
/// which we read as the terminal signal and finish on *without re-saving or
/// re-emitting* (a plain `ReportStep::Terminal { Reported }`). The transcript
/// scrape stays the backstop for a worker that ended its turn with a
/// `fleet-report` block instead. `prefer_mcp = false` keeps the caller on pure
/// transcript-scrape (the quality loop needs the full [`Report`] body — D-016).
pub(crate) fn drive_to_report(
    session: &mut Session,
    ticket: &Ticket,
    opts: &SuperviseOptions,
    store: &dyn Store,
    deadline: Instant,
    prefer_mcp: bool,
) -> Result<ReportStep> {
    // Watch for a `ReportFiled` the hub appends *after* this point; `i64::MAX`
    // disables the watch (`events_since(MAX)` is always empty).
    let mut report_cursor = if prefer_mcp { store.latest_seq()? } else { i64::MAX };

    let mut turns_used = 1u32;
    loop {
        let text = match read_until_result(session, ticket, opts, store, deadline)? {
            TurnEnd::Result { assistant_text } => assistant_text,
            TurnEnd::Timeout => {
                notice(store, NoticeLevel::Error, format!("{}: worker wedged; killing", ticket.id));
                return Ok(ReportStep::terminal(Outcome::TimedOut, TicketState::Failed));
            }
            TurnEnd::Closed => {
                notice(store, NoticeLevel::Error, format!("{}: stream closed with no result", ticket.id));
                return Ok(ReportStep::terminal(Outcome::Crashed, TicketState::Failed));
            }
        };

        // Primary: the worker filed over MCP; the hub owns the save + emit.
        if let Some(step) = mcp_report_step(store, &ticket.id, opts.slot, &mut report_cursor) {
            return Ok(step);
        }

        // Backstop (D-008): a `fleet-report` block scraped from the transcript.
        match Report::from_transcript_text(&text) {
            Some(Ok(report)) => return Ok(ReportStep::Report(report)),
            Some(Err(e)) => {
                notice(store, NoticeLevel::Error, format!("{}: malformed report: {e}", ticket.id));
                return Ok(ReportStep::terminal(Outcome::BadReport { detail: e.to_string() }, TicketState::Failed));
            }
            None => {
                // A late MCP report may still be committing on the hub's task —
                // grace-poll briefly before treating the turn as report-less.
                if prefer_mcp {
                    if let Some(step) = grace_poll_mcp_report(store, &ticket.id, opts.slot, &mut report_cursor, deadline) {
                        return Ok(step);
                    }
                }
                // D-015 idle→stdin: mail that lands while the worker sits idle
                // between turns (the Stop hook already drained empty) is delivered
                // here — the supervisor writes it straight to stdin as a fresh
                // turn. A mail-driven turn is legitimate work, so it does NOT count
                // against the no-report reprompt cap; it's bounded by `deadline`.
                if idle_drain_step(session, store, opts, deadline)? {
                    continue;
                }
                if turns_used >= opts.max_turns {
                    notice(store, NoticeLevel::Warn, format!("{}: no report after reprompt", ticket.id));
                    return Ok(ReportStep::terminal(Outcome::NoReport, TicketState::Failed));
                }
                notice(store, NoticeLevel::Info, format!("{}: no report; reprompting", ticket.id));
                session.send_user(Ticket::NO_REPORT_REPROMPT).context("reprompting")?;
                turns_used += 1;
            }
        }
    }
}

/// If the hub appended a `ReportFiled` for (`ticket`, `slot`) after `cursor`,
/// return the terminal step for it. The hub already saved the report and emitted
/// the event, so the supervisor finishes on it *without* re-saving or re-emitting
/// (that double-log was the 4a symptom D-018 fixes). Advances `cursor` past what
/// it scanned, so repeat polls don't re-report the same event.
fn mcp_report_step(store: &dyn Store, ticket_id: &str, slot: u8, cursor: &mut i64) -> Option<ReportStep> {
    let events = store.events_since(*cursor).ok()?;
    for (seq, ev) in events {
        *cursor = seq;
        if let FleetEvent::ReportFiled { ticket, slot: s, status } = ev {
            if ticket == ticket_id && s == slot {
                return Some(ReportStep::terminal(Outcome::Reported { status }, state_for(status)));
            }
        }
    }
    None
}

/// Poll [`mcp_report_step`] a few times, sleeping between, to catch a report the
/// hub is still committing right at the turn boundary (its task runs on another
/// thread). Bounded by `GRACE_TICKS` and the wall-clock `deadline`.
fn grace_poll_mcp_report(
    store: &dyn Store,
    ticket_id: &str,
    slot: u8,
    cursor: &mut i64,
    deadline: Instant,
) -> Option<ReportStep> {
    const GRACE_TICKS: u32 = 10;
    const TICK: Duration = Duration::from_millis(50);
    for _ in 0..GRACE_TICKS {
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(TICK);
        if let Some(step) = mcp_report_step(store, ticket_id, slot, cursor) {
            return Some(step);
        }
    }
    None
}

/// D-015 idle→stdin: at a report-less turn boundary, briefly watch this worker's
/// mailbox; if mail is queued (or lands within the window), write it to stdin as a
/// fresh turn (framed like every other injection path — [`frame_mail_for_injection`])
/// and return `true`. The bounded poll mirrors [`grace_poll_mcp_report`] so steering
/// mail arriving just after the turn ends is still caught. `false` means the window
/// elapsed with an empty queue — the caller falls through to its reprompt/finish
/// path unchanged. The worker is flipped `Working → Idle → Working` around the wait
/// so the UI can render the honest idle beat.
///
/// The supervisor drains the shared [`Store`] directly (the same seam it reads for
/// `ReportFiled`, D-019) — no cross-runtime channel. `take_mail` is atomic, so this
/// never races the worker's own Stop-hook drain: whichever pulls a message first
/// owns it, and by the time a `result` reaches the supervisor the Stop hook has
/// already drained empty.
///
/// [`frame_mail_for_injection`]: fleetor_core::frame_mail_for_injection
fn idle_drain_step(
    session: &mut Session,
    store: &dyn Store,
    opts: &SuperviseOptions,
    deadline: Instant,
) -> Result<bool> {
    const IDLE_TICKS: u32 = 10;
    const TICK: Duration = Duration::from_millis(50);
    let to = Party::Worker(opts.slot);

    emit(store, worker_change(opts.slot, WorkerState::Working, WorkerState::Idle));
    let mut delivered = false;
    for tick in 0..IDLE_TICKS {
        let mail = store.take_mail(&to).unwrap_or_default();
        if !mail.is_empty() {
            session
                .send_user(&fleetor_core::frame_mail_for_injection(&mail))
                .context("injecting idle mail to stdin")?;
            delivered = true;
            break;
        }
        // Stop polling once the wall-clock budget is spent; don't sleep past it.
        if tick + 1 < IDLE_TICKS && Instant::now() < deadline {
            std::thread::sleep(TICK);
        } else {
            break;
        }
    }
    emit(store, worker_change(opts.slot, WorkerState::Idle, WorkerState::Working));
    Ok(delivered)
}

/// The result of one [`drive_to_report`]: a filed report, or a terminal outcome
/// the caller should finish on (with the board state to land on).
pub(crate) enum ReportStep {
    Report(Report),
    Terminal { outcome: Outcome, final_state: TicketState },
}

impl ReportStep {
    fn terminal(outcome: Outcome, final_state: TicketState) -> Self {
        ReportStep::Terminal { outcome, final_state }
    }
}

enum TurnEnd {
    Result { assistant_text: String },
    Timeout,
    Closed,
}

/// Read the current turn's events until a `result`, collecting assistant text
/// Read exactly one turn and return its assistant text; `None` on a wedge or a
/// closed stream. Used by the quality loop's reviewer, whose turn ends in a
/// `fleet-review` block rather than a `fleet-report` (so the no-report reprompt
/// of [`drive_to_report`] doesn't apply).
pub(crate) fn read_turn_text(
    session: &mut Session,
    ticket: &Ticket,
    opts: &SuperviseOptions,
    store: &dyn Store,
    deadline: Instant,
) -> Result<Option<String>> {
    match read_until_result(session, ticket, opts, store, deadline)? {
        TurnEnd::Result { assistant_text } => Ok(Some(assistant_text)),
        TurnEnd::Timeout | TurnEnd::Closed => Ok(None),
    }
}

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
                                // Surface the worker's words to the transcript view.
                                emit(store, FleetEvent::WorkerSaid {
                                    slot: opts.slot,
                                    ticket: ticket.id.clone(),
                                    text: t.clone(),
                                });
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

pub(crate) fn state_for(status: ReportStatus) -> TicketState {
    match status {
        ReportStatus::Done => TicketState::Done,
        ReportStatus::Blocked => TicketState::Blocked,
        ReportStatus::NeedsDecision => TicketState::Blocked,
        ReportStatus::Failed => TicketState::Failed,
    }
}

/// Common terminal path: mark the worker dead, move the ticket, reap, return.
#[allow(clippy::too_many_arguments)]
pub(crate) fn finish(
    store: &dyn Store,
    ticket_id: &str,
    opts: &SuperviseOptions,
    session: &mut Session,
    from: TicketState,
    outcome: Outcome,
    final_state: TicketState,
) -> Result<Outcome> {
    transition_ticket(store, ticket_id, from, final_state)?;
    // Announce the worker's exit — with the captured stderr tail on a non-success,
    // so a crash that produced no stdout is finally diagnosable in the shell.
    let ok = matches!(outcome, Outcome::Reported { .. });
    emit(store, FleetEvent::WorkerExited {
        slot: opts.slot,
        ticket: ticket_id.to_string(),
        ok,
        detail: if ok { String::new() } else { session.stderr_tail() },
    });
    emit(store, worker_change(opts.slot, WorkerState::Working, WorkerState::Dead));
    session.close_stdin();
    session.kill();
    Ok(outcome)
}

/// Persist a board transition and log it; returns the new state so callers can
/// keep their `from` cursor accurate.
pub(crate) fn transition_ticket(
    store: &dyn Store,
    ticket_id: &str,
    from: TicketState,
    to: TicketState,
) -> Result<TicketState> {
    store.set_ticket_state(ticket_id, to)?;
    emit(store, FleetEvent::TicketState { ticket: ticket_id.to_string(), from, to });
    Ok(to)
}

pub(crate) fn worker_change(slot: u8, from: WorkerState, to: WorkerState) -> FleetEvent {
    FleetEvent::WorkerState { slot, from, to }
}

pub(crate) fn notice(store: &dyn Store, level: NoticeLevel, text: String) {
    emit(store, FleetEvent::Notice { level, text });
}

/// Append an event, swallowing (but flagging) a store error so supervision
/// never dies on a logging failure.
pub(crate) fn emit(store: &dyn Store, event: FleetEvent) {
    if let Err(e) = store.append_event(&event) {
        eprintln!("warn: failed to persist event {}: {e}", event.kind());
    }
}
