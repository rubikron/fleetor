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
use fleetor_core::message::frame_for_pane;
use fleetor_core::pane::{PaneId, WORKER_SLOTS};
use fleetor_core::Store;
use fleetor_db::SqliteStore;
use fleetor_shell::context_gauge::GaugeSources;
use fleetor_shell::deliver::spawn_delivery;
use fleetor_shell::pty::{exit_channel, out_channel, Emit, PaneRegistry};
use fleetor_shell::prompts::PaneContext;
use fleetor_shell::spawn;
use fleetor_server::AppCommand;
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

/// Bring up a registry with every pane running the stand-in, through the real
/// `spawn.rs` command builders so their environment work is exercised too.
fn fleet() -> (Arc<PaneRegistry>, Arc<Transcript>) {
    let (registry, transcript, _registry_path) = fleet_with_registry_path();
    (registry, transcript)
}

/// [`fleet`], plus the scratch pid-registry path it used — for the one test
/// that needs to read that file back and check what landed in it.
fn fleet_with_registry_path() -> (Arc<PaneRegistry>, Arc<Transcript>, PathBuf) {
    // Process-global, but every test in this binary sets it to the same value,
    // so the parallel writes are all identical.
    std::env::set_var("FLEETOR_PANE_CMD", fake_pane_script());

    let transcript = Arc::new(Transcript::default());
    let registry_path = scratch_registry_path();
    let registry = Arc::new(PaneRegistry::new(transcript.emitter(), registry_path.clone()));

    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let socket = PathBuf::from("/tmp/fleetor-panes-test.sock");
    let orch = spawn::orch_command(&cwd, &socket, &cwd.join("unused-orch-cfg"), &PaneContext::baked());
    registry.spawn(PaneId::Orch, orch, 24, 80).unwrap();
    for slot in WORKER_SLOTS {
        let command = spawn::worker_command(
            slot,
            &cwd,
            &cwd.join("unused-home"),
            &cwd.join("unused-cfg"),
            &socket,
            "sk-test",
            &PaneContext::baked(),
        );
        registry.spawn(PaneId::Worker(slot), command, 24, 80).unwrap();
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

    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let socket = PathBuf::from("/tmp/fleetor-panes-test.sock");
    let command = spawn::worker_command(
        4,
        &cwd,
        &cwd.join("unused-home"),
        &cwd.join("unused-cfg"),
        &socket,
        "sk-test",
        &PaneContext::baked(),
    );
    registry.spawn(target, command, 24, 80).expect("a dead pane respawns");

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
    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
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
        fleetor_shell::context_gauge::TranscriptSource { config_dir: config_dir.clone(), cwd },
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
