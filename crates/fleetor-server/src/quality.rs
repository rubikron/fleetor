//! The Phase 3 quality loop (BUILDING §6): the machinery that closes
//! implement → gate → fix → review → done **without the lead** (handoff §8).
//!
//! It reuses the Phase 1 supervisor primitives ([`assign`], [`drive_to_report`])
//! but, unlike [`run_ticket`], keeps the worker's session alive across bounces —
//! so gate feedback and review feedback land with the worker that still
//! remembers why it wrote the code (handoff §6, "session lifetime = ticket
//! lifetime"). One worker session; a *fresh* reviewer session per review round
//! (fresh eyes — handoff §8).
//!
//! Loop, once a `done` report is filed:
//!  1. run the exit gate in the worktree. Fail → bounce the failing checks back
//!     to the worker, capped at `gate_retry_cap`, then escalate to the lead.
//!  2. gate green → dispatch a peer review (a different agent, diff + AC). It
//!     approves, or requests changes → bounce to the worker, capped at
//!     `review_retry_cap`, then escalate.
//!  3. approved → the ticket is done (the human still opens the real PR — §10).
//!
//! A report whose status is not `done` (the worker raised a hand: blocked /
//! needs-decision / failed) short-circuits exactly as Phase 1 — no gate, no
//! review.

use crate::supervisor::{
    assign, drive_to_report, emit, finish, notice, state_for, transition_ticket, ReportStep,
    SuperviseOptions,
};
use crate::supervisor::Outcome;
use anyhow::{Context, Result};
use fleetor_cc::{AgentProcess, Session};
use fleetor_core::event::{FleetEvent, GateOutcome, NoticeLevel, ReviewOutcome, TicketState};
use fleetor_core::report::ReportStatus;
use fleetor_core::review::ReviewVerdict;
use fleetor_core::{GateRunner, Store, Ticket};
use std::path::PathBuf;
use std::time::Instant;

/// Tier-2 default retry cap for both gate and review bounces (DECISIONS: gate
/// retry cap 3; handoff §8 "capped at ~3 retries then escalate").
pub const DEFAULT_RETRY_CAP: u32 = 3;

/// Knobs for one quality-loop run.
pub struct QualityOptions {
    /// The worker-driving options (slot, wall-clock, transcript path).
    pub supervise: SuperviseOptions,
    /// The worktree the gate runs in and the reviewer reads (handoff §3).
    pub cwd: PathBuf,
    /// Max gate bounces before escalating to the lead.
    pub gate_retry_cap: u32,
    /// Max review-changes bounces before escalating to the lead.
    pub review_retry_cap: u32,
    /// The fresh reviewer agent, and the slot it runs as. `None` skips review
    /// (gate-only quality loop).
    pub reviewer: Option<Reviewer>,
}

/// A peer reviewer: a distinct agent in a distinct slot (handoff §8).
pub struct Reviewer {
    pub slot: u8,
    /// Where to tee the reviewer transcript; `None` disables capture.
    pub raw_log: Option<PathBuf>,
}

impl QualityOptions {
    pub fn new(supervise: SuperviseOptions, cwd: PathBuf) -> Self {
        Self {
            supervise,
            cwd,
            gate_retry_cap: DEFAULT_RETRY_CAP,
            review_retry_cap: DEFAULT_RETRY_CAP,
            reviewer: None,
        }
    }
}

/// How the quality loop ended.
#[derive(Debug, Clone, PartialEq)]
pub enum QualityOutcome {
    /// Gate green and (if enabled) review approved — ready for the human's PR.
    Passed { gate_bounces: u32, review_bounces: u32 },
    /// The gate never went green within the retry cap — escalated to the lead.
    GateEscalated,
    /// Review kept requesting changes past the cap — escalated to the lead.
    ReviewEscalated,
    /// The worker raised a hand (blocked / needs-decision / failed) — no gate.
    Reported { status: ReportStatus },
    /// A pre-report terminal outcome (no report, malformed, timeout, crash).
    Aborted { outcome: Outcome },
}

