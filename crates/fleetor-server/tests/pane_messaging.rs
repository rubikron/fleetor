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
use fleetor_server::{AppCommand, DeliveryResult, Hub};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
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
                // Commands land in the same transcript as messages, deliberately:
                // the assertion that matters is what *exact bytes* reached the
                // pty, and a separate list would let a framed command hide in it.
                AppCommand::Command { to, command, ack } => {
                    let result = match states.get(&to) {
                        Some(state) if state.accepts_input() => {
                            sink.lock().unwrap().push((to, command));
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

/// A hub wired to a fake app. The roster is the app's alone: after Phase 5 there
/// is no configured slot list to disagree with it, because a pane the app is not
/// running cannot be messaged and a pane it is running must be reachable.
async fn start_hub(roster: Vec<PaneEntry>) -> (Arc<UnixTransport>, Arc<SqliteStore>, Writes) {
    let dir = std::env::temp_dir().join(format!(
        "fleetor-pane-{}-{}",
        std::process::id(),
        fleetor_core::ids::new_id("t")
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let transport = Arc::new(UnixTransport::new(dir.join("fleet.sock")));
    let store = Arc::new(SqliteStore::open_in_memory().unwrap());
    let (app, writes) = spawn_app(roster);
    let hub = Hub::new(store.clone(), app);
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
    let (transport, store, writes) = start_hub(all_live()).await;
    let mut orch = pane(&transport, PaneId::Orch).await;

    let result = orch
        .call(Op::Send { to: PaneId::Worker(2), text: "take the parser".into() })
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
    let (transport, store, writes) = start_hub(roster).await;
    let mut orch = pane(&transport, PaneId::Orch).await;

    let dead = orch.call(Op::Send { to: PaneId::Worker(2), text: "ping".into() }).await.unwrap();
    let OpResult::Delivered { accepted, detail, .. } = dead else {
        panic!("expected a delivery, got {dead:?}");
    };
    assert!(!accepted, "worker-2 is dead");
    assert_eq!(detail.as_deref(), Some("pane worker-2 is dead"), "the reason reaches the sender");

    // Still booting is not a reason to refuse: gating on a guess about whether the
    // TUI has reached its prompt is how a healthy pane silently goes mute.
    let spawning =
        orch.call(Op::Send { to: PaneId::Worker(3), text: "ping".into() }).await.unwrap();
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
    let (transport, store, _writes) = start_hub(all_live()).await;
    let mut orch = pane(&transport, PaneId::Orch).await;

    let unknown = orch.call(Op::Send { to: PaneId::Worker(9), text: "x".into() }).await.unwrap();
    let OpResult::Delivered { accepted, detail, .. } = unknown else {
        panic!("expected a delivery, got {unknown:?}");
    };
    assert!(!accepted);
    assert_eq!(detail.as_deref(), Some("no pane worker-9 is running"));

    let selfsend = orch.call(Op::Send { to: PaneId::Orch, text: "x".into() }).await.unwrap();
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
    let (transport, _store, writes) = start_hub(roster).await;
    let mut orch = pane(&transport, PaneId::Orch).await;

    let result = orch.call(Op::Send { to: PaneId::Worker(7), text: "hi".into() }).await.unwrap();
    assert!(matches!(result, OpResult::Delivered { accepted: true, .. }), "got {result:?}");
    assert_eq!(writes.lock().unwrap().len(), 1);
}

/// One `broadcast` becomes N−1 legs that share a group id, and the sender is not
/// among them. The framing differs from a direct message so the receiver can obey
/// the do-not-answer-a-broadcast rule in its brief (L5).
#[tokio::test]
async fn a_broadcast_fans_out_to_every_other_pane_under_one_group() {
    let (transport, store, writes) = start_hub(all_live()).await;
    let mut w1 = pane(&transport, PaneId::Worker(1)).await;

    let result = w1.call(Op::Broadcast { text: "rebasing onto master".into() }).await.unwrap();
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
    let (transport, _store, writes) = start_hub(roster).await;
    let mut w1 = pane(&transport, PaneId::Worker(1)).await;

    let result = w1.call(Op::Broadcast { text: "status?".into() }).await.unwrap();
    let OpResult::Delivered { accepted, detail, .. } = result else {
        panic!("expected a delivery, got {result:?}");
    };
    assert!(!accepted, "a fan-out that missed two panes has not been accepted");
    let detail = detail.expect("a partial fan-out must say what missed");
    assert!(detail.contains("worker-2") && detail.contains("worker-3"), "{detail}");
    assert!(!detail.contains("orch"), "the pane that received it is not a failure: {detail}");
    assert_eq!(writes.lock().unwrap().len(), 1);
}

/// `reply` needs no addressee: it goes to whoever last got a message *through* to
/// this pane. This is the verb workers use for almost everything.
#[tokio::test]
async fn reply_routes_to_whoever_last_got_through() {
    let (transport, _store, writes) = start_hub(all_live()).await;
    let mut orch = pane(&transport, PaneId::Orch).await;
    let mut w1 = pane(&transport, PaneId::Worker(1)).await;
    let mut w2 = pane(&transport, PaneId::Worker(2)).await;

    orch.call(Op::Send { to: PaneId::Worker(2), text: "take the parser".into() }).await.unwrap();
    let replied = w2.call(Op::Reply { text: "on it".into() }).await.unwrap();
    assert!(matches!(replied, OpResult::Delivered { accepted: true, .. }), "got {replied:?}");

    // worker-1 now messages worker-2; worker-2's reply target moves to worker-1.
    w1.call(Op::Send { to: PaneId::Worker(2), text: "I own src/api".into() }).await.unwrap();
    w2.call(Op::Reply { text: "noted".into() }).await.unwrap();

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
    let (transport, store, _writes) = start_hub(all_live()).await;
    let mut w3 = pane(&transport, PaneId::Worker(3)).await;

    let result = w3.call(Op::Reply { text: "hello?".into() }).await.unwrap();
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
    let (transport, _store, _writes) = start_hub(roster).await;
    let mut w2 = pane(&transport, PaneId::Worker(2)).await;
    let mut w1 = pane(&transport, PaneId::Worker(1)).await;

    // worker-2 → worker-1 fails (worker-1 is dead), so worker-1 owes no reply...
    w2.call(Op::Send { to: PaneId::Worker(1), text: "you there?".into() }).await.unwrap();
    let result = w1.call(Op::Reply { text: "yes".into() }).await.unwrap();
    assert!(matches!(result, OpResult::Error { .. }), "got {result:?}");
}

/// The roster is the app's answer, not the config's: the hub knows which panes
/// *should* exist, only the pty registry knows which are alive.
///
/// The operator is the one row the app does not supply and the hub adds (WP-07)
/// — it is the *listing* of who a pane may address, and the human is
/// addressable without being a terminal. Every pane row is still exactly what
/// the app said, in the app's order.
#[tokio::test]
async fn roster_reports_live_state_from_the_app() {
    let roster = vec![
        PaneEntry::new(PaneId::Orch, PaneState::Live),
        PaneEntry::new(PaneId::Worker(1), PaneState::Live),
        PaneEntry::new(PaneId::Worker(2), PaneState::Spawning),
        PaneEntry::new(PaneId::Worker(3), PaneState::Dead),
    ];
    let (transport, _store, _writes) = start_hub(roster.clone()).await;
    let mut w1 = pane(&transport, PaneId::Worker(1)).await;

    let result = w1.call(Op::Roster).await.unwrap();
    let expected: Vec<PaneEntry> =
        std::iter::once(PaneEntry::new(PaneId::Operator, PaneState::Present))
            .chain(roster)
            .collect();
    assert_eq!(result, OpResult::Roster { panes: expected });
}

/// The human is on the roster with a word that is not a pane state, and it is
/// never `live`. A faked liveness would be the roster's own version of
/// rendering `accepted` for a message no pty received.
#[tokio::test]
async fn the_roster_lists_the_operator_as_present_and_never_as_live() {
    let (transport, _store, _writes) = start_hub(all_live()).await;
    let mut w1 = pane(&transport, PaneId::Worker(1)).await;

    let OpResult::Roster { panes } = w1.call(Op::Roster).await.unwrap() else {
        panic!("expected a roster");
    };
    let operator = panes.iter().find(|e| e.pane == PaneId::Operator).expect("the human is listed");
    assert_eq!(operator.state, PaneState::Present);
    assert!(!operator.state.is_live());
    assert_eq!(operator.context, None, "a human has no transcript to sample");
}

/// The fleet app going away — the window closed, its receiver dropped — must
/// fail every send rather than logging deliveries into the void.
///
/// The Phase-5 replacement for "a hub with no app": `Hub::new` now *requires* an
/// app, so the never-attached case is structurally impossible rather than merely
/// tested. Identity is the same story — `Hello` carries a `PaneId`, not an
/// `Option<PaneId>`, so an anonymous connection can no longer be constructed and
/// the test that checked it was refused has nothing left to check.
#[tokio::test]
async fn a_hub_whose_app_has_gone_rejects_instead_of_pretending() {
    let dir = std::env::temp_dir()
        .join(format!("fleetor-noapp-{}-{}", std::process::id(), fleetor_core::ids::new_id("t")));
    std::fs::create_dir_all(&dir).unwrap();
    let transport = Arc::new(UnixTransport::new(dir.join("fleet.sock")));
    let store = Arc::new(SqliteStore::open_in_memory().unwrap());

    // An app that is not listening: the sender survives, its receiver does not.
    let (app, rx) = mpsc::unbounded_channel::<AppCommand>();
    drop(rx);

    let hub = Hub::new(store.clone(), app);
    let listener = transport.bind().await.unwrap();
    tokio::spawn(hub.serve(listener));

    let mut orch = pane(&transport, PaneId::Orch).await;
    let result = orch.call(Op::Send { to: PaneId::Worker(1), text: "x".into() }).await.unwrap();
    let OpResult::Delivered { accepted, detail, .. } = result else {
        panic!("expected a delivery, got {result:?}")
    };
    assert!(!accepted);
    assert!(detail.unwrap().contains("not accepting commands"));
    assert_eq!(messages(&store).len(), 1, "the failed attempt is still on the record");
}

/// A broadcast leg must not become the recipient's reply target. It was not
/// addressed to them, and letting it overwrite `last_inbound_from` silently
/// redirects their next `fleet reply` to a pane that never spoke to them — the
/// one failure mode where a message arrives somewhere it was never meant to go.
#[tokio::test]
async fn a_broadcast_does_not_hijack_a_recipients_reply_target() {
    let (transport, _store, writes) = start_hub(all_live()).await;
    let mut orch = pane(&transport, PaneId::Orch).await;
    let mut w1 = pane(&transport, PaneId::Worker(1)).await;
    let mut w2 = pane(&transport, PaneId::Worker(2)).await;

    // orch opens a conversation with worker-2...
    orch.call(Op::Send { to: PaneId::Worker(2), text: "take the parser".into() }).await.unwrap();
    // ...then worker-1 broadcasts, reaching worker-2 among others.
    w1.call(Op::Broadcast { text: "rebasing onto master".into() }).await.unwrap();

    // worker-2's reply must still go to orch, not to worker-1.
    let replied = w2.call(Op::Reply { text: "on it".into() }).await.unwrap();
    assert!(matches!(replied, OpResult::Delivered { accepted: true, .. }), "got {replied:?}");

    let last = writes.lock().unwrap().last().cloned().unwrap();
    assert_eq!(
        last,
        (PaneId::Orch, "[fleet · worker-2] on it".to_string()),
        "the reply went to the broadcaster instead of the pane that addressed it"
    );
}

// --- the command channel (D-045) ---------------------------------------------

/// Every `Command` in the log, oldest first.
fn commands(store: &SqliteStore) -> Vec<FleetEvent> {
    store
        .events_since(0)
        .unwrap()
        .into_iter()
        .map(|(_, e)| e)
        .filter(|e| matches!(e, FleetEvent::Command { .. }))
        .collect()
}

/// The happy path, and the byte-level claim the whole package rests on: what
/// reaches the pty is the command **unframed**, so its `/` is the first character
/// the input box sees. A `[fleet · orch] /compact …` would be prose about a
/// command (`docs/command-channel-notes.md` §3).
#[tokio::test]
async fn a_command_reaches_the_pty_unframed_and_the_why_lands_in_the_log() {
    let (transport, store, writes) = start_hub(all_live()).await;
    let mut orch = pane(&transport, PaneId::Orch).await;

    let result = orch
        .call(Op::Cmd {
            to: PaneId::Worker(2),
            command: "/compact keep the parser design".into(),
            why: "worker-2 finished the parser; the exploration before it is dead weight".into(),
        })
        .await
        .unwrap();
    let OpResult::Delivered { msg_id, accepted, detail } = result else {
        panic!("expected a delivery, got {result:?}");
    };
    assert!(accepted);
    assert_eq!(detail, None);

    assert_eq!(
        writes.lock().unwrap().clone(),
        vec![(PaneId::Worker(2), "/compact keep the parser design".to_string())],
        "the command must arrive with no `[fleet · …]` prefix and nothing else prepended"
    );

    let logged = commands(&store);
    assert_eq!(logged.len(), 1);
    let FleetEvent::Command { id, from, to, command, why, accepted, detail } = &logged[0] else {
        panic!("expected a command event");
    };
    assert_eq!(id, &msg_id, "the id the sender got back is the id in the log");
    assert_eq!((*from, *to), (PaneId::Orch, PaneId::Worker(2)));
    assert_eq!(command, "/compact keep the parser design");
    assert!(why.starts_with("worker-2 finished"), "the reasoning chain is on the record: {why}");
    assert!(*accepted);
    assert_eq!(*detail, None);
    assert!(messages(&store).is_empty(), "a command is not a message row");
}

/// Self-targeting is allowed **for `cmd` only** — a worker compacting its own
/// context is the self-maintenance move the brief teaches. The message self-send
/// guard is untouched and still refuses, in the same test so the two can never
/// quietly converge.
#[tokio::test]
async fn a_pane_may_command_itself_while_a_self_send_is_still_refused() {
    let (transport, store, writes) = start_hub(all_live()).await;
    let mut w2 = pane(&transport, PaneId::Worker(2)).await;

    let commanded = w2
        .call(Op::Cmd {
            to: PaneId::Worker(2),
            command: "/compact keep T-4 and the decisions".into(),
            why: "finished task block 3".into(),
        })
        .await
        .unwrap();
    assert!(
        matches!(commanded, OpResult::Delivered { accepted: true, .. }),
        "a pane must be able to compact itself: {commanded:?}"
    );

    let selfsend = w2.call(Op::Send { to: PaneId::Worker(2), text: "x".into() }).await.unwrap();
    assert!(
        matches!(&selfsend, OpResult::Error { message } if message.contains("cannot message itself")),
        "the message self-send guard must be byte-identical: {selfsend:?}"
    );

    assert_eq!(
        writes.lock().unwrap().clone(),
        vec![(PaneId::Worker(2), "/compact keep T-4 and the decisions".to_string())]
    );
    assert_eq!(commands(&store).len(), 1);
    assert!(messages(&store).is_empty(), "the self-send is not even logged");
}

/// Refusal happens **at accept time, against a constant**, and nothing enters the
/// delivery path. Nothing is written, nothing is logged — the same shape as the
/// self-send refusal, which is the class D-034 explicitly keeps.
#[tokio::test]
async fn a_command_outside_the_allowlist_is_refused_before_it_reaches_a_pty() {
    let (transport, store, writes) = start_hub(all_live()).await;
    let mut orch = pane(&transport, PaneId::Orch).await;

    for (command, why, expect) in [
        ("/model opus", "cheaper", "/compact"),
        ("/exit", "done", "/compact"),
        ("please compact yourself", "stale", "fleet send"),
        ("/clear\u{1b}[201~/exit", "hostile", "control character"),
        ("/clear", "   ", "--why"),
    ] {
        let result = orch
            .call(Op::Cmd { to: PaneId::Worker(1), command: command.into(), why: why.into() })
            .await
            .unwrap();
        let OpResult::Error { message } = result else {
            panic!("{command:?} must be refused, got {result:?}");
        };
        assert!(message.contains(expect), "{command:?}: {message}");
    }

    assert!(writes.lock().unwrap().is_empty(), "a refused command must never reach a pty");
    assert!(commands(&store).is_empty(), "a refusal at accept time is not an event");
}

/// A command to a dead pane is refused by the *app*, in its words, and is still
/// logged with `accepted: false` — an invisible failure is worse than a loud one
/// (L3). This is the other refusal: accepted by the hub, rejected by the pty.
#[tokio::test]
async fn a_command_to_a_dead_pane_is_logged_as_not_accepted() {
    let roster = vec![
        PaneEntry::new(PaneId::Orch, PaneState::Live),
        PaneEntry::new(PaneId::Worker(1), PaneState::Dead),
    ];
    let (transport, store, _writes) = start_hub(roster).await;
    let mut orch = pane(&transport, PaneId::Orch).await;

    let result = orch
        .call(Op::Cmd {
            to: PaneId::Worker(1),
            command: "/clear".into(),
            why: "the task changed completely".into(),
        })
        .await
        .unwrap();
    let OpResult::Delivered { accepted, detail, .. } = result else {
        panic!("expected a delivery, got {result:?}");
    };
    assert!(!accepted);
    assert_eq!(detail.as_deref(), Some("pane worker-1 is dead"));

    let logged = commands(&store);
    assert_eq!(logged.len(), 1, "the attempt is on the record");
    let FleetEvent::Command { accepted, why, .. } = &logged[0] else { panic!("not a command") };
    assert!(!*accepted);
    assert_eq!(why, "the task changed completely", "the why survives a failed write");
}

/// A command must not become a reply target. `fleet reply` answers whoever *said*
/// something to you, and nobody said anything — a `/compact` that redirected the
/// receiver's next reply would send it to a pane that never spoke to them.
#[tokio::test]
async fn a_command_does_not_become_the_targets_reply_target() {
    let (transport, _store, writes) = start_hub(all_live()).await;
    let mut orch = pane(&transport, PaneId::Orch).await;
    let mut w1 = pane(&transport, PaneId::Worker(1)).await;
    let mut w2 = pane(&transport, PaneId::Worker(2)).await;

    // worker-1 opens a conversation with worker-2...
    w1.call(Op::Send { to: PaneId::Worker(2), text: "I own src/api".into() }).await.unwrap();
    // ...then orch compacts worker-2, which is not a thing orch said to them.
    orch.call(Op::Cmd {
        to: PaneId::Worker(2),
        command: "/compact keep the api ownership".into(),
        why: "worker-2's context is mostly stale exploration".into(),
    })
    .await
    .unwrap();

    let replied = w2.call(Op::Reply { text: "noted".into() }).await.unwrap();
    assert!(matches!(replied, OpResult::Delivered { accepted: true, .. }), "got {replied:?}");
    assert_eq!(
        writes.lock().unwrap().last().cloned().unwrap(),
        (PaneId::Worker(1), "[fleet · worker-2] noted".to_string()),
        "the reply went to the pane that ran a command instead of the one that spoke"
    );
}

/// A pane that has only ever received a broadcast has nobody to reply to — a
/// fan-out is not a conversation.
#[tokio::test]
async fn a_broadcast_alone_gives_its_recipients_nobody_to_reply_to() {
    let (transport, _store, _writes) = start_hub(all_live()).await;
    let mut w1 = pane(&transport, PaneId::Worker(1)).await;
    let mut w3 = pane(&transport, PaneId::Worker(3)).await;

    w1.call(Op::Broadcast { text: "status?".into() }).await.unwrap();
    let result = w3.call(Op::Reply { text: "fine".into() }).await.unwrap();
    assert!(
        matches!(&result, OpResult::Error { message } if message.contains("nobody has messaged worker-3")),
        "got {result:?}"
    );
}

// --- the operator as a participant (WP-07) -----------------------------------
//
// The human joins the record. Everything below is the *whole* routing diff: one
// branch in `Hub::deliver` for the one addressee with no pty, and one extra row
// on the roster listing. The pane↔pane assertions above are unchanged and are
// the proof of the other half — this section adds one more that says so out loud.

/// The new arm. A message to the human never touches the app, comes back
/// `recorded`, and lands in the log with its body — which is the whole of what
/// the inbox reads back.
#[tokio::test]
async fn a_message_to_the_operator_is_recorded_and_no_pty_is_written_to() {
    let (transport, store, writes) = start_hub(all_live()).await;
    let mut w2 = pane(&transport, PaneId::Worker(2)).await;

    let result = w2
        .call(Op::Send {
            to: PaneId::Operator,
            text: "the block says \"one grammar\" — did you mean the tokenizer too?".into(),
        })
        .await
        .unwrap();

    let OpResult::Recorded { record_id } = result else {
        panic!("a message to the human is recorded, never delivered: {result:?}");
    };
    assert!(record_id.starts_with("msg-"), "the id names the message: {record_id}");
    assert!(writes.lock().unwrap().is_empty(), "there is no terminal to type into");

    let logged = messages(&store);
    assert_eq!(logged.len(), 1);
    let (from, to, body, group, accepted, detail) = as_message(&logged[0]);
    assert_eq!((from, to), (PaneId::Worker(2), PaneId::Operator));
    assert!(body.contains("one grammar"), "the question is in the record: {body}");
    assert_eq!(group, None);
    assert!(!accepted, "`accepted` means bytes reached a live pty, and none did");
    assert_eq!(detail, None, "nothing failed, so nothing invents a reason");
}

/// The other direction rides the path that already existed, byte for byte: the
/// human's name comes out of `Display`, the framing out of `frame_for_pane`,
/// and the answer is the ordinary `accepted` about a real terminal.
#[tokio::test]
async fn the_operator_writes_into_a_pane_through_the_unchanged_delivery_path() {
    let (transport, store, writes) = start_hub(all_live()).await;
    let mut operator = pane(&transport, PaneId::Operator).await;

    let result = operator
        .call(Op::Send { to: PaneId::Worker(2), text: "ship the parser, skip the CLI".into() })
        .await
        .unwrap();
    assert!(matches!(result, OpResult::Delivered { accepted: true, .. }), "got {result:?}");
    assert_eq!(
        writes.lock().unwrap().clone(),
        vec![(PaneId::Worker(2), "[fleet · operator] ship the parser, skip the CLI".to_string())],
    );

    let (from, to, _, _, accepted, _) = as_message(&messages(&store)[0]);
    assert_eq!((from, to), (PaneId::Operator, PaneId::Worker(2)));
    assert!(accepted, "a live pty took the bytes — the same word it always meant");
}

/// An operator broadcast is a broadcast. It wears the `→ all` framing every
/// worker's anti-amplification clause keys on, so "never reply to a broadcast
/// unless it names you" binds to the human's fan-out exactly as it binds to a
/// pane's — a human shouting at five terminals is the worst case that rule
/// exists for.
#[tokio::test]
async fn an_operator_broadcast_wears_the_all_framing_the_reply_rule_binds_to() {
    let (transport, store, writes) = start_hub(all_live()).await;
    let mut operator = pane(&transport, PaneId::Operator).await;

    let result = operator.call(Op::Broadcast { text: "stop and read the vision doc".into() }).await.unwrap();
    let OpResult::Delivered { msg_id: group, accepted, .. } = result else {
        panic!("expected a delivery, got {result:?}");
    };
    assert!(accepted, "every leg landed");

    let sent = writes.lock().unwrap().clone();
    assert_eq!(sent.len(), 4, "orch and three workers — the sender is never a target");
    for (_, text) in &sent {
        assert!(
            text.starts_with("[fleet · operator → all] "),
            "a leg must be visibly a broadcast: {text}",
        );
    }

    let logged = messages(&store);
    assert_eq!(logged.len(), 4);
    for event in &logged {
        assert_eq!(as_message(event).3, Some(group.as_str()), "one gesture, one group");
    }
}

/// `fleet reply` in both directions across the human. A worker's question makes
/// the operator's reply resolve back to that worker; the reply then makes the
/// operator the worker's own reply target. Nothing about `last_inbound_from`
/// is special-cased — it is keyed by name, and the human has one.
#[tokio::test]
async fn reply_resolves_in_both_directions_across_the_operator() {
    let (transport, _store, writes) = start_hub(all_live()).await;
    let mut w3 = pane(&transport, PaneId::Worker(3)).await;
    let mut operator = pane(&transport, PaneId::Operator).await;

    w3.call(Op::Send { to: PaneId::Operator, text: "which schema?".into() }).await.unwrap();

    let answered = operator.call(Op::Reply { text: "the v2 one".into() }).await.unwrap();
    assert!(matches!(answered, OpResult::Delivered { accepted: true, .. }), "got {answered:?}");
    assert_eq!(
        writes.lock().unwrap().clone(),
        vec![(PaneId::Worker(3), "[fleet · operator] the v2 one".to_string())],
        "the answer went back to the worker that asked, with no target typed",
    );

    // ...and now the worker can answer the human without naming them either.
    let back = w3.call(Op::Reply { text: "v2 it is".into() }).await.unwrap();
    assert!(
        matches!(back, OpResult::Recorded { .. }),
        "a reply to the human is recorded like any other message to them: {back:?}",
    );
    assert_eq!(writes.lock().unwrap().len(), 1, "the reply to a human reaches no terminal");
}

/// The self-send guard needed no operator case and must not have grown one: a
/// participant messaging itself is nonsense whoever they are.
#[tokio::test]
async fn the_operator_cannot_message_itself_either() {
    let (transport, store, _writes) = start_hub(all_live()).await;
    let mut operator = pane(&transport, PaneId::Operator).await;

    let result = operator.call(Op::Send { to: PaneId::Operator, text: "note to self".into() }).await.unwrap();
    assert!(
        matches!(&result, OpResult::Error { message } if message.contains("cannot message itself")),
        "got {result:?}",
    );
    assert!(messages(&store).is_empty(), "a refused self-send is not a log entry");
}

/// **The message path for pane↔pane traffic is untouched.** A fan-out still
/// reaches exactly the terminals the app is running: adding the operator to the
/// broadcast target list would have asked for a pty that does not exist, come
/// back refused, and made every `fleet broadcast` in the fleet report a partial
/// failure. The human is addressed by name.
#[tokio::test]
async fn a_pane_broadcast_still_reaches_only_the_terminals_the_app_runs() {
    let (transport, store, writes) = start_hub(all_live()).await;
    let mut w1 = pane(&transport, PaneId::Worker(1)).await;

    let result = w1.call(Op::Broadcast { text: "rebasing".into() }).await.unwrap();
    assert!(
        matches!(result, OpResult::Delivered { accepted: true, .. }),
        "a fan-out must not be dragged to `accepted: false` by an addressee with no pty: {result:?}",
    );

    let targets: Vec<PaneId> = writes.lock().unwrap().iter().map(|(pane, _)| *pane).collect();
    assert_eq!(targets, vec![PaneId::Orch, PaneId::Worker(2), PaneId::Worker(3)]);
    assert!(
        !messages(&store).iter().any(|e| as_message(e).1 == PaneId::Operator),
        "no leg was addressed to the human",
    );
}

/// `fleet cmd` is about a terminal, so it is refused at accept time for the one
/// name that has none — before anything enters the delivery path, the same
/// class of refusal as the allowlist. Letting it through to be rejected by the
/// registry would describe the human as a crashed pane.
#[tokio::test]
async fn a_slash_command_aimed_at_the_operator_is_refused_rather_than_attempted() {
    let (transport, store, writes) = start_hub(all_live()).await;
    let mut orch = pane(&transport, PaneId::Orch).await;

    let result = orch
        .call(Op::Cmd {
            to: PaneId::Operator,
            command: "/clear".into(),
            why: "trying to reset the human".into(),
        })
        .await
        .unwrap();
    assert!(
        matches!(&result, OpResult::Error { message }
            if message.contains("operator") && message.contains("no terminal")),
        "got {result:?}",
    );
    assert!(writes.lock().unwrap().is_empty());
    assert!(
        store.events_since(0).unwrap().is_empty(),
        "a refusal before the delivery path is not a log entry",
    );
}
