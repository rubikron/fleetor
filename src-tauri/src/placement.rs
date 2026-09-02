//! Where a pane is placed: the one module that owns the order a pane is brought
//! up in (WP-21, D1–D3).
//!
//! Bringing a pane up is a sequence with a strict order — resolve the working
//! directory, seed the pane's configuration for that exact directory, install the
//! write guardrail, record the gauge source, build the command. Before this module
//! **no module owned that order**, so it was improvised at every call site: once
//! as a long match in [`crate::fleet`] with no tests, and once again in the
//! write-guardrail test, which needed the sequence, could not call it, retyped it
//! by hand and left a step out.
//!
//! ## The one rule
//!
//! **Nothing in here reads the process.** No home directory, no environment
//! variable, no configuration path resolved mid-call. Everything arrives in the
//! [`Layout`], the [`Host`], the target or the [`PaneContext`]. That is not a
//! style preference — it is the whole reason this module can be tested. The old
//! sequence bottomed out in functions that read the operator's real `$HOME` at
//! call time and took no argument, so it could only ever be run against the
//! operator's own installation.
//!
//! Two consequences follow, and both are load-bearing:
//!
//!  - **Every filesystem effect is confined to the layout it was handed.** Placing
//!    a pane against a scratch layout writes its configuration seed and its
//!    guardrail policy inside that directory and nowhere else, so the identical
//!    code path that runs in production runs in a test.
//!  - **Notices are returned, not emitted.** [`Placed::notices`] carries
//!    `(level, text)` pairs for the caller to put on the Activity feed — the same
//!    shape [`crate::prompts`] and [`crate::guardrail`] already use, and the reason
//!    this module needs no store.
//!
//! ## What is here yet
//!
//! [`PaneSpec`] has one variant. Workers, the evaluator and the Critic still come
//! up through the old sequence, and each gains its variant in the ticket that
//! moves it. A variant that existed but errored would be a trap of exactly the
//! kind `building.md` §6 names: the obvious next move on finding one is to point
//! it at something live, and the live path for those kinds is elsewhere. Every
//! variant that exists, works.

use std::path::{Path, PathBuf};

use fleetor_core::event::NoticeLevel;
use fleetor_core::pane::PaneId;
use portable_pty::CommandBuilder;

use crate::context_gauge::{self, TranscriptSource};
use crate::prompts::PaneContext;
use crate::{evaluator, guardrail, spawn};

// --- the layout ---------------------------------------------------------------

/// Where this fleet lives on disk (D1).
///
/// One value replacing the free functions that each re-derived the same tree from
/// the operator's real `$HOME`. It is a path and nothing else — deliberately not
/// also a place to keep what the machine has, which is [`Host`]'s job (D12): a
/// layout that also held a secret would no longer be a layout, and a path and a
/// credential have different lifetimes.
///
/// **Two real constructors**, so the seam is not hypothetical:
/// [`Layout::for_operator`] in production and [`Layout::under`] over any directory
/// a test hands it.
#[derive(Clone, Debug)]
pub struct Layout {
    root: PathBuf,
}

