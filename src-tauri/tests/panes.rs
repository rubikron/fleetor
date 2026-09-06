//! Five real ptys, no window, no tokens (D-030, Phase 3).
//!
//! This is the test the pane registry exists to pass. Everything below drives
//! `PaneRegistry` directly against `tests/fake-pane/fake-pane.sh` — a real
//! process on the far end of a real pty, selected through the same
//! `FLEETOR_PANE_CMD` hook the spawn path honors — so the parts that can only
//! fail at runtime (channel isolation, injection, process teardown) fail here
//! rather than in front of an operator.
//!
//! What it deliberately does **not** cover: whether a live `claude` reaches its
//! prompt. That is L1/L2, and it was settled by measurement in Phase 0
//! (`docs/notes/tui-spawn-notes.md`), not by a test — a fake pane would answer the
//! question wrong in the reassuring direction.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD, Engine};
use fleetor_core::event::FleetEvent;
use fleetor_core::message::frame_for_pane;
use fleetor_core::pane::{PaneId, WORKER_SLOTS};
use fleetor_core::wire::{Op, OpResult};
use fleetor_core::Store;
use fleetor_db::SqliteStore;
use fleetor_shell::context_gauge::GaugeSources;
use fleetor_shell::deliver::spawn_delivery;
use fleetor_shell::pty::{exit_channel, out_channel, Emit, PaneRegistry};
use fleetor_shell::placement::spawn;
use fleetor_shell::placement::{self, Host, Layout, PaneSpec, RunSource};
use fleetor_shell::prompts::PaneContext;
use fleetor_server::{AppCommand, Hub};
use portable_pty::CommandBuilder;
use tokio::sync::{mpsc, oneshot};

/// A fresh in-memory store for the tests that only need `spawn_delivery`'s
/// signature satisfied, not the event log itself.
fn scratch_store() -> Arc<dyn Store> {
    Arc::new(SqliteStore::open_in_memory().unwrap())
}

/// Long enough for a shell to start and echo on a loaded machine; short enough
/// that a genuine failure doesn't look like a hang.
const PATIENCE: Duration = Duration::from_secs(10);

/// Everything the registry has emitted, per channel, decoded back to text.
#[derive(Default)]
struct Transcript(Mutex<HashMap<String, String>>);

impl Transcript {
    fn emitter(self: &Arc<Self>) -> Emit {
        let sink = self.clone();
        Arc::new(move |channel: &str, payload: String| {
            let bytes = STANDARD.decode(&payload).unwrap_or_default();
            let mut map = sink.0.lock().unwrap();
            map.entry(channel.to_string())
                .or_default()
                .push_str(&String::from_utf8_lossy(&bytes));
        })
    }

    fn text(&self, channel: &str) -> String {
        self.0.lock().unwrap().get(channel).cloned().unwrap_or_default()
    }

    /// Wait for a channel to be emitted on at all. Distinct from
    /// [`Transcript::wait_for`] because the exit channel carries an empty payload
    /// — its whole content is the fact that it fired.
    fn wait_until_fired(&self, channel: &str) {
        let deadline = Instant::now() + PATIENCE;
        while Instant::now() < deadline {
            if self.0.lock().unwrap().contains_key(channel) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("{channel} never fired");
    }

    /// Wait for `needle` on `channel`, or fail with what did arrive. Polling
    /// rather than a fixed sleep: a pty's timing is the machine's business.
    fn wait_for(&self, channel: &str, needle: &str) -> String {
        let deadline = Instant::now() + PATIENCE;
        while Instant::now() < deadline {
            let text = self.text(channel);
            if text.contains(needle) {
                return text;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("{channel} never carried {needle:?}; it carried: {:?}", self.text(channel));
    }
}

fn fake_pane_script() -> PathBuf {
    // `CARGO_MANIFEST_DIR` is `src-tauri/`; the script is a sibling of it.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fake-pane/fake-pane.sh")
}

/// A pid-registry path this test alone owns, never the operator's real
/// `~/.fleetor/_shell/panes.pids` — unique per call so concurrently-running
/// tests (each on its own thread) never share, and corrupt, one another's file.
fn scratch_registry_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "fleetor-panes-registry-{}-{:?}.pids",
        std::process::id(),
        std::thread::current().id()
    ))
}

