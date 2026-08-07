//! Where a pane's prompts and launch settings actually come from (D-042).
//!
//! Every word a pane is told, and every flag it is launched with, lives in
//! `prompts/` at the repo root as an ordinary file. Those files are baked into
//! the binary with `include_str!` (see `fleetor_core::brief`), so the app always
//! has a working brief and the tests can pin it. This module adds the second
//! rung: the operator's own copies in `~/.fleetor/prompts/`, read at bootstrap.
//!
//! Three rules, all of them about the same failure — an override that does not
//! do what the operator thinks it did:
//!
//!  - **An override that loads is announced.** A prompt change that silently did
//!    nothing is indistinguishable from a prompt change that did not work, and
//!    the operator would be reading a live fleet's behaviour for a brief it is
//!    not running.
//!  - **An override that is broken is refused, loudly, and the built-in is used.**
//!    Never a half-applied brief: `fleetor_core::brief::validate_*` decides, and
//!    its complaint goes on the Activity feed verbatim.
//!  - **An unknown key in `launch.conf` is a warning, not a silent skip.** A
//!    typo'd `permission_mode` reads exactly like a working config until a pane
//!    wedges on its first tool call.
//!
//! Resolution happens **once, at bootstrap**, not per spawn — a fleet whose panes
//! were briefed from two different revisions of a file the operator was editing
//! is not a fleet anyone can reason about. Editing takes effect on next launch,
//! the same rule `fleet_pick_target` follows for the target.

use std::path::{Path, PathBuf};

use fleetor_core::brief::{
    validate_orch, validate_worker, DEFAULT_ORCH, DEFAULT_WORKER,
};
use fleetor_core::event::NoticeLevel;

/// The `claude` flags and endpoint a worker is launched with, from
/// `prompts/launch.conf`. The orchestrator takes none of these on purpose — it is
/// the operator's own `claude` and keeps their model, login and permission mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchConfig {
    pub worker_model: String,
    pub worker_base_url: String,
    pub worker_permission_mode: String,
    /// WP-08, the Fence: `open` is the only posture that exists — see
    /// `prompts/launch.conf`'s `[fence]` section for why the key exists anyway.
    /// Nothing in this codebase branches on it yet; it is a recorded default,
    /// not a gate.
    pub fence_posture: String,
}

/// The baked-in `prompts/launch.conf`, parsed at startup. Shipping the file *and*
/// parsing it (rather than keeping Rust constants beside it) is what stops the two
/// from drifting: there is one set of values and the docs are it.
const DEFAULT_LAUNCH: &str = include_str!("../../prompts/launch.conf");

impl Default for LaunchConfig {
    /// The last-resort values, used only if the baked-in `launch.conf` is somehow
    /// unparseable. They match what ships in that file; [`baked_launch_matches_the_shipped_conf`]
    /// pins them together.
    fn default() -> Self {
        Self {
            worker_model: "deepseek-v4-flash".to_string(),
            worker_base_url: "https://api.deepseek.com/anthropic".to_string(),
            worker_permission_mode: "auto".to_string(),
            fence_posture: "open".to_string(),
        }
    }
}

/// Everything resolved: the two brief templates and the launch settings, plus what
/// to say about how they were arrived at.
#[derive(Debug, Clone)]
pub struct PaneContext {
    pub orch_template: String,
    pub worker_template: String,
    pub launch: LaunchConfig,
    /// Announcements for the Activity feed, in the order they happened. Carried
    /// rather than emitted so this module stays testable without a store.
    pub notices: Vec<(NoticeLevel, String)>,
}

impl PaneContext {
    /// What ships, with no operator overrides considered. The fallback for every
    /// failure path, and what `src-tauri/tests` runs against.
    pub fn baked() -> Self {
        let (launch, complaints) = apply_conf(&LaunchConfig::default(), DEFAULT_LAUNCH);
        Self {
            orch_template: DEFAULT_ORCH.to_string(),
            worker_template: DEFAULT_WORKER.to_string(),
            launch,
            // A complaint here is a bug in what we shipped, not in what the
            // operator wrote, so it is an error rather than a warning.
            notices: complaints
                .into_iter()
                .map(|c| (NoticeLevel::Error, format!("built-in launch.conf: {c}")))
                .collect(),
        }
    }