impl Layout {
    /// Production: the operator's `~/.fleetor`.
    ///
    /// **The one process-global read in the whole placement story, and it is
    /// deliberately out here rather than inside [`place`].** A relocatable state
    /// root is explicitly out of scope for this arc; what the arc needs is the root
    /// arriving as an argument with one production call site that still computes
    /// the real one. This is that call site.
    pub fn for_operator() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        Self { root: PathBuf::from(home).join(".fleetor") }
    }

    /// A test's: any directory at all. Everything below is derived from it, so a
    /// placement run against this layout cannot reach outside it.
    pub fn under(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The operator-facing root: holds `config.json` and the seeded testbed.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// State root, out of the user's repo so `rm -rf ~/.fleetor/_shell` fully
    /// undoes it (Tier-1 boundary).
    pub fn shell(&self) -> PathBuf {
        self.root.join("_shell")
    }

    /// The seeded project the fleet falls back to when no target is configured.
    pub fn testbed(&self) -> PathBuf {
        self.root.join("testbed")
    }

    /// Where the operator names the repo the fleet should work on — and, since
    /// WP-16, whether the app is in dev mode. One file, so there is one place an
    /// operator looks and one place `rm -rf ~/.fleetor` removes (Tier 1.1).
    pub fn config_file(&self) -> PathBuf {
        self.root.join("config.json")
    }

    /// The fleet unix socket the `fleet` CLI dials.
    pub fn socket(&self) -> PathBuf {
        self.shell().join("fleet.sock")
    }

    /// One pane's `CLAUDE_CONFIG_DIR`, by pane name.
    ///
    /// Deliberately *not* the Phase-2 `cc-config/worker-*` dirs: those were built
    /// by headless `-p` runs and carry no onboarding keys at all, which is
    /// precisely L1 (`docs/notes/tui-spawn-notes.md` §1).
    ///
    /// Every pane with a config dir lives under this one root, `orch` included
    /// since WP-14 — which is what makes `runs::harvest_transcripts` archive
    /// `orch`'s transcript with no code of its own: it already walks
    /// `pane-config/*`.
    pub fn pane_config(&self, pane: PaneId) -> PathBuf {
        self.shell().join("pane-config").join(pane.to_string())
    }

    /// A worker's private `HOME` (WP-08, the Fence): `~/.ssh`, the operator's real
    /// Claude config and shell profiles stop being reachable *by name* once this is
    /// what `HOME` resolves to instead. A natural sibling of `pane-config` and
    /// `worktrees` — same `_shell` root, same per-slot layout — and, like both of
    /// those, still under `~/.fleetor`, so `rm -rf ~/.fleetor` still removes
    /// everything FLEETOR made (Tier 1.1).
    pub fn worker_home(&self, slot: u8) -> PathBuf {
        self.shell().join("home").join(format!("worker-{slot}"))
    }

    /// The fleet's own `CARGO_HOME` and `RUSTUP_HOME` (D-069, the Fence's reversal
    /// condition exercised as written).
    ///
    /// **One pair, shared by every worker**, not one per slot: a build's registry
    /// cache is the same cache for all of them, two concurrent builds serialize on
    /// cargo's own package lock rather than corrupting anything, and the per-worker
    /// state that does need separating — `target/` — already lives in each
    /// worktree.
    ///
    /// Under `_shell` for the reason everything else here is: whatever a build
    /// downloads, however large, `rm -rf ~/.fleetor` reaches it (Tier 1.1).
    /// Pointing either of these at the operator's real `~/.cargo` or `~/.rustup`
    /// would make that sentence false — measured in `docs/notes/fence-notes.md`,
    /// arms 6b and 7b.
    pub fn fleet_toolchain(&self) -> spawn::FleetToolchain {
        spawn::FleetToolchain {
            cargo_home: self.shell().join("cargo"),
            rustup_home: self.shell().join("rustup"),
        }
    }

    /// A worker's own checkout of the target, namespaced by target repo so
    /// switching targets neither clobbers existing worktrees nor reuses stale ones.
    pub fn worktree(&self, target: &Path, slot: u8) -> PathBuf {
        self.shell().join("worktrees").join(target_slug(target)).join(format!("worker-{slot}"))
    }
}

/// The directory name a target's worktrees live under: its own name plus a short
/// hash of its absolute path, so two repos called `api` do not collide.
pub(crate) fn target_slug(target: &Path) -> String {
    let canonical = target.canonicalize().unwrap_or_else(|_| target.to_path_buf());
    let name = canonical.file_name().and_then(|n| n.to_str()).unwrap_or("repo");
    let mut hash = 0u32;
    for byte in canonical.to_string_lossy().bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(byte as u32);
    }
    format!("{name}-{:04x}", hash & 0xFFFF)
}

// --- the host -----------------------------------------------------------------

