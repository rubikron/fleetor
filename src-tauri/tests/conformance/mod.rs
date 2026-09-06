//! The conformance driver: **one pass per registered harness**, over the same
//! assertions (WP-25, M23, C17, C19, C20).
//!
//! This module is the shape, not the assertions. It places a real fleet's worth
//! of panes through [`placement::place`] against a scratch layout, once for every
//! entry in the harness registry, and hands each checkpoint file a [`Pass`] to
//! assert against. The checkpoints themselves live beside it, split by number:
//!
//!  - `tests/harness_conformance_1_7.rs` — program and base arguments, brief
//!    carrier, model flag and posture, config dir and its seeding, credential
//!    wiring and the scrub, config and credential isolation, write-guardrail
//!    install.
//!  - `tests/harness_conformance_8_14.rs` — outbound reachability, typing
//!    profile, command channel, gauge, orphan names, transcript, project identity
//!    and trust.
//!
//! **How to add checkpoints 8–14 without editing the file next door.** Write a
//! new top-level test binary, `mod conformance;`, and one `#[test]` per
//! checkpoint whose whole body is [`for_each_registered`]. Everything a
//! checkpoint can look at is on [`Pass`]; if a checkpoint needs something the
//! pass does not carry, add it *here* — a field, or a method like
//! [`Pass::place_worker_against`] — rather than placing panes of your own, so
//! that both files keep asserting against the same bring-up. The one thing that
//! must not move is the rule below.
//!
//! ## The rule every checkpoint here obeys
//!
//! **Assert end to end through [`placement::place`], never against the spec
//! constant.** A test that reads `CLAUDE_CODE_SPEC` and agrees with it tests the
//! constant; a test that drives `place` and finds the spec's answer in the
//! command, in the seeded files, or in the notices tests the seam. The spec is an
//! *input* to every assertion below — it says what to look for — and the
//! observable output of `place` is what is looked at. That is what makes this
//! suite the safety net for the migrate batches: they move the literals onto the
//! spec underneath a `place` whose caller-observable behaviour does not change,
//! so a batch that breaks behaviour fails here rather than passing quietly.
//!
//! **A harness that stubs a checkpoint fails rather than compiling quietly.**
//! Each checkpoint below refuses the empty answer — an empty program name, an
//! empty seed-key list, a scrub list that removes nothing an attended seat is
//! actually given — because an assertion that would pass against a stub is worse
//! than no assertion: it reads as coverage.
//!
//! ## Nothing here touches the operator's installation
//!
//! `tests/placement.rs`'s discipline exactly: the layout and the host arrive as
//! values, every path is under a scratch root this module makes and removes, and
//! **nothing here sets an environment variable**. The credential, the model, the
//! endpoint and the posture are all distinctive scratch values, so finding one on
//! a command proves it travelled from the caller rather than coinciding with a
//! default.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use fleetor_core::pane::PaneId;
use fleetor_shell::placement::harness::{registered, Harness};
use fleetor_shell::placement::{self, HarnessSpec, Host, Layout, PaneSpec, Placed};
use fleetor_shell::prompts::PaneContext;

// --- the values a pass hands in ------------------------------------------------

/// The marker the orchestrator's brief carries. Distinctive, so finding it proves
/// the brief travelled rather than that some default happened to match.
pub const ORCH_BRIEF_MARK: &str = "FLEETOR-CONFORMANCE-ORCH-BRIEF";

/// The marker a worker's brief carries. Different from the orchestrator's, so a
/// harness that hands every seat the same brief fails rather than passing on one
/// of them.
pub const WORKER_BRIEF_MARK: &str = "FLEETOR-CONFORMANCE-WORKER-BRIEF";

/// The fleet's own credential, as this pass's host holds it.
pub const WORKER_KEY: &str = "sk-fleetor-conformance-worker-key";

/// The model the caller names for unattended seats.
pub const WORKER_MODEL: &str = "fleetor-conformance-worker-model";

