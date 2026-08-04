//! Where a message stops being a record and becomes bytes in a terminal (D-030).
//!
//! The hub routes; this owns the machinery. One [`AppCommand`] loop, a per-pane
//! writer, a pty write, and an honest answer back — and **nothing else**. No idle
//! guard, no flush ticker, no retry, no deadline. The thing this replaces had all
//! four, and every one of them could withhold a message indefinitely while the
//! log said it was sent (D-034).
//!
//! ## One writer per pane, draining
//!
//! Each pane gets a single task that owns its terminal. When that task is free it
//! takes **everything** currently queued for that pane and sends it as one
//! bracketed paste. This is the D-039 fix for two problems observed the first
//! time several workers messaged the orchestrator at once:
//!
//!  1. **Ordering was ours to lose.** Every `Deliver` used to get its own
//!     `spawn_blocking` task, and those raced for the pane's writer lock in
//!     whatever order the blocking pool scheduled them. Nothing was dropped, but
//!     a pane could see messages in an order the log disagreed with. A single
//!     ordered queue per pane makes the hub's order the pane's order by
//!     construction.
//!  2. **The receiving TUI was merging them badly.** A `claude` input box is one
//!     text field: message A submits and starts a turn, B's paste lands mid-turn
//!     and is queued by CC, C lands on the same buffer. What arrives is a blend.
//!     We cannot reach inside the TUI — but we *can* do the merging ourselves,
//!     where the framing is still ours, so the model sees three delimited
//!     `[fleet · …]` blocks instead of a blur.
//!
//! **This is not the batching the pivot bans.** That was an idle guard plus a
//! flush ticker that could hold a message back indefinitely. This adds **zero**
//! delay to a message arriving at a free pane — `try_recv` takes what is already
//! there and never waits for more. It only groups messages that were already
//! concurrent, which is exactly the set the TUI was going to merge anyway. Every
//! message keeps its own log row and its own `accepted`.
//!
//! What this still cannot fix: a pane thirty seconds into a turn. Messages
//! arriving across that window land in CC's own queue and are at its mercy. That
//! is why `accepted` has never meant "delivered" (L3).

use std::collections::HashMap;
use std::sync::Arc;

use fleetor_core::pane::PaneId;
use fleetor_server::{AppCommand, DeliveryResult};
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, oneshot};

use crate::pty::PaneRegistry;

/// Between two messages in one paste. A blank line, because bodies may be
/// multi-line themselves (Phase 0 proved they arrive intact) and the `[fleet · …]`
/// prefix alone is a weak boundary when the message before it wrapped.
const SEPARATOR: &str = "\n\n";

/// The pane's writer task is gone — only reachable if the app is tearing down.
const WRITER_GONE: &str = "the pane's writer has stopped — the app may be closing";

/// One message waiting for its pane, with the channel that owes its sender an
/// answer. The ack travels *with* the message so it cannot be forgotten: the hub
/// waits on it with no deadline, and a dropped ack parks `fleet send` forever.
struct Pending {
    text: String,
    ack: oneshot::Sender<DeliveryResult>,
}

/// Serve the hub's pane commands from `registry` until the app shuts down.
pub fn spawn_delivery(
    rt: &Runtime,
    registry: Arc<PaneRegistry>,
    mut commands: mpsc::UnboundedReceiver<AppCommand>,
) {
    rt.spawn(async move {
        let mut outboxes: HashMap<PaneId, mpsc::UnboundedSender<Pending>> = HashMap::new();

        while let Some(command) = commands.recv().await {
            match command {
                // Cheap and lock-only: answering inline costs nothing and keeps
                // `fleet roster` honest about the instant it was asked.
                AppCommand::Roster { ack } => {
                    let _ = ack.send(registry.roster());
                }
                AppCommand::Deliver { to, text, ack } => {
                    let outbox = outboxes
                        .entry(to)
                        .or_insert_with(|| spawn_writer(registry.clone(), to));
                    // Handing off is instant — the queue is unbounded, so this
                    // loop can never be the thing that stalls a busy fleet.
                    if let Err(returned) = outbox.send(Pending { text, ack }) {
                        let _ = returned.0.ack.send(DeliveryResult::rejected(WRITER_GONE));
                    }
                }
            }
        }
    });
}

/// The single writer for one pane: take a message, drain whatever else is
/// already waiting, send the lot as one paste, then answer every sender.
fn spawn_writer(registry: Arc<PaneRegistry>, pane: PaneId) -> mpsc::UnboundedSender<Pending> {
    let (tx, mut rx) = mpsc::unbounded_channel::<Pending>();
    tokio::spawn(async move {
        while let Some(first) = rx.recv().await {
            let mut batch = vec![first];
            // `try_recv`, never `recv` — take what is already here and go. Waiting
            // even briefly for more would be the flush ticker again.
            while let Ok(next) = rx.try_recv() {
                batch.push(next);
            }

            let body = join(batch.iter().map(|p| p.text.as_str()));
            let registry = registry.clone();
            // The write holds a pty lock across the 30 ms submit gap, so it goes
            // to a blocking thread — one pane's terminal must never stall four
            // others, and the hub awaits broadcast legs one at a time.
            let written = tokio::task::spawn_blocking(move || registry.write_paste(pane, &body))
                .await
                .unwrap_or_else(|e| Err(format!("the write to {pane} did not run: {e}")));

            let result = outcome_of(written);
            for pending in batch {
                let _ = pending.ack.send(result.clone());
            }
        }
    });
    tx
}