/// What this machine has (D12).
///
/// The layout answers *where this fleet writes*; this answers *what is out there
/// to find* — the operator's Rust toolchain, the built `fleet` binary, the worker
/// API key, the mission harness roots, and the stand-in pane override. Two
/// concepts, two names.
///
/// **Discovered per spawn, not once at bootstrap (D13.)** This is what the code
/// did before this module existed — the toolchain, the binary and the key were all
/// resolved at each spawn — and preserving it is what keeps this refactor pure. It
/// is also the better semantics: installing a toolchain or adding an `.env`
/// mid-session takes effect on the next pane restart rather than being invisible
/// until relaunch. The cost is a handful of filesystem checks, six times a session.
///
/// **Two real constructors:** [`Host::discover`] reads the machine, [`Host::bare`]
/// describes one that has nothing — which is how "no toolchain means those
/// settings are absent rather than empty" becomes a case a test can express.
#[derive(Clone, Debug, Default)]
pub struct Host {
    /// The built `fleet` binary, or `None` when this machine has none. A pane
    /// without it looks alive and cannot talk.
    pub fleet_bin: Option<PathBuf>,
    /// The stand-in pane program (`FLEETOR_PANE_CMD`), when one is set. Replaces
    /// `claude` and drops its flags — a stand-in is not obliged to understand them.
    pub pane_program: Option<String>,
    /// The operator's own `HOME`. `orch` is the operator's own `claude`, so its
    /// PATH gets rungs derived from this; a worker's deliberately does not.
    pub operator_home: Option<PathBuf>,
    /// The `PATH` the application itself was launched with.
    pub inherited_path: String,
    /// The operator's rustup, or `None` on a machine with no toolchain at all.
    pub toolchain: Option<spawn::OperatorToolchain>,
    /// The worker API key, or `None` — the orchestrator runs without one, worker
    /// panes cannot.
    pub api_key: Option<String>,
    /// Where prepared mission workspaces, the harness and the answer keys live.
    pub missions: evaluator::MissionRoots,
}

impl Host {
    /// Read this machine. Every field resolved the way the spawn path resolved it
    /// before this value existed, at the same moment it did (D13).
    pub fn discover() -> Self {
        Self {
            fleet_bin: spawn::fleet_bin_path(),
            pane_program: spawn::pane_program(),
            operator_home: std::env::var_os("HOME").map(PathBuf::from),
            inherited_path: std::env::var("PATH").unwrap_or_default(),
            toolchain: spawn::operator_toolchain(),
            api_key: crate::fleet::deepseek_api_key(),
            missions: evaluator::MissionRoots::discover(),
        }
    }

    /// A machine with nothing on it. Not a mock — a real value describing a real
    /// possible machine, which is what makes the absent-toolchain and
    /// absent-binary branches testable at all.
    pub fn bare() -> Self {
        Self::default()
    }

    /// The `PATH` `orch` runs with: the `fleet` binary's directory, the operator's
    /// own tool dirs, the system dirs, then whatever the app inherited.
    fn orch_path(&self) -> String {
        let home = self.operator_home.as_deref().map(Path::to_string_lossy).unwrap_or_default();
        spawn::augmented_path_from(self.fleet_bin.as_deref(), &home, &self.inherited_path)
    }
}

// --- what to place ------------------------------------------------------------

/// Which pane to place, carrying exactly what placing that kind needs (D8).
///
/// Deliberately *not* [`PaneId`], which is a frozen wire type serialising as a
/// bare string and shared by the CLI argument, the database payload, the event
/// field and the TypeScript mirror. This is internal, and each kind carries its
/// own inputs, so a future pane kind costs one variant rather than another
/// argument two of the three arms discard.
#[derive(Clone, Debug)]
pub enum PaneSpec {
    /// The operator's own `claude`, in the target itself.
    Orch,
}

impl PaneSpec {
    /// The wire identity this spec places.
    pub fn pane(&self) -> PaneId {
        match self {
            PaneSpec::Orch => PaneId::Orch,
        }
    }
}

/// Everything bringing one pane up produced, and nothing it did along the way.
pub struct Placed {
    /// What the registry will spawn.
    pub command: CommandBuilder,
    /// For the Activity feed, in the order the operator should read them.
    /// Returned rather than emitted: this module holds no store, and a test can
    /// assert on them.
    pub notices: Vec<(NoticeLevel, String)>,
    /// Where this pane's transcript will live, for the WP-04 live gauge. `None`
    /// for panes the gauge does not track.
    pub gauge: Option<TranscriptSource>,
}

// --- placing ------------------------------------------------------------------

/// Bring one pane up: resolve its cwd, seed its config for that exact cwd, install
/// its guardrail, and build its command — in that order, once, here.
///
/// **The L1 re-seed requirement lives in this function, structurally.**
/// `hasTrustDialogAccepted` is keyed by absolute project path, so a fleet pointed
/// at a new target needs its seed re-applied for every pane cwd — otherwise the
/// panes sit on a trust dialog while every `fleet send` reports success. Seeding
/// here rather than at the target picker means that can only be got wrong by
/// deleting a line, not by forgetting a code path.
pub fn place(
    spec: PaneSpec,
    layout: &Layout,
    host: &Host,
    target: &Path,
    context: &PaneContext,
) -> Result<Placed, String> {
    match spec {
        PaneSpec::Orch => place_orch(layout, host, target, context),
    }
}

