//! Placing a pane, driven entirely through the seam (WP-21, ticket 02).
//!
//! **Every test here runs against a scratch directory**, which is the whole point
//! of the module and the thing the old bring-up sequence made impossible: it
//! bottomed out in functions that read the operator's real `$HOME` at call time
//! and took no argument, so it could only ever be run against the operator's own
//! installation. Nothing in this file sets an environment variable, and nothing in
//! it touches `~/.fleetor` — the layout and the host both arrive as values.
//!
//! These assert what a *caller* can observe — the notices that came back, the
//! files that were written, the command that will be run — and never reach past
//! the interface to check how placement arranged its own work. The one place a
//! test reads an installed artifact rather than a return value is the guardrail's
//! `settings.json`, and that is deliberate: the roots are a fact about the file
//! the pane's Claude Code will actually load, not about a `Vec` placement held
//! briefly in memory.

use std::path::{Path, PathBuf};

use fleetor_core::event::NoticeLevel;
use fleetor_core::pane::PaneId;
use fleetor_shell::placement::{self, Host, Layout, PaneSpec};
use fleetor_shell::prompts::PaneContext;

/// A scratch root nothing else in the suite shares, holding a layout and a target
/// side by side. Both are handed to `place`, so between them they are the whole of
/// what a placement is allowed to touch.
struct Scratch {
    root: PathBuf,
    layout: Layout,
    target: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "fleetor-placement-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let target = root.join("repo");
        std::fs::create_dir_all(&target).unwrap();
        Self { layout: Layout::under(root.join("state")), root, target }
    }

    /// The shell command Claude Code will run for every matched tool call, read
    /// out of the `settings.json` placement just wrote for this pane.
    fn hook_command(&self, pane: PaneId) -> String {
        let settings = self.layout.pane_config(pane).join("settings.json");
        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        value["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
            .as_str()
            .expect("the hook command Claude Code will run")
            .to_string()
    }

    /// Every `--root` the installed hook was given, in order.
    fn guardrail_roots(&self, pane: PaneId) -> Vec<String> {
        let command = self.hook_command(pane);
        let parts: Vec<&str> = command.split(' ').collect();
        parts
            .windows(2)
            .filter(|w| w[0] == "--root")
            .map(|w| w[1].trim_matches('\'').to_string())
            .collect()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Every file and directory under `dir`, as paths relative to it.
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(next) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            found.push(path.strip_prefix(dir).unwrap().to_path_buf());
            if path.is_dir() {
                stack.push(path);
            }
        }
    }
    found.sort();
    found
}

/// A host with a `fleet` binary on it, so the missing-binary warning is out of the
/// way for the tests that are not about it. Everything else stays bare — this
/// machine has no rustup, no key and no stand-in pane program, and placing `orch`
/// must not care.
fn host_with_fleet_bin(at: &Path) -> Host {
    Host { fleet_bin: Some(at.to_path_buf()), ..Host::bare() }
}

// --- the guardrail roots --------------------------------------------------------

/// **The orchestrator writes in its target and in the fleet's own state root, and
/// that is all.**
///
/// Asserted through the file the pane's Claude Code will actually load rather than
/// through a value placement returned, and asserted against a *scratch* layout, so
/// what it pins is the rule rather than the operator's own directory names. The
/// evaluator's narrower roots are the same property one ticket later; this is the
/// first time either has had a test at all.
#[test]
fn the_orchestrator_may_write_in_its_target_and_the_fleets_own_state_root() {
    let scratch = Scratch::new("roots");
    let host = host_with_fleet_bin(&scratch.root.join("fleet"));

    placement::place(
        PaneSpec::Orch,
        &scratch.layout,
        &host,
        &scratch.target,
        &PaneContext::baked(),
    )
    .expect("placing orch against a scratch layout");

    let roots = scratch.guardrail_roots(PaneId::Orch);
    assert_eq!(
        roots,
        vec![
            scratch.target.display().to_string(),
            scratch.layout.shell().display().to_string(),
        ],
        "orch's roots are its cwd and `_shell`, in that order, and nothing else",
    );

    // The one directory inside the roots that is still off limits: every pane's
    // config dir, because that is where this guardrail's own rules live. It must
    // be the scratch layout's, not the operator's.
    let command = scratch.hook_command(PaneId::Orch);
    let policy = scratch.layout.shell().join("pane-config");
    assert!(
        command.contains(&policy.display().to_string()),
        "the deny list must be the scratch layout's pane-config: {command}",
    );
}

