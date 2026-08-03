//! FLEETOR desktop shell (Phase 4e). A thin Tauri host over the proven core:
//! it embeds the fleet server (store + live event bus + hub) and streams events
//! to the React webview, while keeping the Phase 0.5 pty bridge for the
//! orchestrator's real `claude` TUI. All supervision logic lives in `crates/`;
//! this binary is window + wiring only (BUILDING §3, "src-tauri stays thin").
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod fleet;
mod pty;
mod testbed;

use fleet::FleetState;
use pty::PtyState;
use tauri::{Manager, WindowEvent};

fn main() {
    tauri::Builder::default()
        .manage(PtyState::default())
        .manage(FleetState::default())
        .invoke_handler(tauri::generate_handler![
            pty::pty_spawn,
            pty::pty_write,
            pty::pty_resize,
            fleet::fleet_bootstrap,
            fleet::fleet_board,
            fleet::fleet_assign,
            fleet::fleet_config,
        ])
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { .. } = event {
                pty::kill_session(&window.state::<PtyState>());
                fleet::shutdown(&window.state::<FleetState>());
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running the fleetor shell");
}
