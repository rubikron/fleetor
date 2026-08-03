//! FLEETOR desktop shell. A thin Tauri host over the proven core: it embeds the
//! fleet server (store + live event bus + hub) and streams events to the React
//! webview, and it owns the five ptys the fleet actually lives in.
//!
//! All supervision logic lives in `crates/`; this package is window + wiring only
//! (BUILDING §3, "src-tauri stays thin").
//!
//! The shell is a **library** with a one-line binary on top, so
//! `tests/panes.rs` can drive the pane registry over real ptys without a window.
//! A registry that could only be exercised through a GUI would be a registry
//! nothing tests.

pub mod deliver;
pub mod fleet;
pub mod pty;
pub mod spawn;
pub mod testbed;

use std::sync::Arc;

use fleet::FleetState;
use pty::PaneRegistry;
use tauri::{Emitter, Manager, WindowEvent};

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(FleetState::default())
        .setup(|app| {
            // The registry emits through a callback rather than holding an
            // `AppHandle`, which is what lets the tests drive five real ptys with
            // no window. This is the one place the two are joined.
            let handle = app.handle().clone();
            app.manage(Arc::new(PaneRegistry::new(Arc::new(
                move |channel: &str, payload: String| {
                    let _ = handle.emit(channel, payload);
                },
            ))));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            pty::pty_spawn,
            pty::pty_write,
            pty::pty_resize,
            pty::pty_kill,
            fleet::fleet_bootstrap,
            fleet::fleet_config,
            fleet::fleet_target,
            fleet::fleet_pick_target,
            fleet::fleet_set_target,
        ])
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { .. } = event {
                // Panes first: they are the processes that cost money.
                pty::kill_all(&window.state::<Arc<PaneRegistry>>());
                fleet::shutdown(&window.state::<FleetState>());
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running the fleetor shell");
}
