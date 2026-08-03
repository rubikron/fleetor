//! The **pane registry** — N live `claude` terminals, one pty each (D-030, Phase 3).
//!
//! This was a single-session bridge for the orchestrator. It is now the thing the
//! whole product stands on: five terminals, each with its own pty, its own pair of
//! event channels, and its own writer lock.
//!
//! Four decisions here are load-bearing, and each of them is a silent failure if
//! reversed:
//!
//!  - **Per-pane event channels** (`pty://output/orch`, `pty://output/2`). Not one
//!    channel with an id in the payload: `AppHandle::emit` wakes *every* listener
//!    on a name, so a shared channel means five JS callbacks fire for every chunk
//!    from every pane and four of them discard it. Per-id names make that
//!    impossible rather than merely unlikely — pane 2 cannot receive pane 1's
//!    bytes if it never listens on that name (L6, chokepoint #2).
//!  - **Coalescing in the reader** (~16 ms / 64 KB). `read()` returns as soon as
//!    *any* bytes are available, so a TUI's 200-byte spinner frame makes a
//!    complete trip across the IPC bridge. The per-event cost barely shrinks with
//!    payload size, which is what makes merging 50 × 200 B into 1 × 10 KB an
//!    order-of-magnitude win (L6, chokepoint #1).
//!  - **One write, one lock** ([`PaneRegistry::write_paste`]). Tauri dispatches
//!    sync commands on a thread pool, so a keystroke racing a delivery could
//!    otherwise land between a message's body and its `\r` — submitting it early
//!    or corrupting it. The writer mutex is per-pane and held across the whole
//!    injection, including the 30 ms gap.
//!  - **`kill_all` signals the process group.** `openpty` gives each child its own
//!    session, so killing the pid alone can leave the real work orphaned. A leaked
//!    Opus after window close is a money bug.
//!
//! The registry itself knows nothing about Tauri: it emits through an [`Emit`]
//! callback, so `src-tauri/tests/panes.rs` drives five real ptys without a window.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD, Engine};
use fleetor_core::pane::{PaneEntry, PaneId, PaneState};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use tauri::State;

/// Merge pty reads for this long before emitting, so a repainting TUI doesn't
/// cross the IPC bridge once per spinner frame.
const COALESCE_WINDOW: Duration = Duration::from_millis(16);
/// …or until this much is buffered, whichever comes first.
const COALESCE_MAX: usize = 64 * 1024;
/// One pty read.
const READ_CHUNK: usize = 8192;

/// Bracketed paste, so the receiving TUI treats a message as pasted text rather
/// than as a stream of keystrokes.
const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";

/// The gap between the closing paste marker and the `\r` that submits it.
///
/// Phase 0 measured 0/10/30 ms and all three submit reliably — pty stream ordering
/// is preserved. 30 ms anyway, so we don't depend on Claude Code batching the
/// end-marker and the CR within one input-handler tick, which is a version detail.
/// This is the **only** delay anywhere between `fleet send` and a pty (D-034).
const SUBMIT_GAP: Duration = Duration::from_millis(30);

/// How long a pane gets to honor SIGTERM before it is killed outright.
const TERM_GRACE: Duration = Duration::from_millis(200);
const TERM_POLL: Duration = Duration::from_millis(20);

/// How the registry talks to the outside world: `(channel, base64 payload)`.
/// A callback rather than an `AppHandle` so the registry is testable headless.
pub type Emit = Arc<dyn Fn(&str, String) + Send + Sync>;

/// One live terminal.
struct Pane {
    master: Box<dyn MasterPty + Send>,
    /// Shared with every writer — keystrokes *and* deliveries — so an injection
    /// and a keypress can never interleave.
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Box<dyn Child + Send + Sync>,
    /// Written by the reader thread, read by the roster. A [`PaneState`] as an
    /// atomic, because the reader learns of an exit and the roster asks about it.
    state: Arc<AtomicU8>,
}

const SPAWNING: u8 = 0;
const LIVE: u8 = 1;
const DEAD: u8 = 2;

fn decode_state(raw: u8) -> PaneState {
    match raw {
        LIVE => PaneState::Live,
        DEAD => PaneState::Dead,
        _ => PaneState::Spawning,
    }
}

