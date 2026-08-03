//! The window. Everything it does lives in the library beside it
//! (`src-tauri/src/lib.rs`), which is what makes the pane registry testable
//! without a GUI.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    fleetor_shell::run();
}