/// The endpoint the caller names for unattended seats.
pub const WORKER_BASE_URL: &str = "https://workers.conformance.invalid/harness";

/// The permission posture the caller names for unattended seats. Deliberately not
/// a value any harness would default to.
pub const WORKER_POSTURE: &str = "fleetor-conformance-posture";

/// The slot the pass's worker is placed in.
pub const WORKER_SLOT: u8 = 1;

// --- one pass ------------------------------------------------------------------

/// One registered harness, and the panes `place` actually produced for it.
///
/// Two seats, because between them they carry every asymmetry checkpoints 1–7 are
/// about: [`Pass::orch`] is the operator's own, attended, on their account, and
/// [`Pass::worker`] is fenced, unattended, on the fleet's credential. A checkpoint
/// asserted on only one of them cannot tell "this harness wires the credential"
/// from "this harness wires the credential everywhere", which is the failure
/// D-062 exists to prevent.
pub struct Pass {
    /// The registry entry this pass is for.
    pub harness: &'static dyn Harness,
    /// Its fourteen answers — the *input* to every assertion, never the thing
    /// asserted against itself.
    pub spec: &'static HarnessSpec,

    /// The scratch root. The layout and the target are both inside it, and it is
    /// removed when the pass drops.
    pub root: PathBuf,
    /// Where this fleet lives, for this pass.
    pub layout: Layout,
    /// The repository the fleet was pointed at — a real git repo, so the worker
    /// gets its own worktree rather than the announced fallback.
    pub target: PathBuf,
    /// What the caller handed placement: briefs carrying the two marks above, and
    /// launch values distinctive enough to prove they travelled.
    pub context: PaneContext,
    /// A machine with a `fleet` binary and the fleet's key on it, and **nothing
    /// else** — no operator home, no toolchain, no stand-in pane program. The
    /// machine-with-nothing case, which is what makes "created fresh rather than
    /// snapshotted from the operator's own" a thing a test can say.
    pub host: Host,

    /// The operator's own seat, placed.
    pub orch: Placed,
    /// One fenced worker, placed.
    pub worker: Placed,
    /// The directory that worker was placed in: its own checkout of the target.
    pub worker_cwd: PathBuf,
}

/// Run `check` once per registered harness, and return how many times it ran.
///
/// `tag` names the checkpoint and only reaches the scratch directory's name, so a
/// failure says which harness and which checkpoint without two tests colliding on
/// disk.
pub fn for_each_registered(tag: &str, check: impl Fn(&Pass)) -> usize {
    let all = registered();
    assert!(
        !all.is_empty(),
        "no harness is registered, so this suite asserts nothing — a suite with one \
         registered harness is the point, a suite with none is decoration",
    );
    for harness in all {
        let pass = Pass::place(tag, *harness);
        check(&pass);
    }
    all.len()
}

impl Pass {
    /// Bring both seats up for one harness, against a scratch root of its own.
    fn place(tag: &str, harness: &'static dyn Harness) -> Self {
        let spec = harness.spec();
        let root = std::env::temp_dir().join(format!(
            "fleetor-conformance-{tag}-{}-{}-{:?}",
            spec.name,
            std::process::id(),
            std::thread::current().id(),
        ));
        let _ = std::fs::remove_dir_all(&root);
        let layout = Layout::under(root.join("state"));
        let target = root.join("repo");
        init_repo(&target);

        let context = conformance_context();
        let host = Host {
            fleet_bin: Some(root.join("fleet")),
            api_key: Some(WORKER_KEY.to_string()),
            ..Host::bare()
        };

        let orch = placement::place(PaneSpec::Orch, &layout, &host, &target, &context)
            .expect("placing the operator's own seat against a scratch layout");
        let worker =
            placement::place(PaneSpec::Worker(WORKER_SLOT), &layout, &host, &target, &context)
                .expect("placing a fenced worker against the same scratch layout");

        // The worker really is in its own checkout rather than the announced
        // fallback — otherwise every containment claim below would be about the
        // target repository instead of about the fleet's own tree.
        let worker_cwd = layout.worktree(&target, WORKER_SLOT);
        assert!(
            worker_cwd.join(".git").exists(),
            "the pass needs a worker in its own worktree, not the shared checkout",
        );

        let pass = Self {
            harness,
            spec,
            root,
            layout,
            target,
            context,
            host,
            orch,
            worker,
            worker_cwd,
        };
        pass.assert_placed_as_the_harness_under_test();
        pass
    }