/// The bytes for one batch. Kept separate and pure so the framing is testable
/// without a pty.
fn join<'a>(texts: impl Iterator<Item = &'a str>) -> String {
    texts.collect::<Vec<_>>().join(SEPARATOR)
}

/// A pty write's result as the fleet reports it.
///
/// `accepted` means the bytes were queued to a live pty — never that the agent
/// read them (L3). A failed write becomes a `detail` the sending model reads on
/// its own stderr and can act on; it is never swallowed.
fn outcome_of(write: Result<(), String>) -> DeliveryResult {
    match write {
        Ok(()) => DeliveryResult::accepted(),
        Err(detail) => DeliveryResult::rejected(detail),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fleetor_core::message::frame_for_pane;

    fn registry() -> Arc<PaneRegistry> {
        let path = std::env::temp_dir().join(format!(
            "fleetor-deliver-test-registry-{}-{:?}.pids",
            std::process::id(),
            std::thread::current().id()
        ));
        Arc::new(PaneRegistry::new(Arc::new(|_, _| {}), path))
    }

    fn runtime() -> Runtime {
        tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap()
    }

    #[test]
    fn a_successful_write_is_accepted_and_a_failed_one_carries_its_reason() {
        assert_eq!(outcome_of(Ok(())), DeliveryResult::accepted());
        assert_eq!(
            outcome_of(Err("worker-3 has exited".into())),
            DeliveryResult::rejected("worker-3 has exited"),
        );
    }

    /// A batched paste must stay readable as N messages, not one blur. Each is
    /// already framed with its sender, and a blank line keeps that boundary
    /// legible when a body of its own wrapped onto several lines.
    #[test]
    fn a_batch_keeps_every_message_delimited_and_attributed() {
        let framed = [
            frame_for_pane(PaneId::Worker(1), "three things:\nfirst\nsecond"),
            frame_for_pane(PaneId::Worker(2), "I took the parser"),
        ];
        let body = join(framed.iter().map(String::as_str));

        assert_eq!(
            body,
            "[fleet · worker-1] three things:\nfirst\nsecond\n\n[fleet · worker-2] I took the parser"
        );
        assert_eq!(body.matches("[fleet · ").count(), 2, "both senders survive the join");
    }

    /// A single message is never padded — batching must be invisible when there
    /// is nothing to batch.
    #[test]
    fn one_message_is_written_exactly_as_it_was_framed() {
        let framed = frame_for_pane(PaneId::Orch, "take T-4");
        assert_eq!(join(std::iter::once(framed.as_str())), framed);
    }

    /// The failure that hangs the fleet: a command the loop understands but never
    /// answers. The hub has no deadline, so an unanswered `Deliver` parks `fleet
    /// send` forever — a missing pane must produce a *refusal*, not silence.
    #[test]
    fn a_message_to_a_pane_that_is_not_running_is_refused_rather_than_dropped() {
        let rt = runtime();
        let (tx, rx) = mpsc::unbounded_channel();
        spawn_delivery(&rt, registry(), rx);

        let (ack, answer) = oneshot::channel();
        tx.send(AppCommand::Deliver { to: PaneId::Worker(2), text: "take T-4".into(), ack }).unwrap();

        let result = rt.block_on(answer).expect("the ack must arrive — silence is a hang");
        assert!(!result.accepted);
        assert!(
            result.detail.as_deref().unwrap_or_default().contains("worker-2"),
            "the refusal must name the pane so the model can act on it: {result:?}"
        );
    }

    /// **Every** sender in a batch gets its own answer. Batching groups the
    /// write, never the accounting: four `fleet send`s that shared one paste are
    /// still four commands waiting on four acks, and one left unanswered parks
    /// that pane's model indefinitely.
    #[test]
    fn every_message_in_a_burst_is_answered_even_when_they_share_a_write() {
        let rt = runtime();
        let (tx, rx) = mpsc::unbounded_channel();
        spawn_delivery(&rt, registry(), rx);

        let answers: Vec<_> = (1..=4)
            .map(|i| {
                let (ack, answer) = oneshot::channel();
                tx.send(AppCommand::Deliver {
                    to: PaneId::Orch,
                    text: format!("message {i}"),
                    ack,
                })
                .unwrap();
                answer
            })
            .collect();

        for (i, answer) in answers.into_iter().enumerate() {
            let result = rt.block_on(answer).unwrap_or_else(|_| panic!("message {i} went unanswered"));
            assert!(!result.accepted, "no pane is running, so all four are refused");
        }
    }

    /// The roster is asked, not declared: before anything is spawned the fleet is
    /// empty, and a `fleet broadcast` must be told so rather than fanning out to
    /// panes that do not exist.
    #[test]
    fn the_roster_answers_even_when_no_pane_has_been_spawned() {
        let rt = runtime();
        let (tx, rx) = mpsc::unbounded_channel();
        spawn_delivery(&rt, registry(), rx);

        let (ack, answer) = oneshot::channel();
        tx.send(AppCommand::Roster { ack }).unwrap();
        assert!(rt.block_on(answer).expect("the ack must arrive").is_empty());
    }
}
