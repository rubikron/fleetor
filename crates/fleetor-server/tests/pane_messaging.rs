//! D-030 pane messaging — the whole `fleet` surface proved in-process, against a
//! real socket and a fake fleet app standing in for the pty registry. No `claude`
//! processes, no terminals, no spend: the app seam is request/response, so a test
//! can answer a delivery exactly the way a live pane, a dead pane, or a wedged app
//! would, and assert on what the hub then wrote to the log.
//!
//! This is the test that has to hold. Everything Phase 3 builds on top of it is
//! pty plumbing; the routing decisions all live here.

use fleetor_core::event::FleetEvent;
use fleetor_core::pane::{PaneEntry, PaneId, PaneState};
use fleetor_core::wire::{Hello, Op, OpResult};
use fleetor_core::Store;
use fleetor_db::SqliteStore;
use fleetor_ipc::{Client, Transport, UnixTransport};
use fleetor_server::{AppCommand, DeliveryResult, Hub, HubConfig, PaneConfig};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

const SLOTS: [u8; 3] = [1, 2, 3];

/// Everything the fake app wrote into a pty, in order, as (pane, exact bytes).
type Writes = Arc<Mutex<Vec<(PaneId, String)>>>;

/// A stand-in for the Phase-3 pty registry: it holds a fixed roster and answers
/// `Deliver` the way the real one will — accepted only for a pane that is live.
fn spawn_app(roster: Vec<PaneEntry>) -> (mpsc::UnboundedSender<AppCommand>, Writes) {
    let (tx, mut rx) = mpsc::unbounded_channel::<AppCommand>();
    let writes: Writes = Arc::new(Mutex::new(Vec::new()));
    let sink = writes.clone();
    tokio::spawn(async move {
        let states: HashMap<PaneId, PaneState> =
            roster.iter().map(|e| (e.pane, e.state)).collect();
        while let Some(cmd) = rx.recv().await {
            match cmd {
                AppCommand::Deliver { to, text, ack } => {
                    // `accepts_input`, not `is_live` — the registry refuses only a
                    // dead pane. A still-spawning pty is written to; the kernel
                    // buffers until the TUI reads.
                    let result = match states.get(&to) {
                        Some(state) if state.accepts_input() => {
                            sink.lock().unwrap().push((to, text));
                            DeliveryResult::accepted()
                        }
                        Some(_) => DeliveryResult::rejected(format!("pane {to} is dead")),
                        None => DeliveryResult::rejected(format!("no pane {to} is running")),
                    };
                    let _ = ack.send(result);
                }
                AppCommand::Roster { ack } => {
                    let _ = ack.send(roster.clone());
                }
            }
        }
    });
    (tx, writes)
}