/// Run the quality loop for one ticket against `worker`, gating with `gate` and
/// (optionally) reviewing with a fresh `review_agent`.
pub fn run_quality_loop(
    worker: &dyn AgentProcess,
    review_agent: &dyn AgentProcess,
    ticket: &Ticket,
    store: &dyn Store,
    gate: &dyn GateRunner,
    opts: &QualityOptions,
) -> Result<QualityOutcome> {
    let sup = &opts.supervise;
    store.upsert_ticket(ticket)?;
    let mut session = Session::spawn(worker, sup.raw_log.clone())
        .with_context(|| format!("spawning worker for {}", ticket.id))?;
    let deadline = Instant::now() + sup.idle_timeout;

    let mut cur = assign(&mut session, ticket, store, sup)?;
    let mut gate_bounces = 0u32;
    let mut review_bounces = 0u32;

    loop {
        // --- get a report (may reprompt on a missing one) ---
        let report = match drive_to_report(&mut session, ticket, sup, store, deadline)? {
            ReportStep::Report(r) => r,
            ReportStep::Terminal { outcome, final_state } => {
                finish(store, &ticket.id, sup, &mut session, cur, outcome.clone(), final_state)?;
                return Ok(QualityOutcome::Aborted { outcome });
            }
        };
        emit(store, FleetEvent::ReportFiled {
            ticket: ticket.id.clone(),
            slot: sup.slot,
            status: report.status,
        });

        // The worker raised a hand — respect it, no gate (handoff §8/§9).
        if report.status != ReportStatus::Done {
            let final_state = state_for(report.status);
            store.save_report(&ticket.id, sup.slot, &report)?;
            finish(store, &ticket.id, sup, &mut session, cur, Outcome::Reported { status: report.status }, final_state)?;
            return Ok(QualityOutcome::Reported { status: report.status });
        }

        // A `done` report enters review; the board reflects it.
        cur = transition_ticket(store, &ticket.id, cur, TicketState::InReview)?;

        // --- the exit gate ---
        let gate_report = gate.run(&opts.cwd).context("running the exit gate")?;
        let gate_passed = gate_report.passed();
        emit(store, FleetEvent::GateResult {
            ticket: ticket.id.clone(),
            slot: sup.slot,
            outcome: if gate_passed { GateOutcome::Pass } else { GateOutcome::Fail },
        });
        // Persist the report with the gate results attached (handoff §4).
        let mut report = report;
        report.gate = Some(gate_report.to_gate_results());
        store.save_report(&ticket.id, sup.slot, &report)?;

        if !gate_passed {
            if gate_bounces >= opts.gate_retry_cap {
                notice(store, NoticeLevel::Warn, format!("{}: gate still failing after {gate_bounces} bounces; escalating", ticket.id));
                finish(store, &ticket.id, sup, &mut session, cur, Outcome::Reported { status: ReportStatus::Blocked }, TicketState::Blocked)?;
                return Ok(QualityOutcome::GateEscalated);
            }
            gate_bounces += 1;
            notice(store, NoticeLevel::Info, format!("{}: gate failed; bounce {gate_bounces}", ticket.id));
            // Back to the worker with the failing checks; it stays InReview→…
            cur = transition_ticket(store, &ticket.id, cur, TicketState::InProgress)?;
            session.send_user(&gate_report.bounce_message(&ticket.id)).context("gate bounce")?;
            continue;
        }

        // --- peer review (optional) ---
        let Some(reviewer) = &opts.reviewer else {
            finish(store, &ticket.id, sup, &mut session, cur, Outcome::Reported { status: ReportStatus::Done }, TicketState::Done)?;
            return Ok(QualityOutcome::Passed { gate_bounces, review_bounces });
        };

        let verdict = review(review_agent, reviewer, ticket, store, sup)?;
        let approved = verdict.as_ref().map(|v| v.approved()).unwrap_or(false);
        emit(store, FleetEvent::ReviewResult {
            ticket: ticket.id.clone(),
            reviewer_slot: reviewer.slot,
            outcome: if approved { ReviewOutcome::Approved } else { ReviewOutcome::ChangesRequested },
        });

        if approved {
            finish(store, &ticket.id, sup, &mut session, cur, Outcome::Reported { status: ReportStatus::Done }, TicketState::Done)?;
            return Ok(QualityOutcome::Passed { gate_bounces, review_bounces });
        }

        // Changes requested (or the reviewer filed no verdict — treat as changes
        // to stay conservative). Bounce to the worker, capped.
        if review_bounces >= opts.review_retry_cap {
            notice(store, NoticeLevel::Warn, format!("{}: review still requesting changes after {review_bounces} bounces; escalating", ticket.id));
            finish(store, &ticket.id, sup, &mut session, cur, Outcome::Reported { status: ReportStatus::Blocked }, TicketState::Blocked)?;
            return Ok(QualityOutcome::ReviewEscalated);
        }
        review_bounces += 1;
        notice(store, NoticeLevel::Info, format!("{}: review requested changes; bounce {review_bounces}", ticket.id));
        let bounce = verdict
            .map(|v| v.bounce_message(&ticket.id))
            .unwrap_or_else(|| format!("Peer review of {} produced no clear verdict; re-check the acceptance criteria and file an updated report.", ticket.id));
        cur = transition_ticket(store, &ticket.id, cur, TicketState::InProgress)?;
        session.send_user(&bounce).context("review bounce")?;
    }
}