/// The operator's own `claude` — their login, their model, their `HOME` — in the
/// target itself.
///
/// Since WP-14 it has one thing of its own: a fleet-owned `CLAUDE_CONFIG_DIR`, so
/// its session transcript lands under the layout where rotation archives it with
/// the run (D-062 closes D-059's named gap). That makes L1 apply to `orch` too —
/// an unseeded config dir never reaches a prompt — so it is seeded here for
/// exactly the reason a worker's is.
///
/// Nothing here reaches into the operator's own config dir. `orch` gets a new
/// directory going forward, and keeps its login through the keychain read `claude`
/// already performs (see [`spawn::orch_command_with`]).
fn place_orch(
    layout: &Layout,
    host: &Host,
    target: &Path,
    context: &PaneContext,
) -> Result<Placed, String> {
    let pane = PaneId::Orch;
    let mut notices = Vec::new();

    // A pane with no `fleet` on its PATH is a pane that looks alive and cannot
    // talk. Say so once, loudly, rather than letting the model discover it as
    // `command not found` mid-turn (L4).
    if host.fleet_bin.is_none() {
        notices.push((NoticeLevel::Warn, MISSING_FLEET_BIN.to_string()));
    }

    std::fs::create_dir_all(target).map_err(|e| format!("create orchestrator cwd: {e}"))?;
    let config_dir = layout.pane_config(pane);
    spawn::seed_config_dir(&config_dir, target)?;
    notices.extend(guardrail_notices(layout, pane, &config_dir, target, context)?);

    // One Activity line per pane launch (WP-04's spawn-time "Loadout" counter):
    // the size of the brief this pane was just handed, estimated from text already
    // in memory — never a file read, never a tokenizer call.
    let rendered = fleetor_core::brief::render_orch(
        &context.orch_template,
        &fleetor_core::pane::PaneId::roster(&fleetor_core::pane::WORKER_SLOTS),
        &target.display().to_string(),
    );
    notices.push((
        NoticeLevel::Info,
        context_gauge::spawn_estimate_notice_text(pane, &rendered, None),
    ));
    notices.push((NoticeLevel::Info, orch_config_dir_notice(&config_dir)));

    let command = spawn::orch_command_with(
        target,
        &layout.socket(),
        &config_dir,
        context,
        host.pane_program.as_deref(),
        &host.orch_path(),
    );

    // `orch` is not on the live gauge: the Loadout counter is a per-run budget line
    // for the panes doing the work, and the gauge samples worker transcripts.
    Ok(Placed { command, notices, gauge: None })
}

/// What the operator is told when the `fleet` binary is nowhere. Pinned by a test
/// for the same reason [`orch_config_dir_notice`] is.
pub const MISSING_FLEET_BIN: &str =
    "the `fleet` binary was not found — panes will spawn but cannot message each other. \
     Build it with `cargo build -p fleetor-cli --bin fleet`.";

/// Install this pane's write guardrail (WP-17), inside the sequence for the same
/// reason the config seed is: the roots are derived from the cwd this pane is
/// actually about to run in, so they cannot be got wrong by a code path that
/// forgot to recompute them.
///
/// `cwd` is the pane's own — `orch`'s target repo, a worker's worktree, or (in the
/// shared-checkout fallback) the target, which degrades the guardrail exactly the
/// way `shared_checkout_warning` says peer review degrades rather than inventing a
/// directory that is not there.
///
/// `pub(crate)` because the old sequence in [`crate::fleet`] still places the
/// other pane kinds and calls this — one implementation, two callers, rather than
/// the hand-copied second copy this module exists to abolish.
pub(crate) fn guardrail_notices(
    layout: &Layout,
    pane: PaneId,
    config_dir: &Path,
    cwd: &Path,
    context: &PaneContext,
) -> Result<Vec<(NoticeLevel, String)>, String> {
    let shell = layout.shell();
    // **The evaluator's roots are narrower than any pane's, deliberately.** It must
    // read everything the run produced and change almost nothing — its own stated
    // boundary is that running a test suite is fine and editing a tracked file,
    // committing or touching a branch is not. So it gets its working directory and
    // nothing else: not `_shell` (which would let it write the live event log it is
    // reading), and not the operator's `[fence] allow` extras, which exist for panes
    // that are doing the work. Reads are untouched for every pane alike — the hook
    // is not registered for `Read` at all (D-065), which is what makes "read
    // everything" true without a rule.
    let roots = if pane.is_evaluator() {
        vec![cwd.to_path_buf()]
    } else {
        guardrail::roots_for(cwd, &shell, &context.launch.fence_allow)
    };
    guardrail::install(
        config_dir,
        pane,
        &roots,
        &guardrail::policy_dir(&shell),
        &guardrail::journal_path(&shell),
    )
}

