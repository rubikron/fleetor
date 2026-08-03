//! The **orchestrator** pty bridge.
//!
//! Spawns the operator's real `claude` TUI under a pseudo-terminal, in the fleet's
//! target repo. Output bytes stream to the webview (base64, so multibyte UTF-8 and
//! escape sequences never split across a chunk boundary); keystrokes and resizes
//! relay back.
//!
//! This is the operator's **own Opus** — we inherit their environment rather than
//! the isolated config the *workers* get. Spawning it spends tokens, so the UI
//! gates this behind an explicit confirm.
//!
//! **Phase 2 removed the MCP wiring.** The orchestrator used to be handed a `fleet`
//! MCP server (the shim) so it could call `assign`/`await_events`/`reply`; there is
//! nothing behind those tools now that the headless fleet is unwired, and a tool
//! surface that silently can't work is worse than none. Phase 3 replaces it with
//! the `fleet` CLI — a Bash command, not an MCP server — which is why
//! `FLEET_SOCKET` stays.
//!
//! Phase 3 rewrites this file into an N-pane registry (`PaneId`-keyed sessions,
//! per-pane event channels, coalesced reads). Everything here is the single-session
//! Phase 0.5 bridge until then.

use std::io::{Read, Write};
use std::sync::Mutex;

use base64::{engine::general_purpose::STANDARD, Engine};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use tauri::{AppHandle, Emitter, State};

use crate::fleet;

/// Emitted for every chunk of pty output; payload is base64 of the raw bytes.
const EVENT_OUTPUT: &str = "pty://output";
/// Emitted once when the child exits or the pty closes.
const EVENT_EXIT: &str = "pty://exit";

/// A live pty session. The reader half is moved into its own thread; the
/// master (for resize) and writer (for keystrokes) stay here behind the mutex.
struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

/// Managed Tauri state: at most one session at a time.
#[derive(Default)]
pub struct PtyState(Mutex<Option<Session>>);

/// Ensure `claude` (and node) are reachable even when the app was launched from a
/// GUI context whose PATH didn't inherit the login shell's additions.
fn augmented_path() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let existing = std::env::var("PATH").unwrap_or_default();
    format!("{home}/.local/bin:{home}/.bun/bin:/opt/homebrew/bin:/usr/local/bin:{existing}")
}

/// Spawn the orchestrator's `claude` in a pty of the given size. If a session
/// already exists it is left untouched and this is a no-op (the shell drives
/// exactly one orchestrator).
///
/// The fleet must be bootstrapped first, both so the hub socket exists and so the
/// target is resolved (and the testbed seeded) before this reads it as the cwd.
#[tauri::command]
pub fn pty_spawn(
    app: AppHandle,
    state: State<'_, PtyState>,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    let mut guard = state.0.lock().map_err(|e| e.to_string())?;
    if guard.is_some() {
        return Ok(());
    }

    let cwd = fleet::target_dir();
    std::fs::create_dir_all(&cwd).map_err(|e| format!("create orchestrator cwd: {e}"))?;

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| format!("openpty: {e}"))?;

    let mut cmd = CommandBuilder::new("claude");
    cmd.cwd(&cwd);
    // Faithful orchestrator TUI: inherit the operator's real environment (their
    // login, config, Opus) so this is literally their `claude`, then force a
    // truecolor-capable TERM.
    for (k, v) in std::env::vars() {
        cmd.env(k, v);
    }
    cmd.env("PATH", augmented_path());
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    // The hub to talk to. Phase 3's `fleet` CLI reads this to find the socket.
    cmd.env("FLEET_SOCKET", fleet::socket_path());

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("spawn claude: {e}"))?;
    // Slave fd is held by the child now; drop our copy so EOF propagates on exit.
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("clone reader: {e}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("take writer: {e}"))?;

    // Reader pump: raw bytes -> base64 -> webview event.
    let app_reader = app.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let encoded = STANDARD.encode(&buf[..n]);
                    if app_reader.emit(EVENT_OUTPUT, encoded).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = app_reader.emit(EVENT_EXIT, ());
    });

    *guard = Some(Session { master: pair.master, writer, child });
    Ok(())
}

/// Relay a chunk of user input (keystrokes, paste) to the pty.
#[tauri::command]
pub fn pty_write(state: State<'_, PtyState>, data: String) -> Result<(), String> {
    let mut guard = state.0.lock().map_err(|e| e.to_string())?;
    let session = guard.as_mut().ok_or("no pty session")?;
    session
        .writer
        .write_all(data.as_bytes())
        .map_err(|e| e.to_string())?;
    session.writer.flush().map_err(|e| e.to_string())
}

/// Resize the pty to match the terminal's fitted grid.
#[tauri::command]
pub fn pty_resize(state: State<'_, PtyState>, rows: u16, cols: u16) -> Result<(), String> {
    let guard = state.0.lock().map_err(|e| e.to_string())?;
    let session = guard.as_ref().ok_or("no pty session")?;
    session
        .master
        .resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| e.to_string())
}

/// Best-effort kill on window close so we don't leave an orphaned `claude`.
pub fn kill_session(state: &PtyState) {
    if let Ok(mut guard) = state.0.lock() {
        if let Some(mut session) = guard.take() {
            let _ = session.child.kill();
        }
    }
}
