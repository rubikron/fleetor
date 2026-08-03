//! The Phase 4c **live event bus** — the push side of the append-only event log,
//! for the UI (BUILDING §4.2, README roadmap "4c").
//!
//! Phases 1–4b made every state change a [`FleetEvent`] persisted through
//! [`Store::append_event`], read back by *polling* [`Store::events_since`]. That
//! is correct but laggy: a UI would poll the DB on a timer. 4c adds a **live
//! push** so a subscriber learns of an event the moment it is appended — from
//! the async [`hub`](crate::hub) tasks and the app's own notices, with no change
//! to either.
//!
//! **How it hooks in without touching the loops:** every event already funnels
//! through the one chokepoint `Store::append_event`. [`BroadcastStore`] is a
//! decorator over any `Store` — it delegates every method and, right after a
//! successful append, publishes `(seq, event)` on a [`tokio::sync::broadcast`]
//! channel. Wrap the inner store once and every emitter publishes for free.
//! Unwrap it and you are back to a plain store (fully reversible — BUILDING §8).
//!
//! **The DB stays the source of truth.** Persist happens *before* publish, and
//! publish is best-effort (a send with no subscribers is a no-op — the event is
//! already durable). A subscriber that joins late, or falls behind the bounded
//! channel, recovers from the DB via the existing `events_since` cursor. The bus
//! only *accelerates* delivery; it never becomes a second source of truth. This
//! is the same "shared, persisted log" bridge 4b used across the sync↔async seam
//! (D-019), now exposed as a stream.

use anyhow::Result;
use fleetor_core::event::FleetEvent;
use fleetor_core::Store;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Ring capacity of the live channel. A slow subscriber that falls this far
/// behind is dropped from the ring and recovers from the DB (see
/// [`EventFollower`]) — so this is a delivery-latency / recovery-frequency knob,
/// never a correctness one. Generous, since a `(seq, FleetEvent)` is cheap and
/// the fleet is a handful of workers, not a firehose.
pub const BUS_CAPACITY: usize = 1024;

/// The live fan-out of appended events. Clone-cheap (an `Arc`-backed
/// [`broadcast::Sender`]); every clone publishes to, and every [`subscribe`] reads
/// from, the same channel.
///
/// [`subscribe`]: EventBus::subscribe
#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<(i64, FleetEvent)>,
}