    /// Resolve against an override directory — `~/.fleetor/prompts/` in the app,
    /// a temp dir in the tests. A directory that does not exist is the ordinary
    /// first-run case and says nothing.
    pub fn resolve(dir: &Path) -> Self {
        let baked = Self::baked();
        let (orch_template, orch_notes) =
            load_template(dir, "orch.md", baked.orch_template, validate_orch);
        let (worker_template, worker_notes) =
            load_template(dir, "worker.md", baked.worker_template, validate_worker);
        let (launch, launch_notes) = load_launch(dir, &baked.launch);

        Self {
            orch_template,
            worker_template,
            launch,
            notices: baked
                .notices
                .into_iter()
                .chain(orch_notes)
                .chain(worker_notes)
                .chain(launch_notes)
                .collect(),
        }
    }
}

// --- the brief templates ------------------------------------------------------

type Note = (NoticeLevel, String);

/// Read one override template, or keep `fallback`. Validation is the caller's
/// (`validate_orch` / `validate_worker`) so this function has no opinion about
/// what a usable brief is — it only decides *whether* to use what it read.
fn load_template(
    dir: &Path,
    name: &str,
    fallback: String,
    validate: fn(&str) -> Result<(), String>,
) -> (String, Vec<Note>) {
    let path = dir.join(name);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return (fallback, Vec::new()); // no override; the ordinary case
    };

    match validate(&text) {
        Ok(()) => (
            text,
            vec![(NoticeLevel::Info, format!("using your {}", path.display()))],
        ),
        Err(why) => (
            fallback,
            vec![(
                NoticeLevel::Warn,
                format!("ignoring {} — {why}. The built-in brief is being used instead.", path.display()),
            )],
        ),
    }
}

// --- launch.conf --------------------------------------------------------------

fn load_launch(dir: &Path, fallback: &LaunchConfig) -> (LaunchConfig, Vec<Note>) {
    let path = dir.join("launch.conf");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return (fallback.clone(), Vec::new());
    };

    let (config, complaints) = apply_conf(fallback, &text);
    let mut notes: Vec<Note> = complaints
        .into_iter()
        .map(|c| (NoticeLevel::Warn, format!("{}: {c}", path.display())))
        .collect();
    if &config != fallback {
        notes.insert(0, (NoticeLevel::Info, format!("using your {}", path.display())));
    }
    (config, notes)
}

