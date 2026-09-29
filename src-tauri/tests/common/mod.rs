//! One whole FLEETOR installation in a scratch directory, against which **every**
//! pane kind can really be placed (WP-21, ticket 04).
//!
//! **Not a grab-bag of test helpers — a module for exactly one thing**, shared
//! because two files assert different properties of the same placements and
//! retyping the setup in both is the move this ticket exists to stop making.
//! `tests/placement.rs` reads the policy file that was written; `write_guardrail.rs`
//! runs the installed hook through a real shell against real tool-call payloads.
//!
//! The target is a real git repository for the reason `tests/placement.rs` gives at
//! its own `init_repo`: a worker's successful path *is* a `git worktree add`. A
//! target that is not a repository would be made one by the first placement
//! (D-084), which is a write into the target this fixture should not depend on —
//! and where that fails, every worker lands in the shared checkout, whose cwd is
//! the target, and "a worker may not write in the target repo" stops being a claim
//! that can be tested at all.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use fleetor_core::pane::PaneId;
use fleetor_shell::guardrail;
use fleetor_shell::placement::{self, Host, Layout, PaneSpec, Placed};
use fleetor_shell::prompts::PaneContext;

/// The worker credential the bench hands the host, for the same reason.
pub const WORKER_KEY: &str = "sk-fleetor-bench-worker-key";

/// A whole installation in a scratch directory: the four arguments [`place`] takes,
/// side by side, and the disk they are allowed to touch.
pub struct Bench {
    /// The scratch root everything lives under. Removed on drop.
    pub root: PathBuf,
    /// The fleet's tree — `config.json` and `_shell/`.
    pub layout: Layout,
    /// What this machine has: a `fleet` binary and a worker key.
    pub host: Host,
    /// The repository the fleet is pointed at.
    pub target: PathBuf,
    /// The briefs and launch settings every placement here spawns with.
    pub context: PaneContext,
}

impl Bench {
    /// Build one. `tag` keeps concurrent tests out of each other's directories.
    ///
    /// **The name is restricted to characters a shell will not split**, which is not
    /// housekeeping: every path derived from this root ends up inside a `sh -c`
    /// command that the guardrail hook parses. `ThreadId(4)`'s parentheses would end
    /// the word, and the hook would answer honestly about a path nobody meant to
    /// write to.
    pub fn new(tag: &str) -> Self {
        let thread: String = format!("{:?}", std::thread::current().id())
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        let root = std::env::temp_dir()
            .join(format!("fleetor-bench-{tag}-{}-{thread}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        let layout = Layout::under(root.join("state"));
        std::fs::create_dir_all(layout.root()).unwrap();

        // A real repository so a worker gets a worktree of its own.
        let target = root.join("repo");
        init_repo(&target);

        let host = Host {
            fleet_bin: Some(root.join("fleet")),
            api_key: Some(WORKER_KEY.to_string()),
            ..Host::bare()
        };

        Self { root, layout, host, target, context: PaneContext::baked() }
    }

    /// Give the operator an extra `[fence] allow` root, as `prompts/launch.conf`
    /// does. Returns the directory, which is not created — being a root and
    /// existing are different questions.
    pub fn operator_allows(&mut self, name: &str) -> PathBuf {
        let dir = self.root.join(name);
        self.context.launch.fence_allow.push(dir.display().to_string());
        dir
    }

    /// **Give this machine an operator login for one harness** (C75), the way
    /// `Host::discover` would on a machine the operator is logged in on.
    ///
    /// The document is a stand-in and is never parsed by anything a placement
    /// reaches: `OperatorLogin` is opaque by construction, and the harnesses write
    /// it verbatim. What a test asserts is *where it lands and what it replaces*,
    /// which is exactly what a real credential would be asserted on — so nothing
    /// here needs the operator's own.
    pub fn operator_logs_in(&mut self, harness: &'static str, document: &str) {
        self.host
            .operator_logins
            .push((harness, placement::OperatorLogin::new(document.to_string())));
    }

    /// Place a pane for real, and hand back what a caller would get.
    pub fn place(&self, spec: PaneSpec) -> Placed {
        let pane = spec.pane();
        self.try_place(spec).unwrap_or_else(|e| panic!("placing {pane}: {e}"))
    }

    /// [`Self::place`] for the tests that are about a refusal.
    pub fn try_place(&self, spec: PaneSpec) -> Result<Placed, String> {
        placement::place(spec, &self.layout, &self.host, &self.target, &self.context)
    }

    /// Where placement seeds this pane's Claude Code configuration — and therefore
    /// where it installed its write guardrail.
    pub fn config_dir(&self, pane: PaneId) -> PathBuf {
        self.layout.pane_config(&fleetor_shell::placement::SessionsId::new(fleetor_shell::placement::UNASSIGNED_SESSIONS), pane)
    }

    /// Every `--root` the hook placement installed for `pane` was given, in order.
    pub fn guardrail_roots(&self, pane: PaneId) -> Vec<String> {
        roots_of(&hook_command(&self.config_dir(pane)))
    }

    /// The refusal journal the guardrail appends to, which is what puts a denial on
    /// the Activity feed.
    pub fn journal(&self) -> PathBuf {
        guardrail::journal_path(&self.layout.shell())
    }
}

impl Drop for Bench {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The shell command Claude Code will run for every matched tool call, read out of
/// the `settings.json` placement wrote into `config_dir`.
pub fn hook_command(config_dir: &Path) -> String {
    let settings = config_dir.join("settings.json");
    let value: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&settings)
            .unwrap_or_else(|e| panic!("{} must exist: {e}", settings.display())),
    )
    .unwrap();
    value["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
        .as_str()
        .expect("the hook command Claude Code will run")
        .to_string()
}

/// Every `--root` on a hook command, in order.
pub fn roots_of(command: &str) -> Vec<String> {
    command
        .split(' ')
        .collect::<Vec<&str>>()
        .windows(2)
        .filter(|w| w[0] == "--root")
        .map(|w| w[1].trim_matches('\'').to_string())
        .collect()
}

/// Make `dir` a real repository with one commit — what `git worktree add` needs
/// before it will succeed. Identity and signing are pinned per-command so the
/// result does not depend on the machine's own git configuration.
fn init_repo(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("README.md"), "scratch\n").unwrap();
    for args in [
        &["init", "-q", "-b", "main"][..],
        &["add", "-A"][..],
        &["commit", "-q", "-m", "first"][..],
    ] {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.email=scratch@fleetor.test"])
            .args(["-c", "user.name=Scratch"])
            .args(["-c", "commit.gpgsign=false"])
            .args(["-c", "core.hooksPath=/dev/null"])
            .args(args)
            .output()
            .expect("git must be on PATH for these tests");
        assert!(
            output.status.success(),
            "git {args:?} in {} failed: {}",
            dir.display(),
            String::from_utf8_lossy(&output.stderr),
        );
    }
}
