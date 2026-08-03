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
//! (`docs/tui-spawn-notes.md`), not by a test — a fake pane would answer the
//! question wrong in the reassuring direction.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD, Engine};
use fleetor_core::message::frame_for_pane;
use fleetor_core::pane::{PaneId, WORKER_SLOTS};
use fleetor_shell::pty::{exit_channel, out_channel, Emit, PaneRegistry};
use fleetor_shell::spawn;

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

/// Bring up a registry with every pane running the stand-in, through the real
/// `spawn.rs` command builders so their environment work is exercised too.
fn fleet() -> (Arc<PaneRegistry>, Arc<Transcript>) {
    // Process-global, but every test in this binary sets it to the same value,
    // so the parallel writes are all identical.
    std::env::set_var("FLEETOR_PANE_CMD", fake_pane_script());

    let transcript = Arc::new(Transcript::default());
    let registry = Arc::new(PaneRegistry::new(transcript.emitter()));

    let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let socket = PathBuf::from("/tmp/fleetor-panes-test.sock");
    registry.spawn(PaneId::Orch, spawn::orch_command(&cwd, &socket), 24, 80).unwrap();
    for slot in WORKER_SLOTS {
        let command = spawn::worker_command(slot, &cwd, &cwd.join("unused-cfg"), &socket, "sk-test");
        registry.spawn(PaneId::Worker(slot), command, 24, 80).unwrap();
    }
    (registry, transcript)
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
    let command = spawn::worker_command(4, &cwd, &cwd.join("unused-cfg"), &socket, "sk-test");
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