/// Managed Tauri state: every pane the shell is running.
pub struct PaneRegistry {
    panes: Mutex<HashMap<PaneId, Pane>>,
    emit: Emit,
}

impl PaneRegistry {
    pub fn new(emit: Emit) -> Self {
        Self { panes: Mutex::new(HashMap::new()), emit }
    }

    /// Spawn `cmd` under a fresh pty as `pane`.
    ///
    /// Idempotent for a pane that is still running (React StrictMode double-mounts
    /// every terminal), and a **respawn** for one that has died — which is what
    /// makes per-tab restart a two-line command rather than a special case.
    pub fn spawn(
        &self,
        pane: PaneId,
        cmd: CommandBuilder,
        rows: u16,
        cols: u16,
    ) -> Result<(), String> {
        let mut panes = self.lock()?;
        if let Some(existing) = panes.get(&pane) {
            if decode_state(existing.state.load(Ordering::Relaxed)).accepts_input() {
                return Ok(());
            }
            panes.remove(&pane); // dead: drop it and start a new one
        }

        let pair = native_pty_system()
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| format!("openpty for {pane}: {e}"))?;

        let child = pair.slave.spawn_command(cmd).map_err(|e| format!("spawn {pane}: {e}"))?;
        // The child holds the slave fd now; drop ours so EOF propagates on exit.
        drop(pair.slave);

        let reader =
            pair.master.try_clone_reader().map_err(|e| format!("clone reader for {pane}: {e}"))?;
        let writer =
            pair.master.take_writer().map_err(|e| format!("take writer for {pane}: {e}"))?;

        let state = Arc::new(AtomicU8::new(SPAWNING));
        // The channel names are computed once, here, and moved into the pump —
        // the reader thread must not `format!` per read.
        spawn_pump(reader, self.emit.clone(), out_channel(pane), exit_channel(pane), state.clone());