/// A scratch `~/.fleetor` and the repo this binary points a fleet at — one pair
/// per test thread, so tests running in parallel never seed each other's
/// directories.
///
/// The target is deliberately **not** a git repository: a worker placed against it
/// takes the announced fallback to the shared checkout, which is exactly what this
/// file wants — a real cwd, nothing branched, and no side effect on the repository
/// the tests are running inside.
fn scratch_installation() -> (Layout, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "fleetor-panes-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let layout = Layout::under(root.join("state"));
    let target = root.join("target");
    std::fs::create_dir_all(&target).unwrap();
    (layout, target)
}

/// The machine this binary describes: the stand-in pane program, a worker key,
/// and the real inherited `PATH` — the fake pane's `#!/usr/bin/env bash` has to
/// find a `bash`, so that one field must be the truth rather than a scratch value.
fn stand_in_host() -> Host {
    Host {
        pane_program: Some(fake_pane_script().display().to_string()),
        api_key: Some("sk-test".to_string()),
        inherited_path: std::env::var("PATH").unwrap_or_default(),
        ..Host::bare()
    }
}

/// Bring up a registry with every pane running the stand-in, **through
/// `placement::place`** — the identical call `fleet::spawn_pane` makes, so the
/// seeding, the guardrail install and the command's environment work are all
/// exercised on the way to a real pty (WP-21, D-075).
///
/// It used to call `spawn.rs`'s command builders directly, and it was the last
/// thing outside `placement` that knew how a pane is shaped. Those builders are
/// internals of that module now, so the stand-in arrives as a [`Host`] field
/// rather than through `FLEETOR_PANE_CMD` — the same override, handed in instead
/// of read.
fn fleet() -> (Arc<PaneRegistry>, Arc<Transcript>) {
    let (registry, transcript, _registry_path) = fleet_with_registry_path();
    (registry, transcript)
}

/// [`fleet`], plus the scratch pid-registry path it used — for the one test
/// that needs to read that file back and check what landed in it.
fn fleet_with_registry_path() -> (Arc<PaneRegistry>, Arc<Transcript>, PathBuf) {
    let transcript = Arc::new(Transcript::default());
    let registry_path = scratch_registry_path();
    let registry = Arc::new(PaneRegistry::new(transcript.emitter(), registry_path.clone()));

    let (layout, target) = scratch_installation();
    let host = stand_in_host();
    let ctx = PaneContext::baked();

    let mut specs = vec![PaneSpec::Orch];
    specs.extend(WORKER_SLOTS.iter().map(|slot| PaneSpec::Worker(*slot)));
    for spec in specs {
        let pane = spec.pane();
        let placed = placement::place(spec, &layout, &host, &target, &ctx)
            .unwrap_or_else(|e| panic!("placing {pane}: {e}"));
        registry.spawn(pane, placed.command, 24, 80).unwrap();
    }
    (registry, transcript, registry_path)
}

/// The reason per-pane channels exist. Five panes announce themselves; each
/// announcement must appear on exactly one channel. A shared channel with an id
/// in the payload would pass a "did it arrive" test and fail this one.
#[test]
fn every_pane_speaks_only_on_its_own_channel() {
    let (registry, transcript) = fleet();
    let roster = PaneId::roster(&WORKER_SLOTS);

    for pane in &roster {
        transcript.wait_for(&out_channel(*pane), &format!("fake-pane {pane} ready"));
    }
    for pane in &roster {
        for other in &roster {
            if pane == other {
                continue;
            }
            assert!(
                !transcript.text(&out_channel(*other)).contains(&format!("fake-pane {pane} ready")),
                "{other}'s channel carried {pane}'s output"
            );
        }
    }
    assert_eq!(registry.roster().len(), 5, "every pane is on the roster");
    registry.kill_all();
}

