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
use fleetor_shell::placement::{self, Host, Layout, PaneSpec, RunSource};
use fleetor_shell::prompts::PaneContext;

/// The whole-installation fixture, shared with `tests/write_guardrail.rs` rather
/// than retyped in both — see that module for what the four preconditions are.
///
/// **Ungated since WP-20** (D-076). It was `#[cfg(feature = "devmode")]` because
/// the only pane that needed a whole installation was the evaluator, which no
/// default build has. The Critic needs one too — it works in a snapshot of a live
/// run, so a real store has to exist for placement to snapshot — and it exists in
/// every build, so the fixture cannot be gated on a feature it does not use. The
/// module carries `#![allow(dead_code)]`, so the mission-only helpers cost a
/// default build nothing.
mod common;

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

impl Scratch {
    /// A second target beside the first, for the target-switch case.
    fn other_target(&self, name: &str) -> PathBuf {
        let dir = self.root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Make `dir` a real repository with one commit — what `git worktree add` needs
/// before it will succeed.
///
/// **Shelling out to git is in keeping**: this suite already spawns real ptys and
/// runs the real guardrail hook through a real shell, and the worker's successful
/// path *is* a `git worktree add`. Faking it would prove nothing about the branch
/// that actually runs. Identity and signing are pinned per-command so the result
/// does not depend on the machine's own git configuration.
fn init_repo(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("README.md"), "scratch\n").unwrap();
    run_git(dir, &["init", "-q", "-b", "main"]);
    run_git(dir, &["add", "-A"]);
    run_git(dir, &["commit", "-q", "-m", "first"]);
}

/// One git command that must succeed, with the machine's own identity, signing and
/// hooks deliberately out of the way.
fn run_git(dir: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.email=scratch@fleetor.test"])
        .args(["-c", "user.name=Scratch"])
        .args(["-c", "commit.gpgsign=false"])
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(args)
        .output()
        .expect("git must be on PATH for the worktree tests");
    assert!(
        output.status.success(),
        "git {args:?} in {} failed: {}",
        dir.display(),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Every branch in `repo`, by short name.
fn branches(repo: &Path) -> Vec<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["branch", "--list", "--format=%(refname:short)"])
        .output()
        .expect("git must be on PATH for the worktree tests");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// A host a worker can actually be placed against: a `fleet` binary and a key.
///
/// Everything else stays bare — **no toolchain**, which is the machine the
/// absent-settings test is about, and no stand-in program, so the command really is
/// the `claude` one production builds.
fn host_for_worker(fleet_bin: &Path) -> Host {
    Host {
        fleet_bin: Some(fleet_bin.to_path_buf()),
        api_key: Some(WORKER_KEY.to_string()),
        ..Host::bare()
    }
}

/// The worker credential these tests hand the host, distinctive enough that finding
/// it on the command proves it travelled rather than coincided.
const WORKER_KEY: &str = "sk-fleetor-scratch-worker-key";

/// What `place` put in the command's environment, as an owned string.
fn env_on(command: &portable_pty::CommandBuilder, key: &str) -> Option<String> {
    command.get_env(key).map(|v| v.to_string_lossy().to_string())
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

// --- what the machine is missing ------------------------------------------------

/// The refusal a worker gets on a machine with no key **names the directory the
/// `.env` walk started from** (D-075).
///
/// It is the operator-actionable half of the sentence and it was lost when the
/// message became a constant: under `tauri dev` the working directory is
/// `src-tauri/`, so a key at the repo root *is* found by the walk, and an operator
/// whose key is somewhere else can only tell which case they are in if the message
/// says where it looked. Asserted through the refusal `place` actually returns, so
/// it cannot pass while the path is dropped between the constant and the caller.
#[test]
fn a_worker_with_no_key_is_told_where_the_key_was_looked_for() {
    let scratch = Scratch::new("no-key");
    let host = Host {
        api_key: None,
        api_key_searched_from: Some(scratch.root.join("app-cwd")),
        ..host_with_fleet_bin(&scratch.root.join("fleet"))
    };

    let why = placement::place(
        PaneSpec::Worker(1),
        &scratch.layout,
        &host,
        &scratch.target,
        &PaneContext::baked(),
    )
    .expect_err("a machine with no key cannot place a worker");

    assert!(why.contains("DEEPSEEK_API_KEY"), "{why}");
    assert!(
        why.contains(&scratch.root.join("app-cwd").display().to_string()),
        "the refusal must say where it looked: {why}",
    );
    assert!(why.contains("worker panes cannot"), "{why}");

    // And a host nobody discovered degrades to the general sentence rather than
    // naming a path it invented.
    let bare = placement::place(
        PaneSpec::Worker(1),
        &scratch.layout,
        &Host::bare(),
        &scratch.target,
        &PaneContext::baked(),
    )
    .expect_err("a bare machine cannot place a worker either");
    assert!(bare.contains("the application's working directory"), "{bare}");
}

/// The two machine-level warnings are placement's decision, and `orch` is excluded
/// from them because its own arm emits the `fleet`-binary line (D-075).
///
/// They are the one part of bringing a pane up that the caller performs rather than
/// `place`, because they have to land *before* a placement that may fail — so what
/// is asserted here is the decision, off a [`Host`], with no fleet to run.
#[test]
fn a_bare_machine_warns_about_the_fleet_binary_and_the_toolchain_but_never_for_orch() {
    let bare = Host::bare();

    let worker: Vec<String> =
        placement::machine_notices(&bare, PaneId::Worker(2)).into_iter().map(|(_, t)| t).collect();
    assert_eq!(worker.len(), 2, "a worker hears about both: {worker:?}");
    assert!(worker[0].contains("`fleet` binary was not found"), "{worker:?}");
    assert!(worker[1].contains("no rustup was found"), "{worker:?}");

    // The evaluator is not a worker and does not build Rust for the fleet, so it
    // hears about the binary and not the toolchain.
    let evaluator: Vec<String> =
        placement::machine_notices(&bare, PaneId::Evaluator).into_iter().map(|(_, t)| t).collect();
    assert_eq!(evaluator, vec![placement::MISSING_FLEET_BIN.to_string()], "{evaluator:?}");

    // `orch` hears nothing here — `place_orch` says it, and saying it twice is how
    // the operator learns to stop reading the feed.
    assert!(
        placement::machine_notices(&bare, PaneId::Orch).is_empty(),
        "orch's copy comes from its own placement arm",
    );

    // A machine that has everything says nothing at all.
    let stocked = Host {
        fleet_bin: Some(PathBuf::from("/somewhere/fleet")),
        toolchain: Some(placement::spawn::OperatorToolchain {
            rustup_bin: PathBuf::from("/somewhere/rustup"),
            rustup_home: PathBuf::from("/somewhere/.rustup"),
        }),
        ..Host::bare()
    };
    assert!(placement::machine_notices(&stocked, PaneId::Worker(1)).is_empty());
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
///
/// **Extended to the worker path** when workers moved onto the seam: a worker
/// writes more than `orch` does — a worktree, a private `HOME`, a second config
/// dir — and all of it must land under the same two arguments.
#[test]
fn placing_against_a_scratch_layout_writes_inside_it_and_nowhere_else() {
    let scratch = Scratch::new("containment");
    init_repo(&scratch.target);

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

    placement::place(
        PaneSpec::Worker(1),
        &scratch.layout,
        &host_for_worker(&scratch.root.join("fleet")),
        &scratch.target,
        &PaneContext::baked(),
    )
    .expect("placing a worker against the same scratch layout");

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

    // The worker's own three, so its half of the walk above is not passing by
    // virtue of the worker having done nothing either.
    let worker_config = scratch.layout.pane_config(PaneId::Worker(1));
    assert!(worker_config.join(".claude.json").is_file(), "the worker's L1 config seed");
    assert!(worker_config.join("settings.json").is_file(), "the worker's guardrail policy");
    assert!(
        scratch.layout.worker_home(1).join(".gitconfig").is_file(),
        "the Fence's seeded git identity",
    );
    assert!(
        scratch.layout.worktree(&scratch.target, 1).join(".git").exists(),
        "the worker's own checkout",
    );

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

// --- the worker ------------------------------------------------------------------

/// **Re-seeding after a target switch — the rule with no test until now.**
///
/// `hasTrustDialogAccepted` is keyed by *absolute project path*, so pointing the
/// fleet at a second repository means every pane's seed must be re-applied for its
/// new working directory. A pane that misses it still opens, still shows a
/// terminal, and still reports `accepted` for every message — into a trust dialog
/// it will never leave. That is the failure this asserts against, and the code
/// calls it the single most likely way to reintroduce it.
///
/// Both targets are real repositories, so both placements resolve to real and
/// *different* worktrees (D-067 namespaces them by target) — which is what makes
/// the two project keys distinct and the assertion worth making.
#[test]
fn switching_targets_re_seeds_the_worker_and_keeps_the_first_targets_trust() {
    let scratch = Scratch::new("re-seed");
    let first = &scratch.target;
    let second = scratch.other_target("second-repo");
    init_repo(first);
    init_repo(&second);
    let host = host_for_worker(&scratch.root.join("fleet"));

    let one = placement::place(
        PaneSpec::Worker(1),
        &scratch.layout,
        &host,
        first,
        &PaneContext::baked(),
    )
    .expect("placing worker-1 against the first target");

    let two = placement::place(
        PaneSpec::Worker(1),
        &scratch.layout,
        &host,
        &second,
        &PaneContext::baked(),
    )
    .expect("placing worker-1 again after the target switched");

    // The two placements really did land in different directories — otherwise
    // there is only one key and this test proves nothing.
    let first_cwd = one.gauge.expect("a worker records a gauge source").cwd;
    let second_cwd = two.gauge.expect("a worker records a gauge source").cwd;
    assert_ne!(first_cwd, second_cwd, "a target switch must move the worker's checkout");

    let seed = scratch.layout.pane_config(PaneId::Worker(1)).join(".claude.json");
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&seed).unwrap()).unwrap();

    for (which, cwd) in [("the first target's", &first_cwd), ("the second target's", &second_cwd)] {
        let key = std::fs::canonicalize(cwd).unwrap().display().to_string();
        assert_eq!(
            value["projects"][&key]["hasTrustDialogAccepted"],
            serde_json::json!(true),
            "{which} project entry must survive the switch, or that pane opens onto a \
             trust dialog and accepts every message into it: {value:#}",
        );
    }
}

/// **The Fence, at the command** (WP-08, D-052).
///
/// Seeding the private `HOME` has a test; the command that will actually run has
/// not had one. Both halves are asserted here, and the second is the one that
/// needed care: `CommandBuilder::new` seeds itself from the *parent* environment,
/// so an `ANTHROPIC_API_KEY` sitting in the operator's shell is genuinely inherited
/// and must be genuinely removed. Asserting it is absent without first putting it
/// there would pass on a machine that never had one — a test passing for the wrong
/// reason. So this test puts one there.
///
/// **The one place in this file that touches the process environment**, and it is
/// simulating the parent the app is launched from, never supplying an input to
/// `place` — every one of those still arrives as a value. The same precedent the
/// `spawn` unit tests set for `HOME`.
#[test]
fn a_workers_command_carries_the_private_home_and_no_inherited_api_key() {
    let scratch = Scratch::new("fence");
    init_repo(&scratch.target);

    let restore = std::env::var("ANTHROPIC_API_KEY").ok();
    std::env::set_var("ANTHROPIC_API_KEY", "sk-the-operators-own-key");

    let placed = placement::place(
        PaneSpec::Worker(3),
        &scratch.layout,
        &host_for_worker(&scratch.root.join("fleet")),
        &scratch.target,
        &PaneContext::baked(),
    )
    .expect("placing worker-3");

    match restore {
        Some(value) => std::env::set_var("ANTHROPIC_API_KEY", value),
        None => std::env::remove_var("ANTHROPIC_API_KEY"),
    }

    assert_eq!(
        env_on(&placed.command, "HOME").as_deref(),
        Some(scratch.layout.worker_home(3).display().to_string().as_str()),
        "a worker's HOME is the fleet's own private one, not the operator's",
    );
    assert_eq!(
        env_on(&placed.command, "ANTHROPIC_API_KEY"),
        None,
        "L2: an inherited api key parks the pane on an approval prompt forever, so it \
         must be *removed* from the command and not merely left unset",
    );
    // The positive control: this really is a worker's command, carrying the key it
    // was handed on the host — so the assertion above is about removal and not
    // about having built something else entirely.
    assert_eq!(
        env_on(&placed.command, "ANTHROPIC_AUTH_TOKEN").as_deref(),
        Some(WORKER_KEY),
        "the worker authenticates with the token from the host",
    );
}

/// **A bare machine: the toolchain settings are absent, not empty** (D-069).
///
/// A `RUSTUP_HOME` naming a directory that does not exist makes rustup try to
/// *install* there — measured, and worse than the `command not found` a worker gets
/// without it. So the claim is specifically that the command does not point at the
/// fleet's own toolchain homes, which on this host were never seeded.
///
/// Note what is *not* asserted: that these variables are absent outright.
/// `CommandBuilder` inherits the parent environment, so a bare `is_none()` would be
/// a claim about the machine running the test rather than about placement — it
/// would pass or fail depending on whether the developer has `CARGO_HOME` exported.
#[test]
fn a_machine_with_no_toolchain_gets_no_toolchain_settings_at_all() {
    let scratch = Scratch::new("bare-toolchain");
    init_repo(&scratch.target);
    let host = host_for_worker(&scratch.root.join("fleet"));
    assert!(host.toolchain.is_none(), "the host under test has no rustup");

    let placed = placement::place(
        PaneSpec::Worker(2),
        &scratch.layout,
        &host,
        &scratch.target,
        &PaneContext::baked(),
    )
    .expect("a machine with no rustup still places a worker");

    let fleet_toolchain = scratch.layout.fleet_toolchain();
    assert_ne!(
        env_on(&placed.command, "CARGO_HOME"),
        Some(fleet_toolchain.cargo_home.display().to_string()),
        "the fleet's CARGO_HOME must not be set when nothing seeded it",
    );
    assert_ne!(
        env_on(&placed.command, "RUSTUP_HOME"),
        Some(fleet_toolchain.rustup_home.display().to_string()),
        "a RUSTUP_HOME naming a directory that does not exist makes rustup install there",
    );

    // And the directories really were never made, which is what makes the two
    // assertions above matter rather than being about a path that happens to exist.
    assert!(!fleet_toolchain.cargo_home.exists(), "nothing was seeded");
    assert!(!fleet_toolchain.rustup_home.exists(), "nothing was seeded");

    // The PATH rung goes with them: without shims to point at, adding the rung
    // would put a directory that does not exist on every worker's PATH.
    let path = env_on(&placed.command, "PATH").expect("a worker is given a PATH");
    assert!(
        !path.contains(&fleet_toolchain.cargo_home.display().to_string()),
        "no cargo/bin rung on a machine with no toolchain: {path}",
    );
}

/// **A target that is not a repository degrades to the shared checkout, loudly.**
///
/// The wording has had a test; the trigger has not. A fleet that fell back silently
/// would look identical to a working one right up until an operator trusted a
/// reviewed `done` that nobody could have reviewed.
#[test]
fn a_target_that_is_not_a_repository_falls_back_and_says_what_review_loses() {
    let scratch = Scratch::new("fallback");
    // Deliberately *not* `init_repo` — an ordinary directory, which is exactly the
    // case the fallback exists for.

    let placed = placement::place(
        PaneSpec::Worker(4),
        &scratch.layout,
        &host_for_worker(&scratch.root.join("fleet")),
        &scratch.target,
        &PaneContext::baked(),
    )
    .expect("a target that is not a repository still places a worker");

    let warning = placed
        .notices
        .iter()
        .find(|(level, _)| *level == NoticeLevel::Warn)
        .map(|(_, text)| text.as_str())
        .expect("the fallback is announced, never silent");
    assert!(warning.contains("worker-4"), "which worker: {warning}");
    assert!(warning.contains("share the checkout"), "what happened: {warning}");
    assert!(
        warning.contains("Peer review is degraded"),
        "what is degraded — the point of the notice: {warning}",
    );
    assert!(
        warning.contains("git diff fleet/worker-N"),
        "the move that stops working: {warning}",
    );
    assert!(
        warning.contains("treat a reviewed `done` as unreviewed"),
        "and what the operator should do about it: {warning}",
    );

    // The worker really did land in the shared checkout, and no worktree was
    // invented beside it.
    assert_eq!(
        placed.gauge.expect("a worker records a gauge source").cwd,
        scratch.target,
        "the fallback cwd is the target itself",
    );
    assert!(
        !scratch.layout.worktree(&scratch.target, 4).exists(),
        "no worktree is created when git could not oblige",
    );
    assert_eq!(
        scratch.guardrail_roots(PaneId::Worker(4)).first().map(String::as_str),
        Some(scratch.target.display().to_string().as_str()),
        "the guardrail follows the cwd the pane will really run in",
    );
}

/// **The successful worktree path, against a real repository.**
///
/// Four workers editing at once must not fight over one index, and peer review is
/// `git diff` between their branches (D-048), so both the directory and the branch
/// are the claim — a worktree without its branch would give a reviewer nothing to
/// compare.
#[test]
fn a_worker_gets_its_own_worktree_and_branch_in_a_real_repository() {
    let scratch = Scratch::new("worktree");
    init_repo(&scratch.target);

    let placed = placement::place(
        PaneSpec::Worker(1),
        &scratch.layout,
        &host_for_worker(&scratch.root.join("fleet")),
        &scratch.target,
        &PaneContext::baked(),
    )
    .expect("placing worker-1 in a real repository");

    let worktree = scratch.layout.worktree(&scratch.target, 1);
    assert!(worktree.join(".git").exists(), "a real checkout at {}", worktree.display());
    assert!(
        placed.notices.iter().all(|(level, _)| *level != NoticeLevel::Warn),
        "nothing is degraded when git obliged: {:#?}",
        placed.notices,
    );

    // The branch peer review will diff against. Asserted by shape rather than by
    // recomputing the target slug, so this pins what a reviewer needs — a branch of
    // this worker's own, namespaced under the target — and not the hash function.
    let found = branches(&scratch.target);
    assert_eq!(
        found.iter().filter(|b| b.starts_with("fleet/") && b.ends_with("/worker-1")).count(),
        1,
        "worker-1 needs exactly one branch of its own to be reviewed against: {found:?}",
    );

    // The gauge source is recorded before the process exists — it is returned from
    // placement, which is strictly earlier than the pty the caller has yet to spawn.
    let gauge = placed.gauge.expect("a worker records a gauge source");
    assert_eq!(gauge.cwd, worktree, "the gauge samples the worker's own checkout");
    assert_eq!(gauge.config_dir, scratch.layout.pane_config(PaneId::Worker(1)));

    // Everything downstream followed the worktree rather than the target.
    assert_eq!(
        scratch.guardrail_roots(PaneId::Worker(1)).first().map(String::as_str),
        Some(worktree.display().to_string().as_str()),
        "the guardrail's first root is the worker's own checkout",
    );
    assert_eq!(
        env_on(&placed.command, "CLAUDE_CONFIG_DIR").as_deref(),
        Some(scratch.layout.pane_config(PaneId::Worker(1)).display().to_string().as_str()),
    );
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

// --- the evaluator ----------------------------------------------------------------

/// **The evaluator's guardrail roots are its own working directory, and nothing
/// else** — narrower than any other pane's, and never asserted until now.
///
/// This is the veil at the filesystem, and it is a different claim from the veil in
/// `PaneId`. The name is absent from every enumeration; *this* is the other half —
/// the pane that reads everything the run produced may change almost nothing. Two
/// exclusions carry it, and both would be invisible in a `Vec` held briefly in
/// memory, so this reads the policy file the pane's Claude Code will actually load:
///
///  1. **No `_shell`.** Every other pane has it, and it holds the live event log
///     this pane is reading. A grader that could write its own evidence is not one.
///  2. **No operator `[fence] allow` extras.** Those exist for the panes doing the
///     work; an operator widening the fleet's reach must not widen its judge's.
///
/// The orchestrator is placed against the same layout with the same context in the
/// same test, because "narrower" is a comparison and asserting it against a
/// remembered number would be asserting it against nothing.
#[test]
#[cfg(feature = "devmode")]
fn the_evaluators_write_roots_are_its_own_directory_alone_and_no_pane_is_narrower() {
    let mut bench = common::Bench::new("roots");
    bench.dev_mode(true);
    // An operator who has widened the fleet's reach. It must reach `orch` and stop
    // there.
    let extra = bench.operator_allows("shared-cache");

    bench.place(PaneSpec::Evaluator);
    let evaluator_roots = bench.guardrail_roots(PaneId::Evaluator);
    assert_eq!(
        evaluator_roots,
        vec![bench.retro_dir().display().to_string()],
        "the evaluator writes in the run it was given and nowhere else",
    );

    // The comparison that makes "narrower" mean something: the same layout, the same
    // context, a pane that is doing the work.
    bench.place(PaneSpec::Orch);
    let orch_roots = bench.guardrail_roots(PaneId::Orch);
    assert_eq!(
        orch_roots,
        vec![
            bench.target.display().to_string(),
            bench.layout.shell().display().to_string(),
            extra.display().to_string(),
        ],
        "orch gets its cwd, `_shell` and the operator's extra — the evaluator got neither \
         of the last two",
    );
    assert!(orch_roots.len() > evaluator_roots.len(), "strictly narrower, not merely different");

    // And its config dir is outside `_shell`, so its own reasoning never lands in the
    // archive the next generation reads (D-062). Asserted here because this is the
    // test that knows where placement actually put it.
    let config_dir = bench.config_dir(PaneId::Evaluator);
    assert!(
        !config_dir.starts_with(bench.layout.shell()),
        "{} is inside the tree `runs::harvest_transcripts` walks",
        config_dir.display(),
    );
}

/// **Dev mode is re-checked at the spawn site, and the flag comes from the layout**
/// (D11).
///
/// The wake has already asked the same question, and that is deliberately not
/// enough: "there is no evaluator outside dev mode" has to be a property of the
/// place the pane is built, or a caller can lie about it. What proves the property
/// *is* a property is flipping the flag without touching the process — the same
/// scratch `config.json` an operator's switch would write, in a directory this test
/// owns — and watching the answer change.
///
/// **Gated on `devmode` because an ungated version would assert nothing.** Without
/// the feature there is no brief compiled in, so placement refuses whatever the flag
/// says, and a test that passed for that reason would say nothing about the mode.
#[test]
#[cfg(feature = "devmode")]
fn placing_an_evaluator_reads_the_mode_from_the_layouts_own_config() {
    let bench = common::Bench::new("mode");

    // No config at all: the ordinary first-run state, and off.
    assert_eq!(
        bench
            .try_place(PaneSpec::Evaluator)
            .expect_err("no config means no mode means no evaluator"),
        placement::NO_EVALUATOR,
    );

    // Explicitly off is the same answer, and the operator's own home was never
    // consulted to reach it.
    bench.dev_mode(false);
    assert_eq!(
        bench.try_place(PaneSpec::Evaluator).expect_err("the mode off is a refusal"),
        placement::NO_EVALUATOR,
    );

    // Nothing was written on the way to either refusal: a refused placement must not
    // leave a half-laid-out run behind for the next one to find.
    assert!(!bench.layout.root().join("dev").exists(), "a refusal lays nothing out");

    // And the one thing that changed is the flag in this layout's own file.
    bench.dev_mode(true);
    bench
        .try_place(PaneSpec::Evaluator)
        .expect("the same call, the same arguments, the mode on in the layout's config");
}

/// **The run is laid out for reading, and the brief points at where it landed**
/// (D2).
///
/// This is not a step a caller could usefully have done first: the brief cites the
/// directory, and a brief citing a directory nobody wrote costs the pane its first
/// turn asking about it. Both halves are asserted, because a snapshot the brief does
/// not name and a name with no snapshot behind it fail identically from the outside.
#[test]
#[cfg(feature = "devmode")]
fn placing_the_evaluator_lays_the_run_out_and_briefs_it_against_that_directory() {
    let bench = common::Bench::new("laid-out");
    bench.dev_mode(true);

    let placed = bench.place(PaneSpec::Evaluator);

    // The run, on disk, in the directory the pane will start in.
    let cwd = bench.retro_dir();
    assert!(cwd.join("events.json").is_file(), "the log, as JSON, for a reader with no SQLite");
    assert!(cwd.join("manifest.json").is_file(), "and what is in the directory");
    assert_eq!(
        placed.command.get_cwd().map(PathBuf::from),
        Some(cwd.clone()),
        "the pane starts in the run it was given",
    );

    // The brief, on the command, naming that same directory and this mission.
    let brief = placed
        .command
        .get_argv()
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .find(|a| a.contains(common::MISSION))
        .expect("the rendered brief is on the command");
    assert!(brief.contains(&cwd.display().to_string()), "the brief names the run directory");
    assert!(
        !brief.contains("{run_dir}") && !brief.contains("{mission_file}"),
        "no placeholder survived rendering",
    );

    // Not on the live gauge: it samples the transcripts of the panes doing the work.
    assert!(placed.gauge.is_none(), "the evaluator records no transcript source");

    // Everything it wrote is under the layout it was handed — the property the whole
    // module exists for, extended to the pane that writes outside `_shell`.
    for path in walk(&bench.root) {
        assert!(
            path.starts_with("state") || path.starts_with("workspaces")
                || path.starts_with("harness") || path.starts_with("answers"),
            "{} is outside everything placement was handed",
            path.display(),
        );
    }
}

// --- the Critic (WP-20, D-076) --------------------------------------------------
//
// Every test below runs **without** `--features devmode`, and that is an
// assertion rather than a convenience: the Critic is a product feature, so a
// default build has to be able to place one. None of them writes an environment
// variable and none of them touches `~/.fleetor`.

/// **The Critic exists with dev mode off**, which is the whole of "it is a product
/// feature and not an instrument of an experiment."
///
/// Asserted the way `placing_an_evaluator_reads_the_mode_from_the_layouts_own_config`
/// asserts the opposite: by flipping the flag in this layout's own `config.json`
/// — the same file an operator's switch writes — and watching the answer *not*
/// change.
#[test]
fn the_critic_is_placed_whatever_the_mode_says_because_nothing_gates_it() {
    let bench = common::Bench::new("critic-mode");

    // No config at all: the ordinary first-run state, and off.
    bench.try_place(PaneSpec::Critic { run: RunSource::Live }).expect("no config is not a refusal");
    bench.dev_mode(false);
    bench.try_place(PaneSpec::Critic { run: RunSource::Live }).expect("the mode off is not either");
    bench.dev_mode(true);
    bench.try_place(PaneSpec::Critic { run: RunSource::Live }).expect("and on changes nothing");
}

/// **The run is laid out for reading, and the brief points at where it landed.**
///
/// The evaluator's D2 applies unchanged: the working directory *is* the run, the
/// brief cites that directory, and a brief citing a directory nobody wrote is a
/// pane that spends its first turn asking about a path. Both halves are asserted,
/// because a snapshot the brief does not name and a name with no snapshot behind
/// it fail identically from the outside.
#[test]
fn placing_the_critic_lays_the_live_run_out_and_briefs_it_against_that_directory() {
    let bench = common::Bench::new("critic-run");

    let placed = bench.place(PaneSpec::Critic { run: RunSource::Live });

    // The run, on disk, in the directory the pane will start in — the same three
    // things `snapshot_live_run` writes for the evaluator, from the same database.
    let cwd = bench.critic_run_dir();
    assert!(cwd.join("events.json").is_file(), "the log, as JSON, for a reader with no SQLite");
    assert!(cwd.join("manifest.json").is_file(), "and what is in the directory");
    assert_eq!(
        placed.command.get_cwd().map(PathBuf::from),
        Some(cwd.clone()),
        "the pane starts in the run it was given",
    );

    // The brief, on the command, naming that same directory.
    let brief = critic_brief(&placed.command);
    assert!(brief.contains(&cwd.display().to_string()), "the brief names the run directory");
    assert!(!brief.contains("{archive}"), "no placeholder survived rendering");
    assert!(
        brief.contains("Every finding carries a citation"),
        "and it is the brief the spike converged on, not a summary of it",
    );

    // Not on the live gauge: it samples the transcripts of the panes doing the work.
    assert!(placed.gauge.is_none(), "the Critic records no transcript source");

    // Everything it wrote is under the layout it was handed.
    for path in walk(&bench.root) {
        assert!(
            path.starts_with("state")
                || path.starts_with("workspaces")
                || path.starts_with("harness")
                || path.starts_with("answers"),
            "{} is outside everything placement was handed",
            path.display(),
        );
    }
}

/// **The Critic is handed the same two variables `orch` is, and is still not in
/// the fleet** (WP-21, D-079).
///
/// This test is WP-20's `the_critic_is_given_no_socket_so_fleet_send_inside_it_
/// reaches_nothing` inverted rather than deleted, and the comparison against
/// `orch` is kept for the reason it was there in the first place: asserting the
/// *presence* of a variable proves nothing unless a pane known to have it is
/// placed from the same layout in the same test, so a bench that silently
/// stopped setting sockets for everyone would fail here rather than pass.
///
/// **Why the absence had to go.** `FLEET_SOCKET` is baked into the
/// `CommandBuilder` at spawn. An operator control that hands the pane a socket
/// when they open the interview would therefore have to respawn the pane —
/// destroying the conversation they opened the interview to have. So the switch
/// is not here; it is in `fleetor_server::hub::Hub::handle`, which refuses an op
/// from `critic` at accept time while the interview is closed. The pty tests in
/// `panes.rs` are where that behaviour is pinned; this file only proves the
/// route exists to be gated.
///
/// **And the point of the whole design, asserted here with the socket present:**
/// `PaneId::Critic.is_fleet_member()` is still `false`. Addressable, never
/// enumerated.
#[test]
fn the_critic_is_given_a_socket_and_is_still_not_a_member_of_the_fleet() {
    let bench = common::Bench::new("critic-socket");

    let critic = bench.place(PaneSpec::Critic { run: RunSource::Live }).command;
    let orch = bench.place(PaneSpec::Orch).command;

    // The comparison. Both get both, and they are the same socket.
    let socket = Some(bench.layout.socket().display().to_string());
    assert_eq!(
        env_on(&orch, "FLEET_SOCKET"),
        socket,
        "orch can reach the hub, which is what makes the presence below mean something",
    );
    assert_eq!(env_on(&orch, "FLEETOR_PANE").as_deref(), Some("orch"));

    assert_eq!(
        env_on(&critic, "FLEET_SOCKET"),
        socket,
        "the Critic dials the same hub orch does — the operator's switch gates what the hub \
         does with what arrives, not whether there is anything to dial",
    );
    assert_eq!(
        env_on(&critic, "FLEETOR_PANE").as_deref(),
        Some("critic"),
        "and it is a participant in the record under its own name, so the log can say who \
         asked rather than attributing an interview to nobody",
    );

    // **The invariant, asserted with the socket in hand.** A voice is not a
    // membership: no roster row, no leg of a broadcast, in no rendered brief's
    // peer list. Every one of those is filtered on this one predicate.
    assert!(
        !PaneId::Critic.is_fleet_member(),
        "a socket must not have made the Critic enumerable — that is the entire design",
    );
    assert!(
        !PaneId::roster(&fleetor_core::pane::WORKER_SLOTS).contains(&PaneId::Critic),
        "the roster is what spawns, what a broadcast reaches, and what a peer list is built \
         from, and the Critic is in none of the three",
    );

    // It is still a terminal, so the rest of what makes a pane a pane is there.
    assert_eq!(env_on(&critic, "TERM").as_deref(), Some("xterm-256color"));
    assert!(env_on(&critic, "PATH").is_some_and(|p| !p.is_empty()));

    // And the machine notice it used to be excluded from is now true of it: a
    // Critic on a machine with no `fleet` binary has an interview that cannot be
    // opened in any useful sense.
    let bare = Host::bare();
    assert!(
        !placement::machine_notices(&bare, PaneId::Critic).is_empty(),
        "a pane that can now reach the hub must be told when the binary that reaches it is \
         missing",
    );
    assert!(
        placement::machine_notices(&bare, PaneId::Orch).is_empty(),
        "…while orch still emits that line from its own placement instead",
    );
}

/// **The Critic's write roots are its own working directory alone, like the
/// evaluator's** — and the comparison is what makes "alone" mean anything.
///
/// Two exclusions carry it, and both would be invisible in a `Vec` held briefly in
/// memory, so this reads the policy file the pane's Claude Code will actually load:
///
///  1. **No `_shell`.** It holds the live event log this pane is reading. A critic
///     that could write its own evidence is not one.
///  2. **No operator `[fence] allow` extras.** Those exist for the panes doing the
///     work; an operator widening the fleet's reach must not widen its reader's.
#[test]
fn the_critics_write_roots_are_its_own_directory_alone_and_orch_is_wider() {
    let mut bench = common::Bench::new("critic-roots");
    let extra = bench.operator_allows("shared-cache");

    bench.place(PaneSpec::Critic { run: RunSource::Live });
    let critic_roots = bench.guardrail_roots(PaneId::Critic);
    assert_eq!(
        critic_roots,
        vec![bench.critic_run_dir().display().to_string()],
        "the Critic writes in the run it was given and nowhere else",
    );

    bench.place(PaneSpec::Orch);
    let orch_roots = bench.guardrail_roots(PaneId::Orch);
    assert_eq!(
        orch_roots,
        vec![
            bench.target.display().to_string(),
            bench.layout.shell().display().to_string(),
            extra.display().to_string(),
        ],
        "orch gets its cwd, `_shell` and the operator's extra — the Critic got neither of \
         the last two",
    );
    assert!(orch_roots.len() > critic_roots.len(), "strictly narrower, not merely different");

    // And its config dir is outside `_shell` — so no pane in the run being
    // critiqued can write into it, and its own transcript is neither copied into
    // the directory it is reading nor archived with the run.
    let config_dir = bench.config_dir(PaneId::Critic);
    assert!(
        !config_dir.starts_with(bench.layout.shell()),
        "{} is inside the tree `runs::harvest_transcripts` walks",
        config_dir.display(),
    );
}

/// **The Critic's brief is the operator's to rewrite**, which is the asymmetry
/// separating it from the pane whose brief is compiled in from another repository.
///
/// Driven all the way through placement rather than asserted on the resolver, so
/// what it pins is that the operator's words reach the process — the thing an
/// operator who edited the file actually wants to be true.
#[test]
fn the_critics_brief_comes_from_the_prompts_directory_the_operator_can_edit() {
    let mut bench = common::Bench::new("critic-brief");

    // What ships is what a default placement uses.
    let shipped = critic_brief(&bench.place(PaneSpec::Critic { run: RunSource::Live }).command);
    assert!(shipped.contains("You are the Critic"), "the shipped brief reaches the command");

    // …and an override replaces it, through the same resolver every other brief
    // uses. The context is the one placement is handed, so this is `resolve`'s
    // output travelling rather than a string poked in.
    let prompts = bench.root.join("prompts-override");
    std::fs::create_dir_all(&prompts).unwrap();
    std::fs::write(
        prompts.join("critic.md"),
        "You are the Critic. Read {archive} and report only what it proves.\n",
    )
    .unwrap();
    bench.context = PaneContext::resolve(&prompts);

    let placed = bench.place(PaneSpec::Critic { run: RunSource::Live });
    let mine = critic_brief(&placed.command);
    assert!(mine.contains("report only what it proves"), "the operator's own words: {mine}");
    assert!(!mine.contains("You have no answer key"), "and not the built-in's: {mine}");
    assert!(
        mine.contains(&bench.critic_run_dir().display().to_string()),
        "rendered against the run, exactly as the built-in is",
    );
}

/// **Reading an archived run is declared and not built** (ticket 09).
///
/// The refusal is a sentence rather than a half-built placement, and rather than a
/// silent fall-through to the live run — which would be the worst of the three,
/// because the operator would get a pane that looked right and was reading a
/// different run than the one they asked for. Nothing is laid out on the way to
/// it, so a refused placement leaves nothing behind for the next one to find.
#[test]
fn placing_a_critic_on_an_archived_run_refuses_and_writes_nothing() {
    let bench = common::Bench::new("critic-archived");

    let why = bench
        .try_place(PaneSpec::Critic { run: RunSource::Archived("2026-08-08T00-32-58Z".into()) })
        .expect_err("an archived run is not built yet");
    assert_eq!(why, placement::ARCHIVED_NOT_BUILT);
    assert!(
        !fleetor_shell::critic::critic_dir(bench.layout.root()).exists(),
        "a refusal lays nothing out",
    );
}

/// The rendered Critic brief, off the command placement built.
///
/// Found by its opening sentence rather than by argument position, so a change to
/// the flag order fails loudly here instead of quietly asserting on
/// `--permission-mode`.
fn critic_brief(command: &portable_pty::CommandBuilder) -> String {
    command
        .get_argv()
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .find(|a| a.contains("You are the Critic"))
        .expect("the rendered brief is on the command")
}
