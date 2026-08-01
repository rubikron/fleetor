//! Single-session pty bridge for the Phase 0.5 spike.
//!
//! Spawns the real `claude` TUI under a pseudo-terminal, streams its raw output
//! bytes to the webview (base64, so multibyte UTF-8 and escape sequences never
//! split across a chunk boundary), and relays keystrokes / resizes back.
//!
//! Deliberately minimal: one session, no supervision, no reconnection. This is
//! a spike to retire the WKWebView terminal-fidelity risk, not the real
//! orchestrator pty (that lands in fleetor-server, Phase 4).

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Mutex;

use base64::{engine::general_purpose::STANDARD, Engine};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use tauri::{AppHandle, Emitter, State};

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

/// Scratch cwd for the spawned TUI — out of the user's repo, `rm -rf`-able,
/// upholding the Tier-1 zero-footprint boundary.
fn spike_cwd() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".fleetor").join("_pty-spike")
}

/// Ensure `claude` is reachable even when the app was launched from a GUI
/// context whose PATH didn't inherit the login shell's additions.
fn augmented_path() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let existing = std::env::var("PATH").unwrap_or_default();
    format!(
        "{home}/.local/bin:{home}/.bun/bin:/opt/homebrew/bin:/usr/local/bin:{existing}"
    )
}

/// Spawn `claude` in a pty of the given size. If a session already exists it is
/// left untouched and this is a no-op (the spike drives exactly one).
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

    let cwd = spike_cwd();
    std::fs::create_dir_all(&cwd).map_err(|e| format!("create scratch cwd: {e}"))?;

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| format!("openpty: {e}"))?;

    let mut cmd = CommandBuilder::new("claude");
    cmd.cwd(&cwd);
    // Faithful TUI: inherit the operator's real environment (their login,
    // config, model) so this is literally their `claude`, then force a
    // truecolor-capable TERM.
    for (k, v) in std::env::vars() {
        cmd.env(k, v);
    }
    cmd.env("PATH", augmented_path());
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");

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