/// The product, minus the socket: a framed message typed into one pane's pty,
/// submitted, and read by the process on the far end — while its four peers hear
/// nothing.
#[test]
fn a_delivered_message_lands_in_its_target_pane_and_nowhere_else() {
    let (registry, transcript) = fleet();
    let target = PaneId::Worker(2);
    transcript.wait_for(&out_channel(target), "ready");

    let framed = frame_for_pane(PaneId::Orch, "take the parser");
    registry.write_paste(target, &framed).expect("a live pane accepts a message");

    // "echo:" is the stand-in's proof it read a *submitted* line — the `\r` after
    // the closing paste marker did its job, rather than leaving the text sitting
    // unsent in an input box.
    let seen = transcript.wait_for(&out_channel(target), "echo:");
    assert!(seen.contains("take the parser"), "the body arrived intact: {seen:?}");

    for other in PaneId::roster(&WORKER_SLOTS).into_iter().filter(|p| *p != target) {
        assert!(
            !transcript.text(&out_channel(other)).contains("take the parser"),
            "{other} received a message addressed to {target}"
        );
    }
    registry.kill_all();
}

/// **The command channel against a real pty (D-045).** A `fleet cmd` must reach
/// the far end as its own submitted line, with the command's `/` as the first
/// character after the paste marker — no `[fleet · …]`, nothing joined onto it.
///
/// Sent as a burst with two messages so the writer has something to batch: the
/// messages join, the command does not, and the assertion is on the exact bytes
/// the pane's tty echoed back. A fake pane cannot prove Claude Code *executes*
/// it — that is `docs/notes/command-channel-notes.md`, measured against the real
/// binary — but it is exactly the right thing to prove what we typed.
#[test]
fn a_command_reaches_a_real_pty_unframed_and_in_a_write_of_its_own() {
    let (registry, transcript) = fleet();
    let target = PaneId::Worker(2);
    transcript.wait_for(&out_channel(target), "ready");

    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let (tx, rx) = mpsc::unbounded_channel::<AppCommand>();
    spawn_delivery(&rt, registry.clone(), rx, scratch_store(), Arc::new(GaugeSources::default()));

    let (first_ack, first) = oneshot::channel();
    tx.send(AppCommand::Deliver {
        to: target,
        text: frame_for_pane(PaneId::Orch, "wrap up the parser"),
        ack: first_ack,
    })
    .unwrap();
    let (cmd_ack, commanded) = oneshot::channel();
    tx.send(AppCommand::Command {
        to: target,
        command: "/compact keep the parser".into(),
        ack: cmd_ack,
    })
    .unwrap();
    let (last_ack, last) = oneshot::channel();
    tx.send(AppCommand::Deliver {
        to: target,
        text: frame_for_pane(PaneId::Orch, "now take the CLI"),
        ack: last_ack,
    })
    .unwrap();

    for (label, answer) in [("message", first), ("command", commanded), ("re-brief", last)] {
        let result = rt.block_on(answer).unwrap_or_else(|_| panic!("{label} went unanswered"));
        assert!(result.accepted, "{label} was refused: {result:?}");
    }

    // The stand-in is a plain shell, so it echoes the bracketed-paste markers
    // back as ordinary characters — which is what makes the boundary visible.
    let seen = transcript.wait_for(&out_channel(target), "now take the CLI");
    assert!(
        seen.contains("echo: \u{1b}[200~/compact keep the parser\u{1b}[201~"),
        "the command must be its own write, with `/` first after the paste marker: {seen:?}"
    );
    assert!(
        !seen.contains("[fleet · orch] /compact"),
        "the command was framed like a message: {seen:?}"
    );
    // Ordering is the hub's: the re-brief a cleared worker needs cannot overtake
    // the command it is meant to follow.
    let at = |needle: &str| seen.find(needle).unwrap_or_else(|| panic!("missing {needle:?}"));
    assert!(
        at("wrap up the parser") < at("/compact keep the parser")
            && at("/compact keep the parser") < at("now take the CLI"),
        "the pane saw the burst out of the order the hub took it: {seen:?}"
    );

    registry.kill_all();
}

/// Operator keystrokes and an injected message share one writer lock, so they
/// cannot interleave. Both must still get through.
#[test]
fn keystrokes_and_deliveries_share_a_pane_without_losing_either() {
    let (registry, transcript) = fleet();
    let target = PaneId::Worker(1);
    transcript.wait_for(&out_channel(target), "ready");

    registry.write(target, b"typed-by-hand\r").unwrap();
    transcript.wait_for(&out_channel(target), "echo: typed-by-hand");

    // The stand-in is a plain shell, not a TUI, so it reads the bracketed-paste
    // markers as ordinary characters and echoes them back. A real `claude`
    // consumes them — Phase 0 verified that against the actual binary. What this
    // asserts is the part a fake *can* prove: the injection reached the far end
    // as one submitted line, after a keystroke, on the same writer.
    registry.write_paste(target, "injected-by-fleet").unwrap();
    transcript.wait_for(&out_channel(target), "echo: \u{1b}[200~injected-by-fleet\u{1b}[201~");

    registry.kill_all();
}

