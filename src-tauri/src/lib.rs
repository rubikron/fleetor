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

pub mod context_gauge;
pub mod deliver;
pub mod fleet;
mod orphans;
pub mod prompts;
pub mod pty;
pub mod runs;
pub mod spawn;
pub mod testbed;

use std::sync::Arc;

use fleet::FleetState;
use pty::PaneRegistry;
use tauri::{Emitter, Manager, WindowEvent};

/// How long the shell waits for the frontend to size and reveal the window
/// before doing it anyway. Long enough for a cold webview to boot and apply
/// geometry, short enough that a broken frontend does not look like an app
/// that failed to launch.
const WINDOW_REVEAL_FALLBACK_MS: u64 = 2500;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(FleetState::default())
        .setup(|app| {
            // Before this session spawns anything of its own: reap whatever a
            // crash or Force Quit left running from the last one. The close
            // handlers below are best-effort and can only run if the process
            // gets a chance to — this is what bounds a missed teardown to "one
            // session" instead of "forever" (`orphans.rs`).
            orphans::sweep();

            // The registry emits through a callback rather than holding an
            // `AppHandle`, which is what lets the tests drive five real ptys with
            // no window. This is the one place the two are joined.
            let handle = app.handle().clone();
            app.manage(Arc::new(PaneRegistry::new(
                Arc::new(move |channel: &str, payload: String| {
                    let _ = handle.emit(channel, payload);
                }),
                orphans::registry_path(),
            )));

            // The window is created hidden (`visible: false` in tauri.conf.json)
            // so the frontend can apply the saved geometry before it is ever
            // seen — otherwise it opens at the configured default and visibly
            // jumps to the remembered size.
            //
            // This is the safety net for that. If the frontend never gets far
            // enough to reveal it — a bundle that fails to load, a render that
            // throws — the window must still appear, or the app is simply
            // invisible with no way to tell it even started. Showing twice is
            // harmless; never showing is not.
            let reveal = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(WINDOW_REVEAL_FALLBACK_MS));
                if let Some(window) = reveal.get_webview_window("main") {
                    let _ = window.show();
                }
            });

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
            fleet::fleet_roster,
            fleet::fleet_send,
            fleet::runs_list,
            fleet::run_events,
            fleet::run_rename,
            fleet::run_delete,
            fleet::run_export,
        ])
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { .. } = event {
                teardown_fleet(window);
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building the fleetor shell")
        .run(|app_handle, event| {
            // macOS Cmd+Q and Dock → Quit skip `WindowEvent::CloseRequested`
            // entirely (tauri-apps/tauri#9198, #13778) — the handler above never
            // runs for them. `RunEvent::Exit` is the one hook that still fires
            // before the process actually exits regardless of *how* it was
            // asked to quit, so it is the real teardown; the window-close
            // handler above is just the fast path for the common case. Calling
            // both is safe — `kill_all` and `fleet::shutdown` are idempotent on
            // an already-torn-down fleet.
            if let tauri::RunEvent::Exit = event {
                teardown_fleet(app_handle);
            }
        });
}

/// Panes first: they are the processes that cost money.
fn teardown_fleet(handle: &impl Manager<tauri::Wry>) {
    pty::kill_all(&handle.state::<Arc<PaneRegistry>>());
    fleet::shutdown(&handle.state::<FleetState>());
}