/// What the operator is told when `orch` spawns on its own config dir (WP-14).
///
/// Kept separate from [`place_orch`] so the wording is pinned by a test: this is
/// the one notice standing between the operator and a pane that looks perfectly
/// healthy while being logged out. `orch` keeps its login through
/// `CLAUDE_SECURESTORAGE_CONFIG_DIR`, which is an internal of Claude Code and
/// version-stamped at 2.1.224 in `docs/notes/orch-config-dir-notes.md`. If a future
/// release stops honouring it, `orch` boots into an empty credential namespace,
/// reaches its input box, and fails on its first turn while every `fleet send`
/// reports `accepted`. The fix is one `/login` inside the pane, and it sticks — so
/// the operator needs the sentence more than they need the machinery to detect it.
fn orch_config_dir_notice(config_dir: &Path) -> String {
    format!(
        "orch is running on the fleet's own config dir at {}, so its transcript is \
         archived with the run. It keeps your login. If the orch pane says \
         “Not logged in · Run /login”, run `/login` inside that pane once — it \
         persists, and it cannot disturb your own `claude`.",
        config_dir.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scratch_layout_derives_everything_under_itself() {
        let layout = Layout::under("/scratch/fleet");
        for path in [
            layout.shell(),
            layout.testbed(),
            layout.config_file(),
            layout.socket(),
            layout.pane_config(PaneId::Orch),
            layout.worker_home(2),
            layout.fleet_toolchain().cargo_home,
            layout.fleet_toolchain().rustup_home,
            layout.worktree(Path::new("/some/repo"), 1),
        ] {
            assert!(
                path.starts_with("/scratch/fleet"),
                "{} escaped the layout it was derived from",
                path.display()
            );
        }
    }

    #[test]
    fn a_bare_host_has_nothing_on_it() {
        let host = Host::bare();
        assert!(host.fleet_bin.is_none(), "a bare machine has no built `fleet`");
        assert!(host.pane_program.is_none(), "a bare machine has no stand-in override");
        assert!(host.toolchain.is_none(), "a bare machine has no rustup");
        assert!(host.api_key.is_none(), "a bare machine has no worker key");
    }

    /// Moved here with the notice itself when `orch` moved onto the placement
    /// seam. WP-14 put `orch` on a fleet-owned config dir, and the variable that
    /// keeps its login is a Claude Code internal. If a release stops honouring it
    /// the pane still looks healthy — input box, `accepted` on every send — and
    /// only its turns fail. So the notice has to name the string the pane will
    /// show and the move that fixes it, not merely announce that a directory
    /// changed.
    #[test]
    fn the_orch_config_dir_notice_says_what_to_do_if_the_login_did_not_carry() {
        let text = orch_config_dir_notice(Path::new("/Users/me/.fleetor/_shell/pane-config/orch"));
        assert!(text.contains("/Users/me/.fleetor/_shell/pane-config/orch"), "{text}");
        assert!(text.contains("archived with the run"), "why it moved at all: {text}");
        assert!(text.contains("Not logged in"), "the exact string the pane would show: {text}");
        assert!(text.contains("/login"), "the move that fixes it: {text}");
        assert!(
            text.contains("cannot disturb your own"),
            "and that the fix is safe to run — this is the operator's own claude: {text}",
        );
    }

    #[test]
    fn the_operator_layout_is_the_dot_fleetor_tree() {
        // The one process-global read, and the shape it must produce. Asserted
        // relatively so it holds whatever `$HOME` the test runner has.
        let layout = Layout::for_operator();
        assert!(layout.root().ends_with(".fleetor"));
        assert_eq!(layout.shell(), layout.root().join("_shell"));
        assert_eq!(layout.socket(), layout.shell().join("fleet.sock"));
    }
}