/// A pane that has exited must **refuse**, naming itself, so the model reading
/// its own stderr knows the message went nowhere. Silence here parks `fleet send`
/// forever; a false success is worse still.
#[test]
fn a_dead_pane_refuses_rather_than_swallowing_a_message() {
    let (registry, transcript) = fleet();
    let target = PaneId::Worker(3);
    transcript.wait_for(&out_channel(target), "ready");

    registry.kill(target).unwrap();
    transcript.wait_until_fired(&exit_channel(target));

    let error = registry.write_paste(target, "anyone there?").expect_err("a dead pane cannot accept");
    assert!(error.contains("worker-3"), "the refusal must name the pane: {error}");

    // Its peers are untouched — killing one tab does not take the fleet down.
    assert!(registry.roster().iter().any(|e| e.pane == PaneId::Worker(4)));
    registry.kill_all();
}

/// A killed pane's tab comes back. This is the per-tab restart Phase 4 offers as
/// the escape hatch when a worker wedges, and it is why `spawn` respawns a dead
/// pane rather than treating its presence in the map as "already running".
#[test]
fn a_killed_pane_can_be_spawned_again() {
    let (registry, transcript) = fleet();
    let target = PaneId::Worker(4);
    transcript.wait_for(&out_channel(target), "ready");
    registry.kill(target).unwrap();

    let (layout, repo) = scratch_installation();
    let placed = placement::place(
        PaneSpec::Worker(4),
        &layout,
        &stand_in_host(),
        &repo,
        &PaneContext::baked(),
    )
    .expect("worker-4 places");
    registry.spawn(target, placed.command, 24, 80).expect("a dead pane respawns");

    registry.write_paste(target, "still here?").unwrap();
    transcript.wait_for(&out_channel(target), "echo:");
    registry.kill_all();
}

/// Every pane reaped on close. A leaked pane is a process with no terminal and
/// nobody watching — for an Opus session, a bill that keeps growing after the
/// window is gone.
#[test]
fn kill_all_reaps_the_whole_fleet() {
    let (registry, transcript) = fleet();
    for pane in PaneId::roster(&WORKER_SLOTS) {
        transcript.wait_for(&out_channel(pane), "ready");
    }

    registry.kill_all();

    assert!(registry.roster().is_empty(), "nothing survives the teardown");
    for pane in PaneId::roster(&WORKER_SLOTS) {
        assert!(
            registry.write_paste(pane, "hello?").is_err(),
            "{pane} still accepted input after kill_all"
        );
    }
}

/// The wiring `orphans.rs`'s own tests cannot see: `write_registry` is only as
/// good as the caller that feeds it, and nothing in `orphans::tests` proves
/// `spawn`/`kill`/`kill_all` actually call it, with the right pids, at the
/// right time. This drives the real `PaneRegistry` and reads the on-disk
/// registry back after each mutation, so a break in that wiring — pids never
/// written, a kill that leaves a stale entry, `kill_all` that leaves the file
/// non-empty — fails here instead of only surfacing as an orphan sweep that
/// quietly does nothing on the next launch.
#[cfg(unix)]
#[test]
fn spawn_kill_and_kill_all_keep_the_pid_registry_in_sync_with_reality() {
    let (registry, transcript, registry_path) = fleet_with_registry_path();
    for pane in PaneId::roster(&WORKER_SLOTS) {
        transcript.wait_for(&out_channel(pane), "ready");
    }

    let pids = read_registry(&registry_path);
    assert_eq!(pids.len(), 5, "every spawned pane is recorded: {pids:?}");
    for pid in &pids {
        assert!(process_alive(*pid as i32), "recorded pid {pid} is not actually running");
    }

    registry.kill(PaneId::Worker(2)).unwrap();
    let after_one_kill = read_registry(&registry_path);
    assert_eq!(after_one_kill.len(), 4, "the killed pane's pid drops out: {after_one_kill:?}");

    registry.kill_all();
    let after_kill_all = read_registry(&registry_path);
    assert!(after_kill_all.is_empty(), "kill_all clears the registry: {after_kill_all:?}");

    let _ = std::fs::remove_file(&registry_path);
}