    /// **Every seat was placed as the harness this pass is for.**
    ///
    /// Today this is trivially true, because exactly one harness is registered and
    /// `PaneSpec::harness` answers it for every pane. That is deliberate: this
    /// assertion is what fails, loudly and in every checkpoint at once, on the day
    /// a second harness is registered without `place` being able to be told which
    /// one to place. It is the seam's own tripwire, and it is here rather than in
    /// one checkpoint because it is a precondition of all of them.
    fn assert_placed_as_the_harness_under_test(&self) {
        for (seat, placed) in self.seats() {
            assert!(
                std::ptr::eq(placed.harness, self.spec),
                "{seat} was placed as {} while this pass is for {} — `place` has no way \
                 to be told which harness to place, so a second registered harness \
                 cannot be conformance-tested until it does",
                placed.harness.name,
                self.spec.name,
            );
        }
    }

    /// The two seats, named for failure messages: the attended one first.
    pub fn seats(&self) -> [(&'static str, &Placed); 2] {
        [("orch", &self.orch), ("worker", &self.worker)]
    }

    /// The unattended seat's own working directory, and the attended one's.
    pub fn cwd_of(&self, seat: &str) -> PathBuf {
        match seat {
            "orch" => self.target.clone(),
            _ => self.worker_cwd.clone(),
        }
    }

    /// The configuration directory a placed pane was actually pointed at —
    /// **read off the command rather than re-derived from the layout**, so a
    /// harness that seeds one directory and points the pane at another fails here
    /// instead of passing twice.
    pub fn config_dir(&self, placed: &Placed) -> PathBuf {
        let var = self.spec.config_dir.env_var;
        PathBuf::from(
            env_on(placed, var)
                .unwrap_or_else(|| panic!("the command must set {var}, checkpoint 4's own name")),
        )
    }

    /// Everything under a placed pane's configuration directory, as text where it
    /// is text. The seeded file, the settings file and anything else a harness
    /// writes are all in here, so a checkpoint can look for a key without knowing
    /// the file format.
    pub fn config_text(&self, placed: &Placed, file: &str) -> String {
        let path = self.config_dir(placed).join(file);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
    }

    /// Whether `path` is inside the layout this pass handed placement, or inside
    /// the target it handed it. Those two are the whole of what a placement may
    /// name.
    pub fn is_contained(&self, path: &Path) -> bool {
        let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        [self.layout.root().to_path_buf(), self.target.clone()].iter().any(|allowed| {
            let allowed = std::fs::canonicalize(allowed).unwrap_or_else(|_| allowed.clone());
            resolved.starts_with(&allowed)
        })
    }

    /// Place a second worker against another repository, for the checkpoints about
    /// what survives a target switch. The config directory is the same one, which
    /// is the whole point.
    pub fn place_worker_against(&self, name: &str) -> (PathBuf, Placed) {
        let other = self.root.join(name);
        init_repo(&other);
        let placed =
            placement::place(PaneSpec::Worker(WORKER_SLOT), &self.layout, &self.host, &other, &self.context)
                .expect("placing the same worker after a target switch");
        (other, placed)
    }
}

impl Drop for Pass {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

// --- reading what a placement produced ------------------------------------------

/// What `place` put in a command's environment.
pub fn env_on(placed: &Placed, key: &str) -> Option<String> {
    placed.command.get_env(key).map(|v| v.to_string_lossy().to_string())
}

/// A command's whole argv, program first.
pub fn argv_of(placed: &Placed) -> Vec<String> {
    placed.command.get_argv().iter().map(|a| a.to_string_lossy().into_owned()).collect()
}

/// The argument immediately following `flag`, or `None` when the flag is not
/// there. **Found by the flag rather than by position**, so a change to argument
/// order fails loudly rather than quietly asserting on the wrong thing.
pub fn arg_after(placed: &Placed, flag: &str) -> Option<String> {
    let argv = argv_of(placed);
    argv.iter().position(|a| a == flag).and_then(|i| argv.get(i + 1).cloned())
}

/// Every `--root` the installed guardrail hook was given, in order.
///
/// The hook's own spelling, not any harness's: the script, its arguments and the
/// journal are the fleet's and identical for every harness (checkpoint 7's doc
/// says so). What varies — the settings file, the event, the matcher — comes off
/// the spec instead.
pub fn roots_in(settings_text: &str) -> Vec<String> {
    let mut roots = Vec::new();
    let mut rest = settings_text;
    while let Some(at) = rest.find("--root '") {
        rest = &rest[at + "--root '".len()..];
        match rest.find('\'') {
            Some(end) => {
                roots.push(rest[..end].to_string());
                rest = &rest[end..];
            }
            None => break,
        }
    }
    roots
}

/// Every file under `dir` whose text contains `needle`, as paths relative to it.
/// A repository's own `.git` is skipped: it is git's bookkeeping, not something a
/// harness wrote into the operator's working tree.
pub fn files_containing(dir: &Path, needle: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(next) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.file_name().is_some_and(|n| n == ".git") {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if std::fs::read_to_string(&path).is_ok_and(|text| text.contains(needle)) {
                found.push(path.strip_prefix(dir).unwrap_or(&path).to_path_buf());
            }
        }
    }
    found.sort();
    found
}

/// Whether `dir` holds anything at all.
pub fn is_non_empty_dir(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_some())
}