/// Apply `key = value` lines under `[section]` headers onto `base`, returning a
/// new config and a complaint for every line that did nothing.
///
/// `base` is never touched. Every unrecognised line produces a complaint rather
/// than being skipped: a setting that looks applied and is not is the whole
/// failure mode this file exists to avoid.
fn apply_conf(base: &LaunchConfig, text: &str) -> (LaunchConfig, Vec<String>) {
    let mut config = base.clone();
    let mut complaints = Vec::new();
    let mut section = String::new();

    for (index, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        let at = index + 1;
        if line.is_empty() {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = name.trim().to_ascii_lowercase();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            complaints.push(format!("line {at}: `{line}` is not `key = value`"));
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim().to_string();
        if value.is_empty() {
            complaints.push(format!("line {at}: `{key}` has no value"));
            continue;
        }
        match (section.as_str(), key.as_str()) {
            ("worker", "model") => config.worker_model = value,
            ("worker", "base_url") => config.worker_base_url = value,
            ("worker", "permission_mode") => config.worker_permission_mode = value,
            // Only `open` is implemented (WP-08) — a Posture Ladder with other
            // values was proposed and rejected upstream. A value this parser
            // doesn't recognise falls back to `open` rather than being stored:
            // a posture that silently did nothing would read as hardening that
            // never happened.
            ("fence", "posture") if value == "open" => config.fence_posture = value,
            ("fence", "posture") => complaints.push(format!(
                "line {at}: `fence.posture = {value}` is not a supported posture — only `open` exists; using `open`"
            )),
            ("", _) => complaints.push(format!("line {at}: `{key}` is before any [section] header")),
            _ => complaints.push(format!("line {at}: `[{section}] {key}` is not a setting")),
        }
    }
    (config, complaints)
}

/// `~/.fleetor/prompts` — where the operator puts their own copies.
pub fn override_dir(fleetor_dir: &Path) -> PathBuf {
    fleetor_dir.join("prompts")
}

#[cfg(test)]
mod tests {
    use super::*;
    use fleetor_core::brief::{render_worker, DEFAULT_WORKER};
    use fleetor_core::pane::{PaneId, WORKER_SLOTS};

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "fleetor-prompts-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The values in `prompts/launch.conf` are the real ones — not a copy of them
    /// living in Rust that the file drifts away from.
    #[test]
    fn baked_launch_matches_the_shipped_conf() {
        let baked = PaneContext::baked();
        assert!(baked.notices.is_empty(), "the shipped launch.conf does not parse: {:?}", baked.notices);
        assert_eq!(baked.launch, LaunchConfig::default(), "launch.conf drifted from the fallback");
    }

    /// The ordinary first run: nothing configured, nothing said, the built-ins used.
    #[test]
    fn no_override_directory_is_silent() {
        let ctx = PaneContext::resolve(&PathBuf::from("/nonexistent/fleetor/prompts"));
        assert_eq!(ctx.orch_template, DEFAULT_ORCH);
        assert_eq!(ctx.worker_template, DEFAULT_WORKER);
        assert_eq!(ctx.launch, LaunchConfig::default());
        assert!(ctx.notices.is_empty(), "a first run must not warn about anything: {:?}", ctx.notices);
    }

    /// A valid override is used *and announced* — a prompt change that quietly did
    /// nothing is the failure this notice exists to prevent.
    #[test]
    fn a_valid_override_is_used_and_announced() {
        let dir = temp_dir("valid");
        let mine = "# {me} in {cwd}\n\nyou work with {peers}. verbs: fleet send, fleet broadcast, \
             fleet reply, fleet cmd, fleet task, fleet done, fleet handoff, fleet roster, \
             fleet whoami.\n\n{delivery_contract}\n\n{broadcast_rule}\n\n{scaffolding}\n";
        std::fs::write(dir.join("worker.md"), mine).unwrap();

        let ctx = PaneContext::resolve(&dir);
        assert_eq!(ctx.worker_template, mine, "the operator's template is what gets rendered");
        assert_eq!(ctx.orch_template, DEFAULT_ORCH, "an orch override was not written, so it is untouched");
        assert!(
            ctx.notices.iter().any(|(l, t)| *l == NoticeLevel::Info && t.contains("worker.md")),
            "the operator is told their override took effect: {:?}",
            ctx.notices
        );

        // And it really does reach a rendered brief, fragments intact.
        let brief =
            render_worker(&ctx.worker_template, PaneId::Worker(2), &PaneId::roster(&WORKER_SLOTS), "/tmp/wt");
        assert!(brief.contains("you work with"));
        assert!(brief.contains("exits non-zero"), "the delivery contract survived the override");
        assert!(brief.contains("Never reply to a broadcast unless it names you"), "L5 survived");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A broken override must never half-apply. The built-in is used, and the
    /// reason reaches the operator rather than only stderr.
    #[test]
    fn a_broken_override_falls_back_and_says_why() {
        let dir = temp_dir("broken");
        // Everything a brief needs except the fragment it cannot afford to lose.
        std::fs::write(dir.join("worker.md"), "# {me} in {cwd}\n\nyou work with {peers}. no fragments.\n")
            .unwrap();

        let ctx = PaneContext::resolve(&dir);
        assert_eq!(ctx.worker_template, DEFAULT_WORKER, "a pane must never run a half-valid brief");
        let (level, text) = ctx.notices.iter().find(|(_, t)| t.contains("worker.md")).expect("told");
        assert_eq!(*level, NoticeLevel::Warn);
        assert!(text.contains("{delivery_contract}"), "the complaint names what to fix: {text}");
        assert!(text.contains("built-in"), "and says what is running instead: {text}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Only the keys present are overridden; the rest keep the shipped values.
    #[test]
    fn launch_conf_overrides_only_what_it_sets() {
        let dir = temp_dir("launch");
        std::fs::write(dir.join("launch.conf"), "[worker]\nmodel = my-model\n").unwrap();

        let ctx = PaneContext::resolve(&dir);
        assert_eq!(ctx.launch.worker_model, "my-model");
        assert_eq!(ctx.launch.worker_base_url, LaunchConfig::default().worker_base_url, "untouched");
        assert_eq!(ctx.launch.worker_permission_mode, "auto", "untouched");
        assert!(ctx.notices.iter().any(|(l, _)| *l == NoticeLevel::Info));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A typo'd key reads exactly like a working config until a pane wedges. It
    /// has to be a warning, and it must not stop the keys that *were* right.
    #[test]
    fn an_unknown_setting_is_reported_rather_than_skipped() {
        let dir = temp_dir("typo");
        std::fs::write(
            dir.join("launch.conf"),
            "[worker]\nmodel = good\npermision_mode = auto\nnot a pair\nbase_url =\n",
        )
        .unwrap();

        let ctx = PaneContext::resolve(&dir);
        assert_eq!(ctx.launch.worker_model, "good", "a bad line must not discard the good ones");

        let warnings: Vec<&String> =
            ctx.notices.iter().filter(|(l, _)| *l == NoticeLevel::Warn).map(|(_, t)| t).collect();
        assert_eq!(warnings.len(), 3, "one per unusable line: {warnings:#?}");
        assert!(warnings.iter().any(|w| w.contains("permision_mode")), "{warnings:#?}");
        assert!(warnings.iter().any(|w| w.contains("is not `key = value`")), "{warnings:#?}");
        assert!(warnings.iter().any(|w| w.contains("has no value")), "{warnings:#?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Comments and blank lines are ordinary in a file meant to be edited by hand.
    #[test]
    fn comments_and_blank_lines_are_not_complaints() {
        let (config, complaints) = apply_conf(
            &LaunchConfig::default(),
            "# a comment\n\n[worker]\n\nmodel = m  # trailing note\n",
        );
        assert!(complaints.is_empty(), "{complaints:#?}");
        assert_eq!(config.worker_model, "m", "the trailing comment is not part of the value");
    }

    /// A setting outside any section is a mistake with a specific fix, so it gets
    /// a specific complaint.
    #[test]
    fn a_setting_before_any_section_is_named_as_such() {
        let (_, complaints) = apply_conf(&LaunchConfig::default(), "model = orphan\n");
        assert_eq!(complaints.len(), 1);
        assert!(complaints[0].contains("before any [section]"), "{complaints:#?}");
    }

    /// WP-08's own knob: `open` is the only posture that exists, and setting it
    /// explicitly must round-trip cleanly — this key is a no-op today, but a
    /// no-op that fails to parse would be a worse first impression than one that
    /// silently did nothing.
    #[test]
    fn fence_posture_open_parses_with_no_complaint() {
        let (config, complaints) = apply_conf(&LaunchConfig::default(), "[fence]\nposture = open\n");
        assert!(complaints.is_empty(), "{complaints:#?}");
        assert_eq!(config.fence_posture, "open");
    }

    /// A posture this parser doesn't implement must not read as if it took
    /// effect: no Posture Ladder exists yet, so anything other than `open`
    /// falls back to `open` and says so loudly rather than being stored as a
    /// setting nothing honors.
    #[test]
    fn an_unsupported_posture_falls_back_to_open_and_warns() {
        let (config, complaints) = apply_conf(&LaunchConfig::default(), "[fence]\nposture = locked-down\n");
        assert_eq!(config.fence_posture, "open", "an unimplemented posture must not silently \"take effect\"");
        assert_eq!(complaints.len(), 1);
        assert!(complaints[0].contains("locked-down"), "{complaints:#?}");
        assert!(complaints[0].contains("only `open` exists"), "{complaints:#?}");
    }
}