#[cfg(unix)]
fn read_registry(path: &std::path::Path) -> Vec<u32> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}

/// **The money-bug regression.** `terminate` SIGTERMs the pane's whole *process
/// group*, not just the shell `portable-pty` handed back — because a real
/// `claude` spawns children of its own that share its pgid, and killing the pid
/// alone leaves them running with no terminal and nobody watching (pty.rs:
/// "A leaked Opus after window close is a money bug"). `kill_all_reaps_the_whole_
/// fleet` only proves the *registry* forgets the pane; it never asked the OS
/// whether anything survived. This spawns a background job inside the pane's own
/// shell — the same relationship a `claude` process has to *its* children — and
/// checks the OS process table directly, so a future change that narrows the
/// kill back down to a single pid fails here instead of in front of an operator.
#[cfg(unix)]
#[test]
fn kill_all_reaps_a_panes_background_grandchild_too() {
    let transcript = Arc::new(Transcript::default());
    let registry = Arc::new(PaneRegistry::new(transcript.emitter(), scratch_registry_path()));

    let mut cmd = CommandBuilder::new("bash");
    cmd.arg("-c");
    cmd.arg(
        "sleep 300 & echo \"grandchild-pid:$!\"; echo ready; \
         while IFS= read -r line; do echo \"echo: $line\"; done",
    );

    let pane = PaneId::Worker(1);
    registry.spawn(pane, cmd, 24, 80).unwrap();

    let seen = transcript.wait_for(&out_channel(pane), "grandchild-pid:");
    let pid: i32 = seen
        .lines()
        .find_map(|line| line.strip_prefix("grandchild-pid:"))
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or_else(|| panic!("no grandchild pid in pty output: {seen:?}"));

    assert!(process_alive(pid), "grandchild {pid} must be running before teardown");

    registry.kill_all();

    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline && process_alive(pid) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !process_alive(pid),
        "kill_all left the pane's background grandchild (pid {pid}) running — \
         a leaked Opus after window close is a money bug"
    );
}

/// `kill -0`: delivers no signal, only reports whether one *could* be — the
/// standard way to ask "is this pid still alive" without touching it.
#[cfg(unix)]
fn process_alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

/// **The D-039 burst.** Four peers messaging one pane at the same moment must all
/// arrive, in the order the hub took them, and must reach the terminal as *fewer
/// writes than there were messages* — which is the whole point: the merging that
/// the receiving TUI was doing invisibly and badly is now ours, done where the
/// framing still says who sent what.
///
/// The write holds the pty for the 30 ms submit gap, so messages 2–4 are
/// guaranteed to be queued while message 1 is in flight. That is what makes this
/// deterministic rather than a race the test happens to win.
#[test]
fn a_burst_from_four_peers_arrives_whole_in_order_and_in_fewer_writes() {
    let (registry, transcript) = fleet();
    let target = PaneId::Orch;
    transcript.wait_for(&out_channel(target), "ready");

    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let (tx, rx) = mpsc::unbounded_channel::<AppCommand>();
    spawn_delivery(&rt, registry.clone(), rx, scratch_store(), Arc::new(GaugeSources::default()));

    // Fire all four without awaiting any of them — a real burst, not a sequence.
    let answers: Vec<oneshot::Receiver<_>> = WORKER_SLOTS
        .iter()
        .map(|slot| {
            let (ack, answer) = oneshot::channel();
            tx.send(AppCommand::Deliver {
                to: target,
                text: frame_for_pane(PaneId::Worker(*slot), &format!("from worker {slot}")),
                ack,
            })
            .unwrap();
            answer
        })
        .collect();

    for (i, answer) in answers.into_iter().enumerate() {
        let result = rt.block_on(answer).unwrap_or_else(|_| panic!("leg {i} went unanswered"));
        assert!(result.accepted, "leg {i} was refused: {result:?}");
    }

    for slot in WORKER_SLOTS {
        transcript.wait_for(&out_channel(target), &format!("from worker {slot}"));
    }

    let seen = transcript.text(&out_channel(target));
    let order: Vec<usize> = WORKER_SLOTS
        .iter()
        .map(|slot| seen.find(&format!("from worker {slot}")).expect("present"))
        .collect();
    let mut sorted = order.clone();
    sorted.sort_unstable();
    assert_eq!(order, sorted, "the pane saw the burst out of the order the hub took it");

    // Each write opens exactly one bracketed paste, and the pane's tty echoes it
    // back, so counting the opening markers counts the writes. Four unbatched
    // messages would show four; batching shows fewer.
    let opens = seen.matches("\u{1b}[200~").count();
    assert!(opens >= 1, "no paste ever reached the pane: {seen:?}");
    assert!(
        opens < WORKER_SLOTS.len(),
        "four messages reached the pane as {opens} separate pastes — they were not batched"
    );

    registry.kill_all();
}