/// The operator's `[fence] allow` extras reach the orchestrator's roots — the
/// branch that separates `roots_for` from a hardcoded pair.
#[test]
fn the_operators_extra_write_roots_reach_the_orchestrator() {
    let scratch = Scratch::new("fence-allow");
    let extra = scratch.root.join("shared-cache");
    let mut context = PaneContext::baked();
    context.launch.fence_allow = vec![extra.display().to_string()];

    placement::place(
        PaneSpec::Orch,
        &scratch.layout,
        &host_with_fleet_bin(&scratch.root.join("fleet")),
        &scratch.target,
        &context,
    )
    .expect("placing orch with an extra allowed root");

    assert_eq!(
        scratch.guardrail_roots(PaneId::Orch).last().map(String::as_str),
        Some(extra.display().to_string().as_str()),
        "an operator's `[fence] allow` root is appended after cwd and `_shell`",
    );
}

// --- the notices ----------------------------------------------------------------

/// **What the operator is told when the orchestrator comes up, and in what order.**
///
/// Notices are returned as data rather than written to the log from inside
/// placement, which is what makes this assertable at all — before the seam the
/// only way to see them was to run the application and read the Activity feed.
#[test]
fn placing_the_orchestrator_returns_the_notices_the_operator_should_read() {
    let scratch = Scratch::new("notices");

    let placed = placement::place(
        PaneSpec::Orch,
        &scratch.layout,
        &host_with_fleet_bin(&scratch.root.join("fleet")),
        &scratch.target,
        &PaneContext::baked(),
    )
    .expect("placing orch against a scratch layout");

    let texts: Vec<&str> = placed.notices.iter().map(|(_, t)| t.as_str()).collect();
    assert_eq!(
        placed.notices.len(),
        2,
        "a healthy machine gets exactly the loadout line and the config-dir line: {texts:#?}",
    );

    // The WP-04 spawn-time "Loadout" counter comes first: the size of the brief
    // this pane was just handed.
    assert_eq!(placed.notices[0].0, NoticeLevel::Info);
    assert!(
        placed.notices[0].1.contains("orch") && placed.notices[0].1.contains("token"),
        "the loadout line names the pane and its brief size: {}",
        placed.notices[0].1,
    );

    // Then the one notice standing between the operator and a pane that looks
    // perfectly healthy while being logged out (WP-14, D-062).
    assert_eq!(placed.notices[1].0, NoticeLevel::Info);
    let config_dir = scratch.layout.pane_config(PaneId::Orch);
    assert!(placed.notices[1].1.contains(&config_dir.display().to_string()));
    assert!(placed.notices[1].1.contains("Not logged in"));
    assert!(placed.notices[1].1.contains("/login"));

    // `orch` is not on the live gauge — the Loadout counter is a per-run budget
    // line for the panes doing the work, and the gauge samples worker transcripts.
    assert!(placed.gauge.is_none(), "orch records no transcript source");
}

/// **A machine with no `fleet` binary is warned about, once, loudly.**
///
/// A pane with no `fleet` on its PATH looks alive and cannot talk, and the model
/// would otherwise discover it as `command not found` mid-turn (L4). The bare host
/// is what makes this branch a case a test can express rather than a branch only
/// an operator with a broken install ever reaches.
#[test]
fn a_machine_with_no_fleet_binary_is_warned_about_first() {
    let scratch = Scratch::new("no-fleet-bin");

    let placed = placement::place(
        PaneSpec::Orch,
        &scratch.layout,
        &Host::bare(),
        &scratch.target,
        &PaneContext::baked(),
    )
    .expect("a machine with no `fleet` binary still places orch");

    assert_eq!(
        placed.notices[0],
        (NoticeLevel::Warn, placement::MISSING_FLEET_BIN.to_string()),
        "the warning comes before anything else the operator reads",
    );
    assert!(
        placed.notices[0].1.contains("cargo build -p fleetor-cli"),
        "and it names the fix",
    );
}