/// The pane a seat name belongs to, for the layout lookups a checkpoint may want.
pub fn pane_of(seat: &str) -> PaneId {
    match seat {
        "orch" => PaneId::Orch,
        _ => PaneId::Worker(WORKER_SLOT),
    }
}

// --- the fixture ---------------------------------------------------------------

/// The briefs and launch values every pass hands in.
///
/// Each one is a scratch value nothing would produce by accident, which is what
/// turns "the command carries a model" into "the command carries *the model the
/// caller named*". The extra write roots are cleared explicitly: an operator's
/// `[fence] allow` is a legitimate widening (WP-17) and the containment claims
/// below are about what placement does on its own.
fn conformance_context() -> PaneContext {
    let mut context = PaneContext::baked();
    context.orch_template = format!("{ORCH_BRIEF_MARK}\n{}", context.orch_template);
    context.worker_template = format!("{WORKER_BRIEF_MARK}\n{}", context.worker_template);
    context.launch.worker_model = WORKER_MODEL.to_string();
    context.launch.worker_base_url = WORKER_BASE_URL.to_string();
    context.launch.worker_permission_mode = WORKER_POSTURE.to_string();
    context.launch.fence_allow = Vec::new();
    context
}

/// Make `dir` a real repository with one commit — what `git worktree add` needs
/// before it will succeed. Identity and signing are pinned per-command so the
/// result does not depend on the machine's own git configuration.
fn init_repo(dir: &Path) {
    std::fs::create_dir_all(dir).expect("scratch repository");
    std::fs::write(dir.join("README.md"), "scratch\n").expect("scratch repository file");
    run_git(dir, &["init", "-q", "-b", "main"]);
    run_git(dir, &["add", "-A"]);
    run_git(dir, &["commit", "-q", "-m", "first"]);
}

fn run_git(dir: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.email=conformance@fleetor.test"])
        .args(["-c", "user.name=Conformance"])
        .args(["-c", "commit.gpgsign=false"])
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(args)
        .output()
        .expect("git must be on PATH for the conformance suite");
    assert!(
        output.status.success(),
        "git {args:?} in {} failed: {}",
        dir.display(),
        String::from_utf8_lossy(&output.stderr),
    );
}