/// **WP-04, end to end.** A real pty, a real `GaugeSources` recording, a real
/// transcript fixture on disk, and a real `AppCommand::Roster` round trip —
/// the whole chain `crate::fleet::spawn_pane` → `GaugeSources::record` →
/// `deliver::spawn_delivery`'s `Roster` arm → the answer a `fleet roster` (or
/// the UI's poll) actually receives. `src-tauri/src/deliver.rs`'s own tests
/// cover `augment_with_gauge` in isolation; this is the one place the pty and
/// the sampler are proven to agree on which pane is which.
#[test]
fn a_roster_ask_surfaces_a_workers_sampled_context_gauge() {
    let (registry, transcript) = fleet();
    let target = PaneId::Worker(1);
    transcript.wait_for(&out_channel(target), "ready");

    // The fixture: a worker's own transcript, seeded exactly where
    // `context_gauge::project_dir` will look for it — the same cwd
    // `fleet_with_registry_path` spawned worker-1 in, under a config dir this
    // test owns outright.
    // The cwd `fleet_with_registry_path` placed worker-1 in: this thread's scratch
    // target, reached by the shared-checkout fallback since that target is no
    // repository.
    let (_, cwd) = scratch_installation();
    let config_dir = std::env::temp_dir().join(format!(
        "fleetor-panes-gauge-cfg-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&config_dir);
    let resolved = spawn::project_key(&cwd);
    let slug: String = resolved.chars().map(|c| if c == '/' || c == '.' { '-' } else { c }).collect();
    let project_dir = config_dir.join("projects").join(slug);
    std::fs::create_dir_all(&project_dir).unwrap();
    // A fifth of the window constant — derived, so this stays 20% if D-054's
    // number moves again.
    let fifth = fleetor_shell::context_gauge::WORKER_WINDOW_TOKENS / 5;
    std::fs::write(
        project_dir.join("session.jsonl"),
        serde_json::json!({
            "type": "assistant",
            "message": {"usage": {"input_tokens": fifth, "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0}}
        })
        .to_string(),
    )
    .unwrap();

    let gauges = Arc::new(GaugeSources::default());
    gauges.record(
        target,
        fleetor_shell::context_gauge::TranscriptSource {
            harness: fleetor_shell::placement::harness::claude_code(),
            config_dir: config_dir.clone(),
            cwd,
        },
    );

    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let (tx, rx) = mpsc::unbounded_channel::<AppCommand>();
    spawn_delivery(&rt, registry.clone(), rx, scratch_store(), gauges);

    let (ack, answer) = oneshot::channel();
    tx.send(AppCommand::Roster { ack }).unwrap();
    let panes = rt.block_on(answer).expect("the roster ack must arrive");

    let worker_one = panes.iter().find(|e| e.pane == target).expect("worker-1 is on the roster");
    let gauge = worker_one.context.expect("worker-1's seeded transcript must be sampled");
    assert_eq!(gauge.used_tokens, fifth);
    assert_eq!(gauge.pct, 20, "a fifth of the worker window is 20%");

    let orch = panes.iter().find(|e| e.pane == PaneId::Orch).expect("orch is on the roster");
    assert_eq!(orch.context, None, "orch is never sampled, even when it is live");

    let other_worker = panes.iter().find(|e| e.pane == PaneId::Worker(2)).expect("worker-2 is on the roster");
    assert_eq!(other_worker.context, None, "a worker nobody recorded a source for stays absent");

    registry.kill_all();
    let _ = std::fs::remove_dir_all(&config_dir);
}

// --- the Critic's interview, on real ptys (WP-21, D-079) ----------------------
//
// Stage A's gate, proven where the arc doc requires it: real processes on real
// ptys, a real `Hub`, a real store, and the operator's switch moved between the
// two calls. A `CommandBuilder` inspection could only have shown that the Critic
// now holds a socket — which is exactly the half of this design that is *not*
// the safety property.

/// Everything one interview test needs: orch and the Critic on real ptys, a hub
/// wired to a real delivery loop, and the store the hub logs into.
///
/// The store is a real one rather than [`scratch_store`]'s throwaway because
/// **the log is the assertion** in the closed-gate test below: a refusal that
/// left a row behind would be the accepted-then-dropped shape Tier 1.4 bans, and
/// nothing but reading the log back can tell the difference.
struct Interviewed {
    registry: Arc<PaneRegistry>,
    transcript: Arc<Transcript>,
    store: Arc<dyn Store>,
    hub: Arc<Hub>,
    rt: tokio::runtime::Runtime,
}

impl Interviewed {
    fn new() -> Self {
        let transcript = Arc::new(Transcript::default());
        let registry = Arc::new(PaneRegistry::new(transcript.emitter(), scratch_registry_path()));

        let (layout, target) = scratch_installation();
        // `place_critic` lays its cwd out from a snapshot of the live run, so a
        // real store has to exist under this layout before it is called — the
        // same precondition `tests/common`'s bench establishes.
        std::fs::create_dir_all(layout.shell()).unwrap();
        SqliteStore::open(&layout.shell().join("state.db"))
            .expect("a live run for the Critic to be placed against");

        let host = stand_in_host();
        let ctx = PaneContext::baked();
        for spec in [PaneSpec::Orch, PaneSpec::Critic { run: RunSource::Live }] {
            let pane = spec.pane();
            let placed = placement::place(spec, &layout, &host, &target, &ctx)
                .unwrap_or_else(|e| panic!("placing {pane}: {e}"));
            registry.spawn(pane, placed.command, 24, 80).unwrap();
        }

        let store: Arc<dyn Store> = Arc::new(SqliteStore::open_in_memory().unwrap());
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let (tx, rx) = mpsc::unbounded_channel::<AppCommand>();
        spawn_delivery(&rt, registry.clone(), rx, store.clone(), Arc::new(GaugeSources::default()));
        let hub = Hub::new(store.clone(), tx);

        Self { registry, transcript, store, hub, rt }
    }

    /// Every row in the event log, as `(seq, kind)` — enough to prove the log
    /// did not move without pinning what an unrelated event looks like.
    fn log(&self) -> Vec<(i64, String)> {
        self.store
            .events_since(0)
            .unwrap()
            .into_iter()
            .map(|(seq, event)| (seq, event.kind().to_string()))
            .collect()
    }

    fn send(&self, from: PaneId, to: PaneId, text: &str) -> OpResult {
        self.rt.block_on(self.hub.handle(from, Op::Send { to, text: text.to_string() }))
    }
}

/// **The Tier 1.4 assertion, and the reason this file is where it lives.**
///
/// With the interview closed, `fleet send orch` from inside the Critic is
/// refused — and the thing that makes the refusal legal rather than the banned
/// accepted-then-dropped shape is what *did not happen*: the event log is
/// identical either side of the attempt, and orch's pty never saw the words.
/// Asserting only that the call failed would pass against an implementation that
/// logged the attempt and threw the message away, which is precisely the shape
/// `building.md` §9.3 records as argued and lost twice.
#[test]
fn a_closed_interview_refuses_the_critic_and_leaves_the_event_log_untouched() {
    let bench = Interviewed::new();
    bench.transcript.wait_for(&out_channel(PaneId::Orch), "ready");

    // A first, ordinary delivery, so the log is non-empty: "unchanged" has to
    // mean "did not move", not "was empty both times".
    let warmed = bench.send(PaneId::Orch, PaneId::Critic, "an ordinary message");
    assert!(
        matches!(warmed, OpResult::Delivered { accepted: true, .. }),
        "the ordinary path must still work: {warmed:?}",
    );
    let before = bench.log();
    assert!(!before.is_empty(), "the log must have something in it for this test to mean anything");

    let refused = bench.send(PaneId::Critic, PaneId::Orch, "why did you route it that way");
    let OpResult::Error { message } = refused else {
        panic!("a closed interview must refuse: {refused:?}");
    };
    assert!(
        message.contains("interview is closed") && message.contains("nothing was sent"),
        "the sentence has to say what happened and what to do about it: {message:?}",
    );

    assert_eq!(
        bench.log(),
        before,
        "a refused send wrote to the event log — that is the accepted-then-dropped shape \
         Tier 1.4 bans, and it makes the log lie about what the run contained",
    );

    // And nothing reached the terminal either. Given time to arrive rather than
    // asserted instantly, so a delivery that was merely slow cannot pass this.
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        !bench.transcript.text(&out_channel(PaneId::Orch)).contains("why did you route it"),
        "the refused message reached orch's pty",
    );

    bench.registry.kill_all();
}

/// **The other edge: open, and it is an ordinary send.**
///
/// The identical call the test above refuses is `accepted`, reaches orch's real
/// pty framed as any pane's message is. The switch is the only thing that moved
/// between the two tests.
#[test]
fn an_open_interview_lets_the_critic_reach_a_real_pane() {
    let bench = Interviewed::new();
    bench.transcript.wait_for(&out_channel(PaneId::Orch), "ready");

    assert!(!bench.hub.interview().is_open(), "a fresh fleet starts closed");
    assert!(bench.hub.interview().set(true), "the switch answers with what it stored");

    let result = bench.send(PaneId::Critic, PaneId::Orch, "what did you believe at the time");
    let OpResult::Delivered { accepted, detail, .. } = result else {
        panic!("an open interview delivers: {result:?}");
    };
    assert!(accepted, "the Critic's message was refused: {detail:?}");

    let seen = bench.transcript.wait_for(&out_channel(PaneId::Orch), "what did you believe");
    assert!(
        seen.contains("[fleet · critic]"),
        "a pane sees an ordinary `fleet send` from `critic`, framed like any other: {seen:?}",
    );

    // Closing it again refuses the very next call, with no respawn in between —
    // which is the whole reason the gate is in the hub rather than at spawn.
    assert!(!bench.hub.interview().set(false));
    let refused = bench.send(PaneId::Critic, PaneId::Orch, "one more question");
    assert!(
        matches!(refused, OpResult::Error { .. }),
        "the switch must close as well as open: {refused:?}",
    );

    bench.registry.kill_all();
}

/// **Inbound: a pane can answer the Critic** (WP-21 performance criteria).
///
/// WP-20's ticket 08 only ever tested the Critic's *outbound* refusal, so a
/// pane's ability to reply to it was unproven — an interview in which only one
/// side can speak is not an interview. This drives the whole path: `fleet send
/// critic` from orch, through the hub, through `deliver.rs`'s writer, onto the
/// Critic's real pty, and into the log naming it as the recipient.
///
/// **It needs no interview.** The gate is on the *sender*, and orch is not
/// gated: the operator's switch decides whether the Critic may interrupt the
/// fleet, never whether a pane may answer one.
#[test]
fn a_pane_can_answer_the_critic_and_the_log_records_the_recipient() {
    let bench = Interviewed::new();
    bench.transcript.wait_for(&out_channel(PaneId::Critic), "ready");

    let result = bench.send(PaneId::Orch, PaneId::Critic, "I had no receipt, I inferred it");
    let OpResult::Delivered { accepted, detail, .. } = result else {
        panic!("a message to the Critic delivers: {result:?}");
    };
    assert!(accepted, "orch's answer to the Critic was refused: {detail:?}");

    let seen = bench.transcript.wait_for(&out_channel(PaneId::Critic), "I had no receipt");
    assert!(seen.contains("[fleet · orch]"), "framed like any message: {seen:?}");

    assert!(
        bench.store.events_since(0).unwrap().iter().any(|(_, event)| matches!(
            event,
            FleetEvent::Message { to, from, .. } if *to == PaneId::Critic && *from == PaneId::Orch
        )),
        "the log must name the Critic as the recipient, or an interview leaves no record of \
         which half of it was the answer",
    );

    bench.registry.kill_all();
}