/// Dispatch one peer-review round: spawn a fresh reviewer, hand it the AC, read
/// its `fleet-review` verdict from the transcript. `Ok(None)` means the reviewer
/// filed no parseable verdict (its session still gets reaped).
fn review(
    review_agent: &dyn AgentProcess,
    reviewer: &Reviewer,
    ticket: &Ticket,
    store: &dyn Store,
    sup: &SuperviseOptions,
) -> Result<Option<ReviewVerdict>> {
    let mut session = Session::spawn(review_agent, reviewer.raw_log.clone())
        .with_context(|| format!("spawning reviewer for {}", ticket.id))?;
    let deadline = Instant::now() + sup.idle_timeout;

    session.send_user(&review_prompt(ticket)).context("dispatching review")?;
    // Reuse the worker turn reader via a reviewer-flavored SuperviseOptions so a
    // reviewer that forgets its verdict is reprompted the same way.
    let rsup = SuperviseOptions {
        slot: reviewer.slot,
        raw_log: reviewer.raw_log.clone(),
        max_turns: sup.max_turns,
        idle_timeout: sup.idle_timeout,
    };
    let verdict = match drive_to_verdict(&mut session, ticket, &rsup, store, deadline)? {
        Some(text) => match ReviewVerdict::from_transcript_text(&text) {
            Some(Ok(v)) => Some(v),
            Some(Err(e)) => {
                notice(store, NoticeLevel::Warn, format!("{}: malformed review verdict: {e}", ticket.id));
                None
            }
            None => {
                notice(store, NoticeLevel::Warn, format!("{}: reviewer filed no verdict", ticket.id));
                None
            }
        },
        None => None,
    };
    session.close_stdin();
    session.kill();
    Ok(verdict)
}

/// Drive the reviewer to a single turn end and return its assistant text.
/// `None` on a wedge / crash (a notice is emitted). Reuses [`drive_to_report`]'s
/// reader by treating the reviewer like a worker whose "report" is the verdict:
/// we drive one turn and hand back the raw text for the review parser.
fn drive_to_verdict(
    session: &mut Session,
    ticket: &Ticket,
    sup: &SuperviseOptions,
    store: &dyn Store,
    deadline: Instant,
) -> Result<Option<String>> {
    // The reviewer's `fleet-review` block is not a `fleet-report`, so
    // drive_to_report would reprompt it as "no report". Instead we read one turn
    // directly. A reviewer that ends without a verdict is handled by the caller.
    match crate::supervisor::read_turn_text(session, ticket, sup, store, deadline)? {
        Some(text) => Ok(Some(text)),
        None => {
            notice(store, NoticeLevel::Error, format!("{}: reviewer wedged or crashed", ticket.id));
            Ok(None)
        }
    }
}

/// The review prompt (handoff §7 "slash commands become worker prompt
/// templates" — hardcoded for Phase 3; user-editable markdown later).
fn review_prompt(ticket: &Ticket) -> String {
    format!(
        "You are a peer reviewer. A teammate has finished ticket {id} and the exit \
         gate is green. Review their change in this worktree against the acceptance \
         criteria below — read the diff (`git diff`) and the files. You did not write \
         this code; look for what self-review misses.\n\n\
         Ticket {id}: {title}\n\n{body}\n\n\
         End your turn with a single fenced `fleet-review` block: a JSON object with \
         keys `decision` (\"approve\" or \"request-changes\"), `summary`, and \
         `blocking` (array of required changes; empty on approve). Example:\n\n\
         ```fleet-review\n{{\"decision\":\"approve\",\"summary\":\"meets the AC\",\"blocking\":[]}}\n```",
        id = ticket.id,
        title = ticket.title,
        body = ticket.body,
    )
}