/// A hub wired to a fake app. `roster` is what the app reports; `SLOTS` is what
/// the hub is configured with — deliberately separate, because "this pane exists"
/// and "this pane is alive" are different questions with different owners.
async fn start_hub(
    roster: Vec<PaneEntry>,
    pane_config: PaneConfig,
) -> (Arc<UnixTransport>, Arc<SqliteStore>, Writes) {
    let dir = std::env::temp_dir().join(format!(
        "fleetor-pane-{}-{}",
        std::process::id(),
        fleetor_core::ids::new_id("t")
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let transport = Arc::new(UnixTransport::new(dir.join("fleet.sock")));
    let store = Arc::new(SqliteStore::open_in_memory().unwrap());
    let (app, writes) = spawn_app(roster);
    let hub = Hub::with_app(
        store.clone(),
        HubConfig { slots: SLOTS.to_vec(), ask_timeout: Duration::from_secs(5) },
        pane_config,
        app,
    );
    let listener = transport.bind().await.unwrap();
    tokio::spawn(hub.serve(listener));
    (transport, store, writes)
}

/// Every pane live — the ordinary case.
fn all_live() -> Vec<PaneEntry> {
    PaneId::roster(&SLOTS).into_iter().map(|p| PaneEntry::new(p, PaneState::Live)).collect()
}

async fn pane(transport: &UnixTransport, pane: PaneId) -> Client {
    Client::connect(transport, Hello::for_pane(pane)).await.unwrap()
}

/// Every `Message` in the log, oldest first.
fn messages(store: &SqliteStore) -> Vec<FleetEvent> {
    store
        .events_since(0)
        .unwrap()
        .into_iter()
        .map(|(_, e)| e)
        .filter(|e| matches!(e, FleetEvent::Message { .. }))
        .collect()
}

fn as_message(event: &FleetEvent) -> (PaneId, PaneId, &str, Option<&str>, bool, Option<&str>) {
    let FleetEvent::Message { from, to, body, group, accepted, detail, .. } = event else {
        panic!("not a message event: {event:?}");
    };
    (*from, *to, body, group.as_deref(), *accepted, detail.as_deref())
}

/// The happy path: orch → worker-2. The bytes that reach the pty are the framed
/// form, and the log keeps the raw body — the feed replays what was *said*, the
/// terminal receives what should be *typed*.
#[tokio::test]
async fn a_direct_send_reaches_a_live_pane_and_the_body_lands_in_the_log() {
    let (transport, store, writes) = start_hub(all_live(), PaneConfig::default()).await;
    let mut orch = pane(&transport, PaneId::Orch).await;

    let result = orch
        .call(Op::PaneSend { to: PaneId::Worker(2), text: "take the parser".into() })
        .await
        .unwrap();
    let OpResult::Delivered { msg_id, accepted, detail } = result else {
        panic!("expected a delivery, got {result:?}");
    };
    assert!(accepted, "a live pane accepts");
    assert_eq!(detail, None);

    assert_eq!(
        writes.lock().unwrap().clone(),
        vec![(PaneId::Worker(2), "[fleet · orch] take the parser".to_string())],
        "exactly one pane was written to, with the framed message"
    );

    let logged = messages(&store);
    assert_eq!(logged.len(), 1);
    let (from, to, body, group, ok, why) = as_message(&logged[0]);
    assert_eq!((from, to), (PaneId::Orch, PaneId::Worker(2)));
    assert_eq!(body, "take the parser", "the log carries the body, not just metadata");
    assert_eq!((group, ok, why), (None, true, None));
    let FleetEvent::Message { id: logged_id, .. } = &logged[0] else { unreachable!() };
    assert_eq!(logged_id, &msg_id, "the id the sender got back is the id in the log");
}

/// A dead pane rejects, the caller is told why in words it can act on, and the
/// attempt is still logged with `accepted: false` — an invisible failure is worse
/// than a loud one (L3). A pane that is merely still spawning **accepts**.
#[tokio::test]
async fn a_dead_pane_is_rejected_loudly_but_a_spawning_one_still_accepts() {
    let roster = vec![
        PaneEntry::new(PaneId::Orch, PaneState::Live),
        PaneEntry::new(PaneId::Worker(1), PaneState::Live),
        PaneEntry::new(PaneId::Worker(2), PaneState::Dead),
        PaneEntry::new(PaneId::Worker(3), PaneState::Spawning),
    ];
    let (transport, store, writes) = start_hub(roster, PaneConfig::default()).await;
    let mut orch = pane(&transport, PaneId::Orch).await;

    let dead = orch.call(Op::PaneSend { to: PaneId::Worker(2), text: "ping".into() }).await.unwrap();
    let OpResult::Delivered { accepted, detail, .. } = dead else {
        panic!("expected a delivery, got {dead:?}");
    };
    assert!(!accepted, "worker-2 is dead");
    assert_eq!(detail.as_deref(), Some("pane worker-2 is dead"), "the reason reaches the sender");

    // Still booting is not a reason to refuse: gating on a guess about whether the
    // TUI has reached its prompt is how a healthy pane silently goes mute.
    let spawning =
        orch.call(Op::PaneSend { to: PaneId::Worker(3), text: "ping".into() }).await.unwrap();
    assert!(
        matches!(spawning, OpResult::Delivered { accepted: true, .. }),
        "a spawning pane must accept: {spawning:?}"
    );

    assert_eq!(writes.lock().unwrap().len(), 1, "only the spawning pane was written to");
    let logged = messages(&store);
    assert_eq!(logged.len(), 2, "both attempts are on the record");
    assert!(!as_message(&logged[0]).4);
    assert!(as_message(&logged[1]).4);
}

/// Membership has exactly one source of truth: the app. A pane the app is not
/// running is rejected by the app, in its words — the hub does not keep a second,
/// config-derived opinion that could disagree with the machine actually holding
/// the ptys. A pane messaging itself is still refused by the hub, because that
/// check compares the sender to itself and cannot be wrong.
#[tokio::test]
async fn membership_is_the_apps_answer_and_a_self_send_is_refused() {
    let (transport, store, _writes) = start_hub(all_live(), PaneConfig::default()).await;
    let mut orch = pane(&transport, PaneId::Orch).await;

    let unknown = orch.call(Op::PaneSend { to: PaneId::Worker(9), text: "x".into() }).await.unwrap();
    let OpResult::Delivered { accepted, detail, .. } = unknown else {
        panic!("expected a delivery, got {unknown:?}");
    };
    assert!(!accepted);
    assert_eq!(detail.as_deref(), Some("no pane worker-9 is running"));

    let selfsend = orch.call(Op::PaneSend { to: PaneId::Orch, text: "x".into() }).await.unwrap();
    assert!(
        matches!(&selfsend, OpResult::Error { message } if message.contains("cannot message itself")),
        "got {selfsend:?}"
    );

    assert_eq!(messages(&store).len(), 1, "the undeliverable send is logged; the self-send is not");
}

/// A pane the hub was never configured with is still reachable if the app is
/// running it. Config is not allowed to veto the machine holding the ptys.
#[tokio::test]
async fn a_pane_outside_the_hub_config_is_still_reachable() {
    let mut roster = all_live();
    roster.push(PaneEntry::new(PaneId::Worker(7), PaneState::Live)); // not in SLOTS
    let (transport, _store, writes) = start_hub(roster, PaneConfig::default()).await;
    let mut orch = pane(&transport, PaneId::Orch).await;

    let result = orch.call(Op::PaneSend { to: PaneId::Worker(7), text: "hi".into() }).await.unwrap();
    assert!(matches!(result, OpResult::Delivered { accepted: true, .. }), "got {result:?}");
    assert_eq!(writes.lock().unwrap().len(), 1);
}

/// One `broadcast` becomes N−1 legs that share a group id, and the sender is not
/// among them. The framing differs from a direct message so the receiver can obey
/// the do-not-answer-a-broadcast rule in its brief (L5).
#[tokio::test]
async fn a_broadcast_fans_out_to_every_other_pane_under_one_group() {
    let (transport, store, writes) = start_hub(all_live(), PaneConfig::default()).await;
    let mut w1 = pane(&transport, PaneId::Worker(1)).await;

    let result = w1.call(Op::PaneBroadcast { text: "rebasing onto master".into() }).await.unwrap();
    let OpResult::Delivered { msg_id: group, accepted, detail } = result else {
        panic!("expected a delivery, got {result:?}");
    };
    assert!(accepted);
    assert_eq!(detail, None, "nothing failed, so there is nothing to explain");

    let written = writes.lock().unwrap().clone();
    let targets: Vec<PaneId> = written.iter().map(|(p, _)| *p).collect();
    assert_eq!(
        targets,
        vec![PaneId::Orch, PaneId::Worker(2), PaneId::Worker(3)],
        "everyone except the sender"
    );
    assert!(
        written.iter().all(|(_, text)| text == "[fleet · worker-1 → all] rebasing onto master"),
        "broadcast framing is visibly a broadcast: {written:?}"
    );

    let logged = messages(&store);
    assert_eq!(logged.len(), 3);
    for event in &logged {
        let (from, _, body, grp, ok, _) = as_message(event);
        assert_eq!(from, PaneId::Worker(1));
        assert_eq!(body, "rebasing onto master");
        assert_eq!(grp, Some(group.as_str()), "every leg carries the same group id");
        assert!(ok);
    }
}

/// A broadcast with some dead panes reports the partial outcome rather than a
/// bare success or a bare failure.
#[tokio::test]
async fn a_partly_undeliverable_broadcast_names_the_panes_that_missed_it() {
    let roster = vec![
        PaneEntry::new(PaneId::Orch, PaneState::Live),
        PaneEntry::new(PaneId::Worker(1), PaneState::Live),
        PaneEntry::new(PaneId::Worker(2), PaneState::Dead),
        PaneEntry::new(PaneId::Worker(3), PaneState::Dead),
    ];
    let (transport, _store, writes) = start_hub(roster, PaneConfig::default()).await;
    let mut w1 = pane(&transport, PaneId::Worker(1)).await;

    let result = w1.call(Op::PaneBroadcast { text: "status?".into() }).await.unwrap();
    let OpResult::Delivered { accepted, detail, .. } = result else {
        panic!("expected a delivery, got {result:?}");
    };
    assert!(accepted, "orch got it, so the broadcast was not a total loss");
    let detail = detail.expect("a partial fan-out must say what missed");
    assert!(detail.contains("worker-2") && detail.contains("worker-3"), "{detail}");
    assert!(!detail.contains("orch"), "the pane that received it is not a failure: {detail}");
    assert_eq!(writes.lock().unwrap().len(), 1);
}

/// `reply` needs no addressee: it goes to whoever last got a message *through* to
/// this pane. This is the verb workers use for almost everything.
#[tokio::test]
async fn reply_routes_to_whoever_last_got_through() {
    let (transport, _store, writes) = start_hub(all_live(), PaneConfig::default()).await;
    let mut orch = pane(&transport, PaneId::Orch).await;
    let mut w1 = pane(&transport, PaneId::Worker(1)).await;
    let mut w2 = pane(&transport, PaneId::Worker(2)).await;

    orch.call(Op::PaneSend { to: PaneId::Worker(2), text: "take the parser".into() }).await.unwrap();
    let replied = w2.call(Op::PaneReply { text: "on it".into() }).await.unwrap();
    assert!(matches!(replied, OpResult::Delivered { accepted: true, .. }), "got {replied:?}");

    // worker-1 now messages worker-2; worker-2's reply target moves to worker-1.
    w1.call(Op::PaneSend { to: PaneId::Worker(2), text: "I own src/api".into() }).await.unwrap();
    w2.call(Op::PaneReply { text: "noted".into() }).await.unwrap();

    assert_eq!(
        writes.lock().unwrap().clone(),
        vec![
            (PaneId::Worker(2), "[fleet · orch] take the parser".to_string()),
            (PaneId::Orch, "[fleet · worker-2] on it".to_string()),
            (PaneId::Worker(2), "[fleet · worker-1] I own src/api".to_string()),
            (PaneId::Worker(1), "[fleet · worker-2] noted".to_string()),
        ]
    );
}

/// Replying with nothing to reply to is an error that says what to do instead —
/// the model reads its own stderr and self-corrects.
#[tokio::test]
async fn reply_with_no_inbound_message_says_so() {
    let (transport, store, _writes) = start_hub(all_live(), PaneConfig::default()).await;
    let mut w3 = pane(&transport, PaneId::Worker(3)).await;

    let result = w3.call(Op::PaneReply { text: "hello?".into() }).await.unwrap();
    let OpResult::Error { message } = result else { panic!("expected an error, got {result:?}") };
    assert!(message.contains("nobody has messaged worker-3"), "{message}");
    assert!(message.contains("fleet send"), "the error names the verb to use instead: {message}");
    assert!(messages(&store).is_empty());
}

/// A rejected delivery must not become a reply target: you cannot answer a pane
/// that never heard you.
#[tokio::test]
async fn a_rejected_delivery_does_not_become_a_reply_target() {
    let roster = vec![
        PaneEntry::new(PaneId::Orch, PaneState::Live),
        PaneEntry::new(PaneId::Worker(1), PaneState::Dead),
        PaneEntry::new(PaneId::Worker(2), PaneState::Live),
        PaneEntry::new(PaneId::Worker(3), PaneState::Live),
    ];
    let (transport, _store, _writes) = start_hub(roster, PaneConfig::default()).await;
    let mut w2 = pane(&transport, PaneId::Worker(2)).await;
    let mut w1 = pane(&transport, PaneId::Worker(1)).await;

    // worker-2 → worker-1 fails (worker-1 is dead), so worker-1 owes no reply...
    w2.call(Op::PaneSend { to: PaneId::Worker(1), text: "you there?".into() }).await.unwrap();
    let result = w1.call(Op::PaneReply { text: "yes".into() }).await.unwrap();
    assert!(matches!(result, OpResult::Error { .. }), "got {result:?}");
}

/// The roster is the app's answer, not the config's: the hub knows which panes
/// *should* exist, only the pty registry knows which are alive.
#[tokio::test]
async fn roster_reports_live_state_from_the_app() {
    let roster = vec![
        PaneEntry::new(PaneId::Orch, PaneState::Live),
        PaneEntry::new(PaneId::Worker(1), PaneState::Live),
        PaneEntry::new(PaneId::Worker(2), PaneState::Spawning),
        PaneEntry::new(PaneId::Worker(3), PaneState::Dead),
    ];
    let (transport, _store, _writes) = start_hub(roster.clone(), PaneConfig::default()).await;
    let mut w1 = pane(&transport, PaneId::Worker(1)).await;

    let result = w1.call(Op::Roster).await.unwrap();
    assert_eq!(result, OpResult::Roster { panes: roster });
}

/// Pane identity comes from the `Hello`. A connection that never declared one is
/// refused rather than guessed at — a misattributed message is unrecoverable.
#[tokio::test]
async fn a_connection_without_a_pane_cannot_use_the_pane_surface() {
    let (transport, store, _writes) = start_hub(all_live(), PaneConfig::default()).await;
    let mut anon =
        Client::connect(transport.as_ref(), Hello::new(fleetor_core::Party::Lead)).await.unwrap();

    for op in [
        Op::PaneSend { to: PaneId::Worker(1), text: "x".into() },
        Op::PaneBroadcast { text: "x".into() },
        Op::PaneReply { text: "x".into() },
    ] {
        let result = anon.call(op.clone()).await.unwrap();
        assert!(
            matches!(&result, OpResult::Error { message } if message.contains("did not identify a pane")),
            "{op:?} gave {result:?}"
        );
    }
    assert!(messages(&store).is_empty());
}

/// A hub with no app attached must fail every send rather than logging deliveries
/// into the void. This is the state between "fleet started" and "panes spawned".
#[tokio::test]
async fn a_hub_with_no_app_rejects_instead_of_pretending() {
    let dir = std::env::temp_dir()
        .join(format!("fleetor-noapp-{}-{}", std::process::id(), fleetor_core::ids::new_id("t")));
    std::fs::create_dir_all(&dir).unwrap();
    let transport = Arc::new(UnixTransport::new(dir.join("fleet.sock")));
    let store = Arc::new(SqliteStore::open_in_memory().unwrap());
    let hub = Hub::new(
        store.clone(),
        HubConfig { slots: SLOTS.to_vec(), ask_timeout: Duration::from_secs(5) },
    );
    let listener = transport.bind().await.unwrap();
    tokio::spawn(hub.serve(listener));

    let mut orch = pane(&transport, PaneId::Orch).await;
    let result = orch.call(Op::PaneSend { to: PaneId::Worker(1), text: "x".into() }).await.unwrap();
    let OpResult::Delivered { accepted, detail, .. } = result else {
        panic!("expected a delivery, got {result:?}")
    };
    assert!(!accepted);
    assert!(detail.unwrap().contains("no fleet app"));
    assert_eq!(messages(&store).len(), 1, "the failed attempt is still on the record");
}

/// A wedged app cannot park the CLI forever: the ack has a ceiling, and blowing
/// through it is reported as a failure, not a success.
#[tokio::test]
async fn a_wedged_app_times_out_rather_than_parking_the_sender() {
    let (tx, mut rx) = mpsc::unbounded_channel::<AppCommand>();
    // Hold every command without ever answering it.
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Some(cmd) = rx.recv().await {
            held.push(cmd);
        }
    });

    let dir = std::env::temp_dir()
        .join(format!("fleetor-wedged-{}-{}", std::process::id(), fleetor_core::ids::new_id("t")));
    std::fs::create_dir_all(&dir).unwrap();
    let transport = Arc::new(UnixTransport::new(dir.join("fleet.sock")));
    let store = Arc::new(SqliteStore::open_in_memory().unwrap());
    let hub = Hub::with_app(
        store.clone(),
        HubConfig { slots: SLOTS.to_vec(), ask_timeout: Duration::from_secs(5) },
        PaneConfig { ack_timeout: Duration::from_millis(80), ..PaneConfig::default() },
        tx,
    );
    let listener = transport.bind().await.unwrap();
    tokio::spawn(hub.serve(listener));

    let mut orch = pane(&transport, PaneId::Orch).await;
    let result = orch.call(Op::PaneSend { to: PaneId::Worker(1), text: "x".into() }).await.unwrap();
    let OpResult::Delivered { accepted, detail, .. } = result else {
        panic!("expected a delivery, got {result:?}")
    };
    assert!(!accepted, "an unanswered delivery is not a delivery");
    assert!(detail.unwrap().contains("wedged"));
}
