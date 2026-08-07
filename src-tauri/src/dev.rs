//! **Dev mode** (WP-16) — an operator-triggered application posture, loud in the
//! UI, and the container the rest of the self-improvement arc lives inside.
//!
//! On its own this does almost nothing, and that is the design. WP-15's
//! evaluator exists *only* in dev mode and WP-17's fence is scoped *by* it, so
//! what this package owes them is one flag with one spelling, one home, and one
//! way to read it. Everything else about the arc is deliberately not here.
//!
//! **One source of truth: `dev_mode` in `~/.fleetor/config.json`.** Not
//! `localStorage` (where the theme and the nav selection live) — those are
//! webview preferences nothing on the Rust side ever asks about, and dev mode
//! has to be readable by code that has no webview. Not a second file either: a
//! mode with its own file is a mode the operator has to be told where to find.
//! Under `~/.fleetor` because everything runtime is (Tier 1.1) — `rm -rf
//! ~/.fleetor` turns dev mode off along with everything else.
//!
//! **Read fresh, every time.** [`is_enabled`] re-reads the file rather than
//! caching a boot-time snapshot, so a toggle takes effect the moment it is
//! flipped and there is no second copy to go stale. The target is snapshotted at
//! bootstrap for the opposite reason (half a fleet in one repo and half in
//! another is incoherent); a mode has no such half-state today, and when WP-15
//! gives it one — a window that exists or does not — that window's lifecycle is
//! WP-15's to decide, not something this file should pre-empt by caching.
//!
//! **Tier 1.4: nothing here is on the message path.** No module between
//! `fleet send` and a pty imports this one — not `deliver`, not `pty`, not the
//! hub, not the CLI — and `tests/dev_mode.rs` fails if that ever stops being
//! true. A delivery path that could read a mode is a delivery path that could
//! one day branch on one, and that is the refusal Tier 1.4 forbids.
//!
//! **`orch` is not told.** Dev mode is absent from `prompts/`, from `brief`, and
//! from `fleet roster`; the same test pins that. The mode is the *operator's*
//! posture, and WP-12's open question 4 takes the cheap answer: the veil is
//! cheaper to keep than to re-establish.

use std::path::Path;

use crate::fleet;

/// The config key. One spelling, referenced by every reader and the tripwire.
pub const CONFIG_KEY: &str = "dev_mode";

/// Is the app in dev mode? **The one read.** Anything that needs to branch on
/// the mode calls this rather than reaching for the config file itself.
///
/// A missing, unreadable or malformed config reads as *off*. Off is the state
/// with no evaluator, no second window and no fence — failing closed costs an
/// operator one click and failing open would silently put a fleet in a posture
/// nobody chose.
pub fn is_enabled() -> bool {
    read_at(&fleet::config_path())
}

/// Turn dev mode on or off, persistently. Returns what is now stored, which is
/// what the caller should render — never the value it asked for. Writing and
/// then reading back is what makes "it persisted" a fact rather than a hope.
pub fn set_enabled(enabled: bool) -> Result<bool, String> {
    write_at(&fleet::config_path(), enabled)
}

/// [`is_enabled`] against a named config file.
fn read_at(file: &Path) -> bool {
    parse_dev_mode(&std::fs::read_to_string(file).unwrap_or_default())
}

/// [`set_enabled`] against a named config file — the seam the round-trip test
/// uses so persistence is proven in a temp directory, not asserted about the
/// operator's real home.
fn write_at(file: &Path, enabled: bool) -> Result<bool, String> {
    fleet::write_config_key_at(file, CONFIG_KEY, enabled.into())?;
    Ok(read_at(file))
}

/// Pull the flag out of config text. Pure, so the whole truth table is unit
/// tested without touching the operator's home directory.
///
/// Only a real JSON `true` is on. A string `"true"`, a `1`, a missing key and
/// text that is not JSON at all are all off — a mode this loud must be
/// something the operator switched on, never something a typo produced.
fn parse_dev_mode(text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| v.get(CONFIG_KEY).and_then(|f| f.as_bool()))
        .unwrap_or(false)
}

// --- commands -----------------------------------------------------------------
//
// Two, and neither touches `FleetState`: dev mode is an app posture, not a
// property of a running fleet, so both work on the start gate — which is where
// an operator deciding what this session is for actually stands.

/// Whether the app is in dev mode, for the banner and the settings switch.
#[tauri::command]
pub fn dev_mode_get() -> bool {
    is_enabled()
}

/// Persist the operator's choice and answer with the stored state.
#[tauri::command]
pub fn dev_mode_set(enabled: bool) -> Result<bool, String> {
    set_enabled(enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The requirement, on a real file.** The flag the switch writes is the
    /// flag the next launch reads — a fresh `read_at` is precisely what a
    /// relaunched app does, since nothing caches the mode. Both directions,
    /// because a mode that cannot be turned back off is a trap.
    #[test]
    fn dev_mode_survives_a_restart_in_both_directions() {
        let dir = std::env::temp_dir().join(format!("fleetor-dev-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("config.json");

        assert!(!read_at(&file), "a first run, with no config at all, is off");

        assert!(write_at(&file, true).unwrap(), "the write reports what it stored");
        assert!(read_at(&file), "and a cold read — a relaunch — agrees");

        assert!(!write_at(&file, false).unwrap());
        assert!(!read_at(&file), "off persists exactly as on does");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Turning the mode on must not cost the operator the repo they pointed the
    /// fleet at — one config file, two settings, and neither clobbers the other.
    #[test]
    fn toggling_dev_mode_leaves_the_target_alone_and_vice_versa() {
        let with_target = fleet::merge_config_key(None, "target", "/Users/me/code".into()).unwrap();
        let with_both =
            fleet::merge_config_key(Some(&with_target), CONFIG_KEY, true.into()).unwrap();
        let config: serde_json::Value = serde_json::from_str(&with_both).unwrap();
        assert_eq!(config["target"], serde_json::json!("/Users/me/code"));
        assert!(parse_dev_mode(&with_both), "{with_both}");

        let target_again =
            fleet::merge_config_key(Some(&with_both), "target", "/Users/me/other".into()).unwrap();
        assert!(parse_dev_mode(&target_again), "re-picking a target kept the mode: {target_again}");
    }

    /// Absence is off. A first run has no config at all, and the operator has
    /// not asked for anything by not having asked.
    #[test]
    fn no_config_and_no_key_are_both_off() {
        assert!(!parse_dev_mode(""));
        assert!(!parse_dev_mode("{}"));
        assert!(!parse_dev_mode(r#"{"target": "/Users/me/code"}"#));
    }

    /// A config nobody can parse must not read as *on*. The failure this guards
    /// is an operator in an evaluation posture they never chose — the exact
    /// thing the banner exists to make impossible.
    #[test]
    fn a_malformed_or_lookalike_flag_is_off_not_on() {
        assert!(!parse_dev_mode("not json at all"));
        assert!(!parse_dev_mode("[]"));
        assert!(!parse_dev_mode(r#"{"dev_mode": "true"}"#), "a string is not the flag");
        assert!(!parse_dev_mode(r#"{"dev_mode": 1}"#), "a number is not the flag");
        assert!(!parse_dev_mode(r#"{"devMode": true}"#), "one spelling, and this is not it");
    }

    /// The only value that turns it on.
    #[test]
    fn a_real_json_true_is_the_one_thing_that_turns_it_on() {
        assert!(parse_dev_mode(r#"{"dev_mode": true}"#));
        assert!(!parse_dev_mode(r#"{"dev_mode": false}"#));
    }
}