        panes.insert(
            pane,
            Pane { master: pair.master, writer: Arc::new(Mutex::new(writer)), child, state },
        );
        Ok(())
    }

    /// Relay operator keystrokes. Takes the same per-pane writer lock a delivery
    /// does, which is what stops the two from interleaving.
    pub fn write(&self, pane: PaneId, data: &[u8]) -> Result<(), String> {
        let writer = self.writable(pane)?;
        let mut guard = writer.lock().map_err(|e| e.to_string())?;
        guard.write_all(data).map_err(|e| format!("write to {pane}: {e}"))?;
        guard.flush().map_err(|e| format!("flush {pane}: {e}"))
    }

    /// Type one framed message into `pane` as a bracketed paste, then submit it.
    ///
    /// The whole injection happens under a single hold of that pane's writer lock,
    /// so nothing — not a keystroke, not another delivery — can land between the
    /// body and its `\r`. Blocking, by design: [`crate::deliver`] runs it off the
    /// command loop so one slow pane cannot stall the other four.
    pub fn write_paste(&self, pane: PaneId, text: &str) -> Result<(), String> {
        let writer = self.writable(pane)?;

        let mut body = Vec::with_capacity(text.len() + PASTE_START.len() + PASTE_END.len());
        body.extend_from_slice(PASTE_START);
        body.extend_from_slice(text.as_bytes());
        body.extend_from_slice(PASTE_END);

        let mut guard = writer.lock().map_err(|e| e.to_string())?;
        guard.write_all(&body).map_err(|e| format!("write to {pane}: {e}"))?;
        guard.flush().map_err(|e| format!("flush {pane}: {e}"))?;
        std::thread::sleep(SUBMIT_GAP);
        guard.write_all(b"\r").map_err(|e| format!("submit to {pane}: {e}"))?;
        guard.flush().map_err(|e| format!("flush {pane}: {e}"))
    }

    pub fn resize(&self, pane: PaneId, rows: u16, cols: u16) -> Result<(), String> {
        let panes = self.lock()?;
        let entry = panes.get(&pane).ok_or_else(|| format!("{pane} is not running"))?;
        entry
            .master
            .resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| format!("resize {pane}: {e}"))
    }

    /// Stop one pane. Its tab stays; a later [`PaneRegistry::spawn`] restarts it.
    ///
    /// The exit event is left to the pump, which fires it when the pty EOFs —
    /// both dropping the master and killing the child guarantee that. Emitting
    /// one here too would give the tab two deaths for one process.
    pub fn kill(&self, pane: PaneId) -> Result<(), String> {
        let mut panes = self.lock()?;
        let mut entry = panes.remove(&pane).ok_or_else(|| format!("{pane} is not running"))?;
        terminate(&mut entry);
        Ok(())
    }

    /// Reap every pane. Called on window close — best-effort and deliberately
    /// silent, because there is nowhere left to report to.
    pub fn kill_all(&self) {
        let Ok(mut panes) = self.panes.lock() else { return };
        for (_, mut entry) in panes.drain() {
            terminate(&mut entry);
        }
    }

    /// Every pane the shell is running, orch first. The hub's single source of
    /// truth for fleet membership — a pane that was never spawned must not appear
    /// here, or `fleet broadcast` fans out to somewhere that cannot receive.
    pub fn roster(&self) -> Vec<PaneEntry> {
        let Ok(panes) = self.panes.lock() else { return Vec::new() };
        let mut entries: Vec<PaneEntry> = panes
            .iter()
            .map(|(pane, p)| PaneEntry::new(*pane, decode_state(p.state.load(Ordering::Relaxed))))
            .collect();
        entries.sort_by_key(|e| e.pane);
        entries
    }

    /// The writer for a pane that can take input, or the sentence explaining why
    /// not — which becomes the `detail` the sending model reads on stderr.
    ///
    /// **The predicate is [`PaneState::accepts_input`], never `is_live()`.**
    /// Nothing tells us when `claude` reaches its prompt; `Spawning` is a guess
    /// about a running process. Refusing on it means a healthy pane silently drops
    /// every message until the guess catches up — which is L1's shape exactly. A
    /// write to a still-booting pty is buffered by the kernel and read when the
    /// TUI starts reading; that is the failure we can live with.
    fn writable(&self, pane: PaneId) -> Result<Arc<Mutex<Box<dyn Write + Send>>>, String> {
        let panes = self.lock()?;
        let entry = panes.get(&pane).ok_or_else(|| format!("{pane} is not running"))?;
        let state = decode_state(entry.state.load(Ordering::Relaxed));
        if !state.accepts_input() {
            return Err(format!("{pane} has exited"));
        }
        Ok(entry.writer.clone())
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, HashMap<PaneId, Pane>>, String> {
        self.panes.lock().map_err(|e| format!("pane registry poisoned: {e}"))
    }
}

/// `pty://output/orch`, `pty://output/2`.
pub fn out_channel(pane: PaneId) -> String {
    format!("pty://output/{}", channel_key(pane))
}

/// `pty://exit/orch`, `pty://exit/2`.
pub fn exit_channel(pane: PaneId) -> String {
    format!("pty://exit/{}", channel_key(pane))
}

fn channel_key(pane: PaneId) -> String {
    match pane.slot() {
        Some(n) => n.to_string(),
        None => "orch".to_string(),
    }
}

