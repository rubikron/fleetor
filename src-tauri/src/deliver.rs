//! Where a message stops being a record and becomes bytes in a terminal (D-030).
//!
//! The hub routes; this owns the machinery. One [`AppCommand`] loop, a registry
//! lookup, a pty write, and an honest answer back — and **nothing else**. No
//! queue, no idle guard, no flush ticker, no retry, no deadline. The thing this
//! replaces had all five, and every one of them could withhold a message
//! indefinitely while the log said it was sent (D-034).
//!
//! Two shapes here are the whole design:
//!
//!  - **The loop never blocks.** Each `Deliver` goes to `spawn_blocking`, because
//!    the write holds a pty writer lock across a 30 ms submit gap. Inline, five
//!    panes would serialize behind each other and a slow pane would stall every
//!    message queued behind it — and the hub awaits broadcast legs one at a time,
//!    so that cost multiplies by the roster.
//!  - **Every command is answered.** The hub waits on the ack with no deadline,
//!    so a command received and dropped parks the caller's `fleet send` forever.
//!    A rejection is an outcome; silence is a hang.

use std::sync::Arc;

use fleetor_server::{AppCommand, DeliveryResult};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;

use crate::pty::PaneRegistry;

/// Serve the hub's pane commands from `registry` until the app shuts down.
pub fn spawn_delivery(
    rt: &Runtime,
    registry: Arc<PaneRegistry>,
    mut commands: mpsc::UnboundedReceiver<AppCommand>,
) {
    rt.spawn(async move {
        while let Some(command) = commands.recv().await {
            match command {
                // Cheap and lock-only: answering inline costs nothing and keeps
                // `fleet roster` honest about the instant it was asked.
                AppCommand::Roster { ack } => {
                    let _ = ack.send(registry.roster());
                }
                AppCommand::Deliver { to, text, ack } => {
                    let registry = registry.clone();
                    tokio::task::spawn_blocking(move || {
                        let _ = ack.send(outcome_of(registry.write_paste(to, &text)));
                    });
                }
            }
        }
    });
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
    use fleetor_core::pane::PaneId;
    use tokio::sync::oneshot;

    fn registry() -> Arc<PaneRegistry> {
        Arc::new(PaneRegistry::new(Arc::new(|_, _| {})))
    }

    #[test]
    fn a_successful_write_is_accepted_and_a_failed_one_carries_its_reason() {
        assert_eq!(outcome_of(Ok(())), DeliveryResult::accepted());
        assert_eq!(
            outcome_of(Err("worker-3 has exited".into())),
            DeliveryResult::rejected("worker-3 has exited"),
        );
    }

    /// The failure that hangs the fleet: a command the loop understands but never
    /// answers. The hub has no deadline, so an unanswered `Deliver` parks `fleet
    /// send` forever — a missing pane must produce a *refusal*, not silence.
    #[test]
    fn a_message_to_a_pane_that_is_not_running_is_refused_rather_than_dropped() {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
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

    /// The roster is asked, not declared: before anything is spawned the fleet is
    /// empty, and a `fleet broadcast` must be told so rather than fanning out to
    /// panes that do not exist.
    #[test]
    fn the_roster_answers_even_when_no_pane_has_been_spawned() {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let (tx, rx) = mpsc::unbounded_channel();
        spawn_delivery(&rt, registry(), rx);

        let (ack, answer) = oneshot::channel();
        tx.send(AppCommand::Roster { ack }).unwrap();
        assert!(rt.block_on(answer).expect("the ack must arrive").is_empty());
    }
}
