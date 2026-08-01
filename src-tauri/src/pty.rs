//! The **lead** pty bridge (Phase 4e-2).
//!
//! Spawns the operator's real `claude` TUI under a pseudo-terminal as the fleet
//! **lead**: it runs in the scratch repo, with the `fleet` MCP server (the shim,
//! `FLEETOR_ROLE=lead`) added on top of the operator's own config so the model can
//! call `mcp__fleet__assign / await_events / reply / send` to drive the hub that
//! [`crate::fleet`] already bound. Output bytes stream to the webview (base64, so
//! multibyte UTF-8 and escape sequences never split across a chunk boundary);
//! keystrokes and resizes relay back.
//!
//! The lead is the operator's **own Opus** — we inherit their environment and add
//! the fleet surface, rather than the isolated DeepSeek config the *workers* get.
//! Spawning it spends tokens, so the UI gates this behind an explicit confirm.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Mutex;

use base64::{engine::general_purpose::STANDARD, Engine};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use tauri::{AppHandle, Emitter, State};

use crate::fleet;

/// Emitted for every chunk of pty output; payload is base64 of the raw bytes.
const EVENT_OUTPUT: &str = "pty://output";
/// Emitted once when the child exits or the pty closes.
const EVENT_EXIT: &str = "pty://exit";

/// The MCP server key the shim registers under — **must** be `fleet` so the model
/// sees the tools as `mcp__fleet__assign` etc. (matches `fleetor_cc::spawn`).
const FLEET_MCP_KEY: &str = "fleet";
/// The generated lead MCP config, written into the lead's config dir.
const LEAD_MCP_CONFIG_FILE: &str = "lead-mcp.json";

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

/// The lead's config dir, holding its generated MCP config. Under the shell dir
/// so `rm -rf ~/.fleetor/_shell` removes it (Tier-1 boundary).
fn lead_config_dir() -> PathBuf {
    fleet::shell_dir().join("lead-config")
}

/// Ensure `claude` (and node) are reachable even when the app was launched from a
/// GUI context whose PATH didn't inherit the login shell's additions.
fn augmented_path() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let existing = std::env::var("PATH").unwrap_or_default();
    format!("{home}/.local/bin:{home}/.bun/bin:/opt/homebrew/bin:/usr/local/bin:{existing}")
}

/// Write the lead's `fleet` MCP config: the shim, run with `FLEETOR_ROLE=lead` and
/// the fleet socket. Additive to the operator's own MCP servers (via
/// `--mcp-config`), so the lead keeps its normal tools and gains the fleet face.
fn write_lead_mcp_config() -> Result<PathBuf, String> {
    let dir = lead_config_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("create lead config dir: {e}"))?;
    let config = serde_json::json!({
        "mcpServers": {
            FLEET_MCP_KEY: {
                "type": "stdio",
                "command": fleet::shim_path(),
                "env": {
                    "FLEET_SOCKET": fleet::socket_path(),
                    "FLEETOR_ROLE": "lead",
                },
            }
        }
    });
    let path = dir.join(LEAD_MCP_CONFIG_FILE);
    std::fs::write(&path, serde_json::to_vec_pretty(&config).map_err(|e| e.to_string())?)
        .map_err(|e| format!("write lead MCP config: {e}"))?;
    Ok(path)
}

/// Spawn the lead `claude` in a pty of the given size. If a session already exists
/// it is left untouched and this is a no-op (the shell drives exactly one lead).
///
/// The fleet must be bootstrapped first (so the hub socket exists); the shim
/// retries the connection briefly, so a small ordering race is tolerated.
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

    let cwd = fleet::scratch_repo();
    std::fs::create_dir_all(&cwd).map_err(|e| format!("create lead cwd: {e}"))?;
    let mcp_config = write_lead_mcp_config()?;

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| format!("openpty: {e}"))?;

    let mut cmd = CommandBuilder::new("claude");
    cmd.cwd(&cwd);
    // Faithful lead TUI: inherit the operator's real environment (their login,
    // config, Opus) so this is literally their `claude`, then force a
    // truecolor-capable TERM.
    for (k, v) in std::env::vars() {
        cmd.env(k, v);
    }
    cmd.env("PATH", augmented_path());
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    // Fleet identity for the shim CC spawns as the MCP server.
    cmd.env("FLEETOR_ROLE", "lead");
    cmd.env("FLEET_SOCKET", fleet::socket_path());
    // Add the fleet MCP surface on top of the operator's config, expose the fleet
    // dir, and auto-approve the fleet tools so the lead drives without prompts.
    cmd.args(["--mcp-config", &mcp_config.to_string_lossy()]);
    cmd.args(["--add-dir", &fleet::shell_dir().to_string_lossy()]);
    cmd.args(["--allowedTools", &format!("mcp__{FLEET_MCP_KEY}")]);

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