/// Read the pty on one thread and coalesce on another.
///
/// Two threads, not one, and the reason is a correctness bug rather than taste: a
/// single thread can only check its window *after* the next blocking `read`
/// returns, so the tail of a burst would sit unflushed until the pane happened to
/// print again — the last line of a finished turn, invisible for as long as the
/// pane stays quiet. The coalescer's `recv_timeout` has no such blind spot.
fn spawn_pump(
    mut reader: Box<dyn Read + Send>,
    emit: Emit,
    out_channel: String,
    exit_channel: String,
    state: Arc<AtomicU8>,
) {
    let (tx, rx) = mpsc::channel::<Vec<u8>>();

    std::thread::spawn(move || {
        let mut buf = [0u8; READ_CHUNK];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
        // Dropping `tx` here is what tells the coalescer the pane is gone.
    });

    std::thread::spawn(move || {
        loop {
            let Ok(first) = rx.recv() else { break };
            let mut batch = first;
            let deadline = Instant::now() + COALESCE_WINDOW;
            let mut disconnected = false;
            while batch.len() < COALESCE_MAX {
                let remaining = deadline.saturating_duration_since(Instant::now());
                match rx.recv_timeout(remaining) {
                    Ok(more) => batch.extend_from_slice(&more),
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
            // The pane painted something, so it is past `claude`'s startup. The
            // closest honest signal we have that it is a terminal and not a
            // process; delivery does not depend on it (see `writable`).
            state.store(LIVE, Ordering::Relaxed);
            emit(&out_channel, STANDARD.encode(&batch));
            if disconnected {
                break;
            }
        }
        state.store(DEAD, Ordering::Relaxed);
        emit(&exit_channel, String::new());
    });
}

/// SIGTERM the child's **process group**, then SIGKILL what is left.
///
/// `openpty` puts the child in its own session, so it is a process-group leader
/// and its own children (a `claude` runs plenty) share its pgid. Killing the pid
/// alone leaves them running with no terminal and nobody watching — which for an
/// Opus session is a bill that keeps growing after the window is closed.
fn terminate(pane: &mut Pane) {
    pane.state.store(DEAD, Ordering::Relaxed);
    let pid = pane.child.process_id();

    #[cfg(unix)]
    if let Some(pid) = pid {
        unsafe { libc::killpg(pid as i32, libc::SIGTERM) };
    }

    let deadline = Instant::now() + TERM_GRACE;
    while Instant::now() < deadline {
        if matches!(pane.child.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(TERM_POLL);
    }

    #[cfg(unix)]
    if let Some(pid) = pid {
        unsafe { libc::killpg(pid as i32, libc::SIGKILL) };
    }
    let _ = pane.child.kill();
    let _ = pane.child.wait();
}

// --- Tauri commands (thin wrappers) -------------------------------------------

#[tauri::command]
pub fn pty_spawn(
    registry: State<'_, Arc<PaneRegistry>>,
    fleet: State<'_, crate::fleet::FleetState>,
    pane: PaneId,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    crate::fleet::spawn_pane(&fleet, &registry, pane, rows, cols)
}

#[tauri::command]
pub fn pty_write(
    state: State<'_, Arc<PaneRegistry>>,
    pane: PaneId,
    data: String,
) -> Result<(), String> {
    state.write(pane, data.as_bytes())
}

#[tauri::command]
pub fn pty_resize(
    state: State<'_, Arc<PaneRegistry>>,
    pane: PaneId,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    state.resize(pane, rows, cols)
}

#[tauri::command]
pub fn pty_kill(state: State<'_, Arc<PaneRegistry>>, pane: PaneId) -> Result<(), String> {
    state.kill(pane)
}

/// Best-effort teardown on window close.
pub fn kill_all(state: &Arc<PaneRegistry>) {
    state.kill_all();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The channel names the UI listens on. Pinned because a rename that only
    /// lands on one side produces a pane that renders nothing and reports no
    /// error — there is no failure signal for listening on the wrong name.
    #[test]
    fn each_pane_owns_a_distinct_pair_of_channel_names() {
        assert_eq!(out_channel(PaneId::Orch), "pty://output/orch");
        assert_eq!(out_channel(PaneId::Worker(2)), "pty://output/2");
        assert_eq!(exit_channel(PaneId::Orch), "pty://exit/orch");
        assert_eq!(exit_channel(PaneId::Worker(4)), "pty://exit/4");

        let all: Vec<String> = PaneId::roster(&fleetor_core::pane::WORKER_SLOTS)
            .into_iter()
            .flat_map(|p| [out_channel(p), exit_channel(p)])
            .collect();
        let unique: std::collections::HashSet<&String> = all.iter().collect();
        assert_eq!(all.len(), unique.len(), "two panes share a channel: {all:?}");
    }

    /// Only a dead pane refuses input — asserted on the decoder the registry
    /// actually consults, so a state-encoding change can't quietly flip it.
    #[test]
    fn a_spawning_pane_still_accepts_input() {
        assert!(decode_state(SPAWNING).accepts_input(), "refusing here is L1's shape");
        assert!(decode_state(LIVE).accepts_input());
        assert!(!decode_state(DEAD).accepts_input());
    }
}
