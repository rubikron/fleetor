//! FLEETOR Phase 0.5 pty spike — bare Tauri window hosting the real `claude`
//! TUI through portable-pty + xterm.js. Exit test: resize, colors, alternate
//! screen, scrollback, paste all behave. Throwaway-grade by charter; it seeds
//! `src-tauri` but carries no supervision logic.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod pty;

use pty::PtyState;
use tauri::{Manager, WindowEvent};

fn main() {
    tauri::Builder::default()
        .manage(PtyState::default())
        .invoke_handler(tauri::generate_handler![
            pty::pty_spawn,
            pty::pty_write,
            pty::pty_resize,
        ])
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { .. } = event {
                pty::kill_session(&window.state::<PtyState>());
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running fleetor pty spike");
}