impl EventBus {
    /// A fresh bus with [`BUS_CAPACITY`] ring slots.
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(BUS_CAPACITY);
        Self { tx }
    }

    /// Publish one appended event. Best-effort: an error means there are no live
    /// subscribers, which is fine — the event is already persisted, and any
    /// future subscriber picks it up from the DB snapshot. Callable from a sync
    /// thread (the supervisor) or an async task (the hub); `send` needs no runtime.
    pub fn publish(&self, seq: i64, event: FleetEvent) {
        let _ = self.tx.send((seq, event));
    }

    /// A raw receiver on the live channel. Prefer [`EventBus::follow`], which adds
    /// the DB snapshot and lag recovery a UI needs; this is the low-level handle.
    pub fn subscribe(&self) -> broadcast::Receiver<(i64, FleetEvent)> {
        self.tx.subscribe()
    }

    /// A gap-free, dup-free stream of every event **after** `after`, oldest
    /// first: the DB history up to now, then live events as they land. `after = 0`
    /// replays the whole log. This is the UI-facing primitive (a Tauri event
    /// pump, a `--follow` printer, the 4d orchestrator feed).
    ///
    /// Subscribe-then-snapshot ordering is what makes it gap-free: the live
    /// receiver starts buffering *before* the history read, so anything appended
    /// during the read is caught by one path or the other, and the `seq` cursor
    /// de-duplicates the overlap.
    pub fn follow(&self, store: Arc<dyn Store>, after: i64) -> Result<EventFollower> {
        // Order matters: subscribe first so the ring captures everything from now
        // on, *then* read history — no window in which an event is in neither.
        let rx = self.subscribe();
        let mut pending = VecDeque::new();
        for (seq, ev) in store.events_since(after)? {
            pending.push_back((seq, ev));
        }
        Ok(EventFollower { store, rx, cursor: after, pending })
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

/// An async cursor over the event log that yields every event exactly once, in
/// `seq` order, live. Built by [`EventBus::follow`].
///
/// It reconciles two sources — the DB history buffered at construction and the
/// live channel — by tracking the last `seq` it emitted (`cursor`): a live item
/// at or below the cursor is a replay of history and is skipped. If the
/// subscriber falls behind the bounded channel ([`broadcast`] reports `Lagged`),
/// it re-reads the tail from the DB rather than dropping events — the DB is the
/// source of truth, the channel only the fast path.
pub struct EventFollower {
    store: Arc<dyn Store>,
    rx: broadcast::Receiver<(i64, FleetEvent)>,
    /// Highest `seq` already yielded; the de-dup / recovery watermark.
    cursor: i64,
    /// History (or post-lag catch-up) waiting to be drained before the live tail.
    pending: VecDeque<(i64, FleetEvent)>,
}

impl EventFollower {
    /// The next event, or `None` when the bus is closed (every [`EventBus`]
    /// dropped) and history is drained — the end of the stream.
    pub async fn next(&mut self) -> Result<Option<(i64, FleetEvent)>> {
        loop {
            if let Some((seq, ev)) = self.pending.pop_front() {
                // Guard against a history/catch-up row we somehow already passed.
                if seq <= self.cursor {
                    continue;
                }
                self.cursor = seq;
                return Ok(Some((seq, ev)));
            }
            match self.rx.recv().await {
                Ok((seq, ev)) => {
                    if seq <= self.cursor {
                        continue; // overlap with history already yielded — skip
                    }
                    self.cursor = seq;
                    return Ok(Some((seq, ev)));
                }
                // Fell behind the ring: refill from the DB (the durable log) and
                // resume. Everything in the ring is already committed (publish
                // follows persist), so this catch-up is a superset of what we lost.
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    for (seq, ev) in self.store.events_since(self.cursor)? {
                        self.pending.push_back((seq, ev));
                    }
                }
                // All senders gone — no more live events will ever arrive.
                Err(broadcast::error::RecvError::Closed) => return Ok(None),
            }
        }
    }
}

/// A [`Store`] decorator that publishes every appended event on an [`EventBus`]
/// while delegating all persistence to the inner store. Wrap the real store once
/// and every emitter (supervisor, hub) feeds the live bus for free.
///
/// It *is* a `Store`, so it drops straight into anything that takes
/// `Arc<dyn Store>` (e.g. [`run_fleet`](crate::run_fleet)); keep the concrete
/// `Arc<BroadcastStore>` around to [`subscribe`](BroadcastStore::subscribe) /
/// [`follow`](BroadcastStore::follow) before coercing it to `Arc<dyn Store>`.
pub struct BroadcastStore {
    inner: Arc<dyn Store>,
    bus: EventBus,
}

impl BroadcastStore {
    /// Wrap `inner` with a fresh bus.
    pub fn new(inner: Arc<dyn Store>) -> Self {
        Self { inner, bus: EventBus::new() }
    }

    /// The bus these appends publish to — clone it to hand out subscriptions.
    pub fn bus(&self) -> EventBus {
        self.bus.clone()
    }

    /// A raw live receiver (see [`EventBus::subscribe`]).
    pub fn subscribe(&self) -> broadcast::Receiver<(i64, FleetEvent)> {
        self.bus.subscribe()
    }

    /// A snapshot-plus-live stream from `after` (see [`EventBus::follow`]).
    pub fn follow(&self, after: i64) -> Result<EventFollower> {
        self.bus.follow(self.inner.clone(), after)
    }
}

// Delegate every `Store` method to the inner store; the one override is
// `append_event`, which publishes the appended event's real `seq` after it lands.
impl Store for BroadcastStore {
    fn append_event(&self, event: &FleetEvent) -> Result<i64> {
        // Persist first: the DB is the source of truth. Only a durable event is
        // published, so the bus can never surface something the log lacks.
        let seq = self.inner.append_event(event)?;
        self.bus.publish(seq, event.clone());
        Ok(seq)
    }

    fn events_since(&self, after: i64) -> Result<Vec<(i64, FleetEvent)>> {
        self.inner.events_since(after)
    }
    fn latest_seq(&self) -> Result<i64> {
        self.inner.latest_seq()
    }
}