// --- containment ----------------------------------------------------------------

/// **Placing against a scratch layout writes inside it and nowhere else.**
///
/// This is the property the whole module exists for. Two claims, because one of
/// them is empirical and the other is structural and they fail differently:
///
///  1. Every path that appeared under the scratch root is under the layout it was
///     handed or under the target it was handed — the two arguments that name
///     somewhere placement is allowed to write.
///  2. Every path placement *decided on* — the config dir it seeded, the guardrail
///     policy dir and journal it pointed the hook at, the socket it put on the
///     command — is under that layout. A placement that wrote nothing but pointed
///     the pane at the operator's real directory would pass the first check and
///     fail this one.
#[test]
fn placing_against_a_scratch_layout_writes_inside_it_and_nowhere_else() {
    let scratch = Scratch::new("containment");

    // A sibling of both, created up front, so an escape into the scratch root
    // itself is visible as a new entry beside it rather than as an absence.
    let canary = scratch.root.join("canary");
    std::fs::create_dir_all(&canary).unwrap();

    placement::place(
        PaneSpec::Orch,
        &scratch.layout,
        &host_with_fleet_bin(&scratch.root.join("fleet")),
        &scratch.target,
        &PaneContext::baked(),
    )
    .expect("placing orch against a scratch layout");

    for path in walk(&scratch.root) {
        assert!(
            path.starts_with("state") || path.starts_with("repo") || path == Path::new("canary"),
            "{} is outside both the layout and the target placement was handed",
            path.display(),
        );
    }
    assert_eq!(walk(&canary), Vec::<PathBuf>::new(), "nothing was written beside the layout");

    // The seed and the policy really did land, so the check above is not passing
    // by virtue of placement having done nothing.
    let config_dir = scratch.layout.pane_config(PaneId::Orch);
    assert!(config_dir.join(".claude.json").is_file(), "the L1 config seed");
    assert!(config_dir.join("settings.json").is_file(), "the guardrail policy");
    assert!(config_dir.join("write-guardrail.py").is_file(), "the guardrail hook itself");

    // And every path it pointed the pane at is the scratch layout's.
    let command = scratch.hook_command(PaneId::Orch);
    for named in [scratch.layout.shell().join("pane-config"), scratch.layout.shell().join("guardrail.jsonl")]
    {
        assert!(
            command.contains(&named.display().to_string()),
            "the hook must name {}, not the operator's own: {command}",
            named.display(),
        );
    }
}

/// The config seed is keyed by the exact working directory the pane will run in
/// (L1) — `hasTrustDialogAccepted` is per absolute project path, so a seed written
/// for the wrong directory is a pane that opens onto a trust dialog and accepts
/// every message into it.
#[test]
fn the_config_seed_is_keyed_to_the_target_the_pane_will_run_in() {
    let scratch = Scratch::new("seed-key");

    placement::place(
        PaneSpec::Orch,
        &scratch.layout,
        &host_with_fleet_bin(&scratch.root.join("fleet")),
        &scratch.target,
        &PaneContext::baked(),
    )
    .expect("placing orch against a scratch layout");

    let seed = scratch.layout.pane_config(PaneId::Orch).join(".claude.json");
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&seed).unwrap()).unwrap();
    assert_eq!(value["hasCompletedOnboarding"], serde_json::json!(true));

    let key = std::fs::canonicalize(&scratch.target).unwrap().display().to_string();
    assert_eq!(
        value["projects"][&key]["hasTrustDialogAccepted"],
        serde_json::json!(true),
        "the trust flag must be under the resolved target path: {value:#}",
    );
}
