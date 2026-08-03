//! The live event bus: the push side of the event log must carry *exactly* what
//! the persisted log holds — every event, in `seq` order, no gaps, no duplicates
//! — for a subscriber that was live throughout and for one that joins late, plus
//! lag recovery from the DB.
//!
//! Phase 5 removed the supervisor these tests used to run behind, so they now
//! append through [`BroadcastStore`] themselves. That is the more honest shape
//! anyway: the bus's contract is about the *store* chokepoint, not about who
//! happens to be writing through it, and the app's own notices go through the
//! same door as the hub's messages.

use std::sync::Arc;
use std::time::Duration;

use fleetor_core::event::{FleetEvent, NoticeLevel};
use fleetor_core::pane::PaneId;
use fleetor_core::{ids, Store};
use fleetor_db::SqliteStore;
use fleetor_server::{BroadcastStore, EventBus, EventFollower, BUS_CAPACITY};

fn message(body: &str) -> FleetEvent {
    FleetEvent::Message {
        id: ids::new_id("msg"),
        from: PaneId::Orch,
        to: PaneId::Worker(2),
        body: body.into(),
        group: None,
        accepted: true,
        detail: None,
    }
}

fn notice(text: &str) -> FleetEvent {
    FleetEvent::Notice { level: NoticeLevel::Info, text: text.into() }
}

fn store() -> Arc<BroadcastStore> {
    Arc::new(BroadcastStore::new(Arc::new(SqliteStore::open_in_memory().unwrap())))
}

/// Pull exactly `want` items, failing loudly rather than hanging if the stream
/// stalls — a lost event is the bug these tests exist to catch.
async fn collect(follower: &mut EventFollower, want: usize) -> Vec<(i64, FleetEvent)> {
    let mut out = Vec::new();
    while out.len() < want {
        match tokio::time::timeout(Duration::from_secs(5), follower.next()).await {
            Ok(Ok(Some(item))) => out.push(item),
            other => panic!("follower stopped after {} of {want}: {other:?}", out.len()),
        }
    }
    out
}

/// A subscriber that is already listening sees every appended event, in `seq`
/// order, and sees nothing the log does not contain.
#[tokio::test]
async fn a_live_subscriber_receives_exactly_the_persisted_log() {
    let store = store();
    let mut follower = store.follow(0).unwrap();

    store.append_event(&message("take the parser")).unwrap();
    store.append_event(&notice("worker-3 has no fleet binary")).unwrap();
    store.append_event(&message("on it")).unwrap();

    let seen = collect(&mut follower, 3).await;
    assert_eq!(seen.iter().map(|(seq, _)| *seq).collect::<Vec<_>>(), vec![1, 2, 3]);
    assert_eq!(
        seen.iter().map(|(_, e)| e.kind()).collect::<Vec<_>>(),
        ["message", "notice", "message"],
    );
    assert_eq!(store.events_since(0).unwrap(), seen, "the stream and the log agree exactly");
}

/// A subscriber that arrives after the fact replays the whole history from the
/// DB, then continues live, with no gap or repeat at the boundary — the UI's
/// mount path.
#[tokio::test]
async fn a_late_subscriber_gets_the_full_history_then_live_events() {
    let store = store();
    store.append_event(&message("before you arrived")).unwrap();
    store.append_event(&notice("also before")).unwrap();

    let mut follower = store.follow(0).unwrap();
    let history = collect(&mut follower, 2).await;
    assert_eq!(history, store.events_since(0).unwrap(), "snapshot != persisted log");

    let seq = store.append_event(&message("after you arrived")).unwrap();
    let live = collect(&mut follower, 1).await;
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].0, seq, "the live event follows the snapshot's last seq exactly");
}

/// A follower that falls further behind than the ring holds recovers what it
/// missed from the DB rather than losing it. This is the reason the bus is
/// allowed to be bounded at all.
#[tokio::test]
async fn a_lagged_follower_recovers_every_event_from_the_db() {
    let store = store();
    // Subscribe, then flood well past the ring capacity *without* reading — the
    // receiver is guaranteed to lag.
    let mut follower = store.follow(0).unwrap();
    let total = BUS_CAPACITY + 250;
    for i in 0..total {
        store.append_event(&message(&format!("m{i}"))).unwrap();
    }

    let seen = collect(&mut follower, total).await;
    assert_eq!(
        seen.iter().map(|(seq, _)| *seq).collect::<Vec<_>>(),
        (1..=total as i64).collect::<Vec<_>>(),
        "a lagged follower must yield every seq exactly once, in order",
    );
}

/// Sanity: a bare `EventBus` fans a published event to every current subscriber
/// — the primitive under `BroadcastStore`.
#[tokio::test]
async fn the_bare_bus_fans_out_to_all_subscribers() {
    let inner: Arc<dyn Store> = Arc::new(SqliteStore::open_in_memory().unwrap());
    let bus = EventBus::new();
    let mut a = bus.follow(inner.clone(), 0).unwrap();
    let mut b = bus.follow(inner.clone(), 0).unwrap();

    let event = notice("hi");
    bus.publish(7, event.clone());

    assert_eq!(a.next().await.unwrap(), Some((7, event.clone())));
    assert_eq!(b.next().await.unwrap(), Some((7, event)));
}
