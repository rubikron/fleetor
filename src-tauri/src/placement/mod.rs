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
//! [`PaneSpec`] has four variants: `orch`, a worker, the evaluator and the Critic
//! — every pane kind that can exist today. Every one of them works, which is the
//! rule this module keeps: a variant that existed but errored would be a trap of
//! exactly the kind `building.md` §6 names, since the obvious next move on
//! finding one is to point it at something live.
//!
//! **[`RunSource`] is the one place that rule is bent, and deliberately** (D-076).
//! The Critic carries the run it is critiquing, and a run is either the live one
//! or an archived one; the shape is the design prototype's and both variants are
//! declared here so that the type says what a Critic is rather than what this
//! ticket got to. Only [`RunSource::Live`] is built. [`RunSource::Archived`] is
//! constructed nowhere in the application — no UI offers it, no caller names it —
//! and [`place_critic`] answers it with [`ARCHIVED_NOT_BUILT`] rather than with a
//! half-built placement. The §6 trap does not apply, because there is no other
//! live path for it to be re-pointed at: reading an archived run does not exist
//! yet anywhere.
//!
//! ## The one read that is not from the process, and is not an argument either
//!
//! [`Layout::dev_enabled`] opens a file. That is not a hole in the rule above, it
//! is the rule applied: the file is `config.json` **inside the layout placement
//! was handed**, so a test pointing the layout at a scratch directory points the
//! mode at a scratch directory too. It exists because the evaluator arm has to
//! re-check the mode *itself* rather than trust its caller (D11) — "unreachable
//! outside dev mode" is a property of the spawn site or it is not a property at
//! all — and because the mode is documented as read fresh at every wake rather
//! than snapshotted. Taking a `bool` argument would have lost the first of those.
//!
//! ## The one thing this module decides and does not do
//!
//! [`machine_notices`] says what a machine is missing — a `fleet` binary, a rustup
//! — and the caller emits it *before* calling [`place`]. That split is ordering,
//! not an exception to the rules above: those lines have to reach the operator even
//! when the placement that follows returns `Err`, and a machine with no worker key
//! would otherwise be told only about the key. The sentences and the conditions are
//! here; only the emit is out there.

use std::path::{Path, PathBuf};

use fleetor_core::event::NoticeLevel;
use fleetor_core::pane::PaneId;
use portable_pty::CommandBuilder;

use crate::context_gauge::{self, TranscriptSource};
use crate::prompts::PaneContext;
use crate::{critic, dev, evaluator, guardrail, runs};

/// How a pane's `claude` process is shaped — **an internal of this module** since
/// D-075.
///
/// It was a sibling of `placement` while the improvised bring-up sequence still
/// existed and could call it. Every seeding and command-building function in it
/// has exactly one caller, which is `placement`, so the seam it presented was
/// hypothetical rather than real: those items are `pub(super)` and the module is a
/// child, which together mean nothing outside this file can call them at all — let
/// alone call them in the wrong order.
///
/// The one item that is still `pub` is [`spawn::project_key`], which is neither
/// seeding nor command-building: it is **Claude Code's own** project-key
/// canonicalization — checkpoint 14's answer for one vendor rather than the
/// fleet's for all of them (WP-25, M23). Its two production consumers, the trust
/// flag and the context gauge, now reach it through
/// [`harness::Harness::project_key`] instead of by name, so that they agree on one
/// spelling of a cwd *because they asked the same harness* and not because they
/// happened to call the same function. What is left public is the implementation
/// behind that method, still read directly by the delivery and gauge tests that
/// pin the transcript slug rule until their own batches move.
pub mod spawn;

/// What a harness *is* — the fourteen checkpoints, answered once per vendor
/// (WP-25, M23, C17).
///
/// It lives here rather than in a crate of its own for the reason M23 records:
/// almost every vendor-specific reference in this codebase is already inside
/// `placement` and [`crate::guardrail`], so a crate would invert the dependency
/// direction and pull the guardrail, delivery, the orphan sweep and the context
/// gauge in behind it.
///
/// **Nothing outside this module reads a field of it yet.** It is the expand half
/// of a wide refactor: the literals in [`spawn`] and its siblings are still what
/// runs, and this is the form they move onto. See the module's own doc comment for
/// what that means and what is asserted in the meantime.
pub mod harness;

/// **Codex — the second harness's answers, and the first non-Claude one** (WP-25
/// phase 2, #26).
///
/// It is deliberately **not registered** while phase 2 is in flight: the
/// conformance suite's exit condition is one harness until every checkpoint has
/// landed, and #33 is the single moment codex joins the registry. See the module's
/// own header for which checkpoints are answered and which are still their own
/// ticket's.
pub mod codex;

pub use harness::{Harness, HarnessSpec, Seed};

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

    /// Whether *this* installation is in dev mode (D11, D-061).
    ///
    /// The stored flag, read through the layout rather than through the
    /// process-global reader — which is what lets [`place_evaluator`] re-check the
    /// mode at the spawn site while a test still points it at a scratch
    /// `config.json`. Three properties at once, and dropping any one of them was a
    /// rejected alternative: the spawn site genuinely re-checks so a caller cannot
    /// lie to it, the flag is read fresh at every placement rather than
    /// snapshotted, and the operator's real `~/.fleetor/config.json` is never
    /// consulted by a test.
    ///
    /// A missing, unreadable or malformed config reads as off, exactly as
    /// [`crate::dev::is_enabled`] does — this is that function with the file named
    /// instead of derived.
    pub fn dev_enabled(&self) -> bool {
        dev::read_at(&self.config_file())
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
        pane_config_root(&self.shell()).join(pane.to_string())
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

/// The root every pane's `CLAUDE_CONFIG_DIR` lives under — **the one spelling of
/// it in the whole shell** (D1, D-075).
///
/// There were four before this ticket, each `shell.join("pane-config")` written
/// out again: [`Layout::pane_config`], [`crate::guardrail::policy_dir`], and the
/// two transcript scans in [`crate::runs`]. That is not a style complaint. The
/// four are not independent facts — the guardrail denies writes to exactly the
/// directory the layout puts configs in, and rotation archives a transcript by
/// walking exactly that directory — so a rename that reached three of them would
/// leave a fleet whose panes can write their own hook policy, or whose
/// orchestrator's transcript stops being archived, and no test would notice.
///
/// Takes the `_shell` path rather than the whole [`Layout`], because the two
/// callers outside this module hold one and not the other; every one of them now
/// derives the name from here instead of restating it.
pub(crate) fn pane_config_root(shell: &Path) -> PathBuf {
    shell.join("pane-config")
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
    /// Where the search for that key began: the application's own working
    /// directory, which is the head of the `.env` walk
    /// (`crate::fleet::api_key_search_start`). `None` on a machine nobody looked
    /// at.
    ///
    /// **A field only so a refusal can name a path (D-075).** The sentence a
    /// worker fails with used to say where the key was looked for and lost that
    /// clause when it became a constant here (D-072); "no key found" and "no key
    /// found under `/Users/me/code/api`" are the same fact and only the second one
    /// tells an operator whose `.env` is one directory up what to do. It is on the
    /// [`Host`] rather than read in [`place`] for the reason everything else here
    /// is.
    pub api_key_searched_from: Option<PathBuf>,
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
            api_key_searched_from: Some(crate::fleet::api_key_search_start()),
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

    /// The `PATH` a worker runs with (WP-08, the Fence): the `fleet` binary's
    /// directory, the fleet's own `cargo/bin` shims when this machine has a
    /// toolchain at all, the system dirs, then whatever the app inherited.
    ///
    /// **[`Self::operator_home`] is not read here, and that is the point.** It is
    /// the one field the two path methods differ on: `orch` is the operator's own
    /// `claude` and gets their tool rungs, a worker is fenced and must not reach
    /// the operator's tooling by an unqualified command name.
    fn worker_path(&self, cargo_bin: Option<&Path>) -> String {
        spawn::worker_augmented_path_from(
            self.fleet_bin.as_deref(),
            cargo_bin,
            &self.inherited_path,
        )
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
    /// One fenced worker, by slot: its own worktree of the target, its own
    /// private `HOME`, and the fleet's toolchain rather than the operator's.
    Worker(u8),
    /// The evaluator (WP-15), in a snapshot of the live run.
    ///
    /// It carries no inputs of its own, and that is D2 rather than an omission:
    /// the run it reads is the live one, laid out *by* placement, so there is
    /// nothing for a caller to pass in and no directory it could point at that
    /// placement did not write.
    Evaluator,
    /// The Critic (WP-20, D-076), in the run it is critiquing.
    ///
    /// **The one variant that carries an input, and the input is which run.**
    /// The evaluator's run is the live one by construction — it wakes on a
    /// handoff, which only a live run produces. The Critic is opened by the
    /// operator, who can be looking at the run in progress or at a row in
    /// History, so *which run* is a real choice a caller makes and therefore
    /// travels in the spec rather than being re-derived somewhere deeper.
    Critic { run: RunSource },
}

/// Which run a Critic is pointed at (WP-20, D-076).
///
/// **Both variants are declared and only [`RunSource::Live`] is built.** See this
/// module's "What is here yet" for why the type says what a Critic is rather than
/// what this ticket got to, and what placement does with the other one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunSource {
    /// The run in progress, laid out by placement the way the evaluator's is —
    /// `events.json`, `manifest.json` and every pane's transcript, copied rather
    /// than moved, from a database that is still being written.
    Live,
    /// A past run, by its archive id. Nothing constructs this yet.
    Archived(String),
}

impl PaneSpec {
    /// The wire identity this spec places.
    pub fn pane(&self) -> PaneId {
        match self {
            PaneSpec::Orch => PaneId::Orch,
            PaneSpec::Worker(slot) => PaneId::Worker(*slot),
            PaneSpec::Evaluator => PaneId::Evaluator,
            PaneSpec::Critic { .. } => PaneId::Critic,
        }
    }

    /// Which harness this pane runs (WP-25).
    ///
    /// **The one lookup, and it is here because the harness is a property of the
    /// pane rather than of the machine or the fleet's directory tree.** A mixed
    /// fleet is the point of the arc this belongs to — the gate offers a harness
    /// per role (C23) — so the answer has to be able to differ between two panes
    /// of the same run, which rules out [`Host`] (what the *machine* has) and
    /// [`Layout`] (a path and nothing else).
    ///
    /// It ignores `self` today because exactly one harness is registered, and
    /// registering a second is a later ticket's work rather than something this
    /// method should pretend to already do. What it buys now is that
    /// [`place`] and every arm below it hold the answer as a value, so a call site
    /// that migrates onto [`HarnessSpec`] finds it already in scope.
    pub fn harness(&self) -> &'static dyn Harness {
        harness::claude_code()
    }
}

/// Everything bringing one pane up produced, and nothing it did along the way.
///
/// `Debug` so a test asserting a *refusal* can say what it got instead — the
/// refusal arms are the ones with no other output to print.
#[derive(Debug)]
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
    /// Which harness this pane was placed as (WP-25).
    ///
    /// **Returned rather than assumed**, for the reason [`Placed::gauge`] is: the
    /// caller has to write this down — `manifest.json` records a pane's harness,
    /// model and transcript format so a Critic reading a mixed run cold knows what
    /// it is holding (M24) — and a fact the caller re-derives is a fact that can
    /// disagree with the one placement acted on.
    ///
    /// Nothing reads it yet. It is here so that the ticket which writes it into
    /// the manifest finds it already carried, rather than having to thread it back
    /// through four arms.
    pub harness: &'static HarnessSpec,
    /// **Checkpoint 5's scrub, as data:** the credential names placement removed
    /// from the environment this pane's command inherited (WP-25, C27).
    ///
    /// Empty for every attended seat, and that emptiness is the asymmetry rather
    /// than a missing answer — the operator's own seat runs their login and has
    /// nothing to scrub. A fenced worker's is
    /// [`Credentials::scrubbed_env`](harness::Credentials::scrubbed_env).
    ///
    /// **Returned because `env_remove` deletes rather than marks.** On the
    /// finished command a name that was scrubbed and a name the machine never
    /// exported are the same observation, so "this pane had the operator's key
    /// removed" was not a thing a caller could see without first putting one in
    /// the process environment. It is not the *whole* of what a placement removes
    /// — `CLAUDE_CODE_CHILD_SESSION` goes from every pane as terminal hygiene, and
    /// is nobody's checkpoint — it is checkpoint 5's scrub and nothing else.
    pub scrubbed: &'static [&'static str],
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
    // Which harness this pane runs, resolved once here rather than per arm
    // (WP-25). Every arm below is handed it and hands it back on `Placed`; none of
    // them looks it up again, so "which harness is this pane" has exactly one
    // answer per placement by construction.
    let harness = spec.harness();
    match spec {
        PaneSpec::Orch => place_orch(harness, layout, host, target, context),
        PaneSpec::Worker(slot) => place_worker(harness, slot, layout, host, target, context),
        PaneSpec::Evaluator => place_evaluator(harness, layout, host, target, context),
        PaneSpec::Critic { run } => place_critic(harness, run, layout, host, context),
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
    harness: &'static dyn Harness,
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

    // This pane's brief, rendered once — **before the seed**, because a harness
    // whose brief carrier is a config key writes it into the configuration
    // directory in the same pass that writes the rest of the seed (#27, C3). A
    // harness that carries it in argv ignores the field.
    let rendered = fleetor_core::brief::render_orch(
        &context.orch_template,
        &fleetor_core::pane::PaneId::roster(&fleetor_core::pane::WORKER_SLOTS),
        &target.display().to_string(),
    );

    let config_dir = layout.pane_config(pane);
    notices.extend(harness.seed_config_dir(
        &Seed::new(&config_dir, target, host.operator_home.as_deref())
            .with_brief(&rendered)
            .for_the_operator(),
    )?);
    notices.extend(guardrail_notices(harness, layout, pane, &config_dir, target, context, true)?);

    // One Activity line per pane launch (WP-04's spawn-time "Loadout" counter):
    // the size of the brief this pane was just handed, estimated from text already
    // in memory — never a file read, never a tokenizer call.
    notices.push((
        NoticeLevel::Info,
        context_gauge::spawn_estimate_notice_text(pane, &rendered, None),
    ));
    notices.push((NoticeLevel::Info, orch_config_dir_notice(&config_dir, harness.spec())));

    let command = spawn::orch_command_with(
        harness,
        target,
        &layout.socket(),
        &config_dir,
        context,
        host.pane_program.as_deref(),
        &host.orch_path(),
    );

    // `orch` is not on the live gauge: the Loadout counter is a per-run budget line
    // for the panes doing the work, and the gauge samples worker transcripts.
    Ok(Placed { command, notices, gauge: None, harness: harness.spec(), scrubbed: &[] })
}

/// One fenced worker: its own checkout of the target, its own `HOME`, the fleet's
/// own toolchain, and Flash on DeepSeek.
///
/// **Everything a worker needs before its process exists is decided and made true
/// here, in this order**, and the order is the old sequence's exactly: the working
/// directory first, because everything after it is keyed to that directory; then
/// the configuration seed for that exact directory (L1); then the private `HOME`
/// with the git identity a worker's commits need (WP-08); then the fleet-owned
/// toolchain, or its deliberate absence (D-069); then the guardrail roots; then
/// the gauge source; then the command.
///
/// Two of those steps are conditional and both conditions live on the [`Host`],
/// never on the process: a target that is not a git repository degrades to the
/// shared checkout with a notice, and a machine with no rustup gets the toolchain
/// settings **absent** rather than pointing at a directory that does not exist.
fn place_worker(
    harness: &'static dyn Harness,
    slot: u8,
    layout: &Layout,
    host: &Host,
    target: &Path,
    context: &PaneContext,
) -> Result<Placed, String> {
    let pane = PaneId::Worker(slot);
    let mut notices = Vec::new();

    // Before anything is written: a worker with no key cannot authenticate at all,
    // and failing here costs nothing, where failing after four filesystem seeds
    // would leave them behind. This is the old arm's first line too.
    let key = host
        .api_key
        .as_deref()
        .ok_or_else(|| missing_api_key(host.api_key_searched_from.as_deref()))?;

    // Its own git worktree, or the announced fallback to the shared checkout. The
    // notice is returned rather than logged, which is what makes the degraded case
    // assertable for the first time.
    let cwd = match ensure_worktree(layout, target, slot) {
        Ok(dir) => dir,
        Err(why) => {
            notices.push((NoticeLevel::Warn, shared_checkout_warning(slot, &why, target)));
            target.to_path_buf()
        }
    };

    // This pane's brief, rendered once — before the seed, for `place_orch`'s
    // reason (#27, C3).
    let rendered = fleetor_core::brief::render_worker(
        &context.worker_template,
        pane,
        &fleetor_core::pane::PaneId::roster(&fleetor_core::pane::WORKER_SLOTS),
        &cwd.display().to_string(),
    );

    let config_dir = layout.pane_config(pane);
    notices.extend(harness.seed_config_dir(
        &Seed::new(&config_dir, &cwd, host.operator_home.as_deref()).with_brief(&rendered),
    )?);

    // The Fence (WP-08): a private HOME, created and seeded before the process
    // exists — same reason the config dir is seeded here rather than at the target
    // picker (see [`place`]'s doc comment).
    let home = layout.worker_home(slot);
    spawn::seed_worker_home(&home, slot)?;

    // The Fence learns about rustup (D-069). `None` when this machine has no
    // rustup at all, and then the two settings are absent from the command rather
    // than naming a directory that is not there — a `RUSTUP_HOME` pointing at
    // nothing makes rustup try to *install* there, which is worse than today's
    // `command not found`.
    let toolchain = match &host.toolchain {
        Some(operator) => {
            let fleet = layout.fleet_toolchain();
            spawn::seed_fleet_toolchain(&fleet, operator)?;
            Some(fleet)
        }
        None => None,
    };

    notices.extend(guardrail_notices(harness, layout, pane, &config_dir, &cwd, context, false)?);

    // The WP-04 spawn-time "Loadout" counter, from text already in memory.
    let rendered = fleetor_core::brief::render_worker(
        &context.worker_template,
        pane,
        &fleetor_core::pane::PaneId::roster(&fleetor_core::pane::WORKER_SLOTS),
        &cwd.display().to_string(),
    );
    notices.push((
        NoticeLevel::Info,
        context_gauge::spawn_estimate_notice_text(
            pane,
            &rendered,
            Some(context_gauge::WORKER_WINDOW_TOKENS),
        ),
    ));

    let cargo_bin = toolchain.as_ref().map(|t| t.cargo_home.join("bin"));
    let worker = spawn::worker_command_with(
        harness,
        slot,
        &cwd,
        &home,
        &config_dir,
        &layout.socket(),
        key,
        toolchain.as_ref(),
        context,
        host.pane_program.as_deref(),
        &host.worker_path(cargo_bin.as_deref()),
    );

    // The live gauge's source of truth, returned so the caller records it **before
    // the process exists** — a `fleet roster` landing before this pane's first turn
    // samples the not-yet-there transcript as absent, never a stale pane's numbers.
    Ok(Placed {
        command: worker.command,
        notices,
        gauge: Some(TranscriptSource { harness, config_dir, cwd }),
        harness: harness.spec(),
        scrubbed: worker.scrubbed,
    })
}

/// The evaluator (WP-15): the operator's own `claude` in a snapshot of the run it
/// is about to read.
///
/// **It re-checks the mode itself (D11).** Its one caller has already asked
/// [`evaluator::readiness`] the same question, and that is not enough: "there is no
/// evaluator outside dev mode" has to be a property of the spawn site rather than of
/// whoever happens to call it. What changed with this module is *how* it re-checks —
/// through [`Layout::dev_enabled`] and [`Host::missions`], so the identical branch a
/// production spawn takes can be taken against a scratch configuration.
///
/// **It lays the run out before it renders anything (D2).** The evaluator's working
/// directory *is* a snapshot of the run, so there is no order in which a caller could
/// usefully do this first: the brief cites the directory, and a brief citing a
/// directory nobody wrote is a pane that spends its first turn asking about a path.
///
/// Shaped like `orch` — the operator's account and model, their `HOME`, a full
/// environment inherit, no Fence — and unlike it in the three ways that are about
/// the veil rather than about permissions: its brief comes from a separate repo with
/// no `~/.fleetor/prompts/` override, its config dir is outside `pane-config/` so its
/// own reasoning never lands in the archive the next generation reads, and its
/// guardrail roots are its own working directory alone.
fn place_evaluator(
    harness: &'static dyn Harness,
    layout: &Layout,
    host: &Host,
    target: &Path,
    context: &PaneContext,
) -> Result<Placed, String> {
    let pane = PaneId::Evaluator;

    let evaluator::Readiness::Ready(mission) =
        evaluator::readiness(layout.dev_enabled(), target, &host.missions)
    else {
        return Err(NO_EVALUATOR.to_string());
    };

    // The run, laid out for reading. `snapshot_live_run` owns the directory it is
    // given — it clears and recreates it — so nothing may be seeded into the cwd
    // before this line.
    let shell = layout.shell();
    let run_id = runs::live_run_id(&shell, fleetor_core::time::now_ms());
    let cwd = evaluator::retro_dir(layout.root(), &run_id);
    runs::snapshot_live_run(&shell, &cwd, &run_id)?;

    let brief = evaluator::render_brief(&mission, &cwd)?;

    let config_dir = evaluator::config_dir(layout.root());
    let mut notices = harness.seed_config_dir(
        &Seed::new(&config_dir, &cwd, host.operator_home.as_deref()).with_brief(&brief),
    )?;
    notices.extend(guardrail_notices(harness, layout, pane, &config_dir, &cwd, context, false)?);

    let command = spawn::evaluator_command_with(
        harness,
        &cwd,
        &layout.socket(),
        &config_dir,
        &brief,
        &context.launch.worker_permission_mode,
        host.pane_program.as_deref(),
        &host.orch_path(),
    );

    // Not on the live gauge, for `orch`'s reason: the gauge samples the transcripts
    // of the panes doing the work, and this pane is not one of them.
    Ok(Placed { command, notices, gauge: None, harness: harness.spec(), scrubbed: &[] })
}

/// The Critic (WP-20, D-076): the operator's own `claude` in the run it is
/// critiquing, with no way back into the fleet.
///
/// **Three things it does not do, and each is the identity rather than an
/// omission:**
///
///  - **It does not check dev mode.** It is a product feature. There is no
///    readiness question to ask, because there is no answer key it could be
///    missing — it reports what the archive proves and nothing else.
///  - **It does not take the target.** The Critic reads a run, and a run is a
///    directory placement lays out; the repository the fleet was pointed at is
///    named inside `manifest.json`, where the Critic reads it like every other
///    fact. A pane that reads a run has no business holding the live checkout.
///  - **It is given a socket, and it is still not in the fleet** (WP-21, D-079).
///    [`spawn::critic_command_with`] hands it `FLEET_SOCKET` and `FLEETOR_PANE`
///    exactly as [`place_evaluator`] does, because a socket baked in at spawn is
///    the only kind there is — the operator's switch cannot add one later
///    without respawning the pane. What the switch gates instead is the hub:
///    while the interview is closed every op from `critic` is refused before it
///    resolves anything. `PaneId::Critic.is_fleet_member()` is `false` either
///    way, so this adds no roster row, no broadcast leg and no peer-list entry.
///
/// **It lays the run out before it renders anything**, for the reason
/// [`place_evaluator`] does (D2): the working directory *is* the run, the brief
/// cites that directory, and a brief citing a directory nobody wrote is a pane
/// that spends its first turn asking about a path.
fn place_critic(
    harness: &'static dyn Harness,
    run: RunSource,
    layout: &Layout,
    host: &Host,
    context: &PaneContext,
) -> Result<Placed, String> {
    let pane = PaneId::Critic;
    let RunSource::Live = run else {
        return Err(ARCHIVED_NOT_BUILT.to_string());
    };

    // The run, laid out for reading. `snapshot_live_run` owns the directory it is
    // given — it clears and recreates it — so nothing may be seeded into the cwd
    // before this line.
    let shell = layout.shell();
    let run_id = runs::live_run_id(&shell, fleetor_core::time::now_ms());
    let cwd = critic::run_dir(layout.root(), &run_id);
    runs::snapshot_live_run(&shell, &cwd, &run_id)?;

    let brief = critic::render_brief(&context.critic_template, &cwd)?;

    let config_dir = critic::config_dir(layout.root());
    let mut notices = harness.seed_config_dir(
        &Seed::new(&config_dir, &cwd, host.operator_home.as_deref()).with_brief(&brief),
    )?;
    notices.extend(guardrail_notices(harness, layout, pane, &config_dir, &cwd, context, false)?);

    let command = spawn::critic_command_with(
        harness,
        &cwd,
        &layout.socket(),
        &config_dir,
        &brief,
        &context.launch.worker_permission_mode,
        host.pane_program.as_deref(),
        &host.orch_path(),
    );

    // Not on the live gauge, for `orch`'s reason and the evaluator's: the gauge
    // samples the transcripts of the panes doing the work, and this pane is not
    // one of them.
    Ok(Placed { command, notices, gauge: None, harness: harness.spec(), scrubbed: &[] })
}

/// What placing a Critic on an archived run answers with, until the ticket that
/// builds it.
///
/// A sentence rather than a half-built placement, and rather than a variant that
/// silently placed a *live* Critic instead — which would be the worst of the
/// three, because the operator would get a pane that looked right and was reading
/// the wrong run.
pub const ARCHIVED_NOT_BUILT: &str =
    "the Critic can read the run in progress; reading a past run from History is not built yet";

/// What placing an evaluator fails with when this run does not get one.
///
/// One sentence for all three refusals rather than three, because they are three
/// spellings of the same fact and only one of them is ever a surprise: a default
/// build has no grader compiled in, an operator with the mode off asked for no
/// grader, and a fleet pointed at an ordinary repo has no ground truth to grade
/// against (D-060). The caller that reaches this in production —
/// [`crate::fleet::wake_evaluator`] — has already told the operator *which* of the
/// three it is, in the words that case deserves.
pub const NO_EVALUATOR: &str =
    "there is no evaluator for this run: it needs a devmode build, dev mode on in \
     the fleet's own config, and a prepared mission workspace as the target";

/// What placing a worker fails with when this machine has no key at all.
///
/// **It names the directory the `.env` walk started from (D-075).** That clause
/// was in the sentence the old bring-up sequence emitted, and it was lost when the
/// message became a constant here. It is the operator-actionable half: under
/// `tauri dev` the working directory is `src-tauri/`, so a key sitting one level
/// up at the repo root *is* found by the walk — and an operator whose key is
/// somewhere else entirely can only tell which case they are in if the message
/// says where it looked.
///
/// `searched_from` is `None` on a [`Host`] nobody discovered, and then the
/// sentence degrades to the general one rather than naming a made-up path.
pub fn missing_api_key(searched_from: Option<&Path>) -> String {
    let where_ = match searched_from {
        Some(dir) => format!("any .env from {} upward", dir.display()),
        None => "any .env from the application's working directory upward".to_string(),
    };
    format!(
        "no DEEPSEEK_API_KEY in the environment or {where_} — \
         the orchestrator runs without one, worker panes cannot"
    )
}

/// What the operator is told when a worker ends up in the shared checkout.
///
/// Kept separate from [`place_worker`] so the wording is pinned by a test — this is
/// the one notice whose absence would let a fleet look like it is reviewing itself.
///
/// **WP-06 raised what the fallback costs, so it raised the notice with it.** The
/// worktrees are not only an editing convenience: peer review is `git diff
/// fleet/worker-N` from a reviewer's *own* worktree, over the object database all
/// of them share (D-048). In the shared checkout there are no per-worker branches
/// and there is one working tree, so a reviewer asked to look at a peer's branch is
/// looking at the same files it is editing itself — one checkout reported five
/// times. The receipts still work; the review step does not, and the operator has
/// to know that before they trust a `done`.
fn shared_checkout_warning(slot: u8, why: &str, target: &Path) -> String {
    format!(
        "worker-{slot}: {why} — it will share the checkout at {}. \
         Peer review is degraded there: the workers have no branches of their own, \
         so `git diff fleet/worker-N` has nothing to compare and a reviewer sees the \
         same working tree it is editing. Receipts still report honestly; treat a \
         reviewed `done` as unreviewed until the target is a git repository.",
        target.display()
    )
}

/// A worker's own checkout, created if it is not already there.
///
/// Takes the [`Layout`] rather than reading one, which is the whole reason the
/// successful path is testable: the worktree lands under whatever root the caller
/// was handed. Everything else is the old `fleet::ensure_worktree` unchanged —
/// including the short-circuit on an existing `.git`, which is what makes a relaunch
/// cheap, and the prune, which clears registrations left by a worktree directory
/// someone deleted by hand.
fn ensure_worktree(layout: &Layout, target: &Path, slot: u8) -> Result<PathBuf, String> {
    let dir = layout.worktree(target, slot);
    if dir.join(".git").exists() {
        return Ok(dir);
    }
    let parent = dir.parent().ok_or("worktree path has no parent")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("create worktree root: {e}"))?;
    let _ = git(target, &["worktree", "prune"]);

    let slug = target_slug(target);
    let branch = format!("fleet/{slug}/worker-{slot}");
    let dir_str = dir.to_string_lossy().into_owned();
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(target)
        .args(["worktree", "add", "-B", &branch, &dir_str])
        .output()
        .map_err(|e| format!("could not run git: {e}"))?;
    if !output.status.success() {
        let why = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git worktree add failed: {}", why.trim().replace('\n', "; ")));
    }
    Ok(dir)
}

/// Run git in a repository, reporting only whether it succeeded.
///
/// `pub(crate)` with one caller outside this module ([`crate::fleet`]'s review
/// view), so there is one implementation rather than the second copy that would
/// otherwise have appeared when the worktree path moved here.
pub(crate) fn git(repo: &Path, args: &[&str]) -> bool {
    std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// What the operator is told when the `fleet` binary is nowhere. Pinned by a test
/// for the same reason [`orch_config_dir_notice`] is.
pub const MISSING_FLEET_BIN: &str =
    "the `fleet` binary was not found — panes will spawn but cannot message each other. \
     Build it with `cargo build -p fleetor-cli --bin fleet`.";

/// What a worker's operator is told when this machine has no rustup at all
/// (D-069).
///
/// Same shape and same reason as [`MISSING_FLEET_BIN`]: a worker with no toolchain
/// reachable looks perfectly healthy right up until its first `cargo build`, and
/// then fails with `command not found` — a message that points at the worker's own
/// PATH rather than at the machine.
pub const MISSING_RUSTUP: &str =
    "no rustup was found — workers will spawn but cannot build Rust. \
     Install it from https://rustup.rs, or set RUSTUP_HOME/CARGO_HOME \
     before launching if your toolchain lives somewhere unusual.";

/// What this machine is missing, for the caller to emit **before** it asks for a
/// placement (D-075).
///
/// The one part of bringing a pane up that is deliberately not inside [`place`],
/// and the reason is ordering rather than tidiness: these lines have to reach the
/// operator even when the placement that follows returns `Err`. A machine with no
/// worker key would otherwise be told only about the key, and a run that gets no
/// evaluator would never hear that the `fleet` binary is absent — the two cases
/// where the operator most needs the whole list.
///
/// What moved here in this ticket is the *decision*: which sentences, under which
/// conditions, off which [`Host`]. The caller had its own copy of both, reading
/// the machine a second time to evaluate them; now it holds neither and reads it
/// once. `orch` is excluded because [`place_orch`] emits the `fleet`-binary line
/// itself, from this same constant — it is the one pane kind whose placement
/// cannot fail before that line is reached.
///
/// **The Critic was excluded too, and WP-21 put it back** (D-076, then D-079).
/// The old reason was that the line would be *false* for it: with no
/// `FLEET_SOCKET` it could not message a pane on a machine where `fleet` was
/// built, so telling the operator to go build one would have been advice that
/// changed nothing. It has a socket now, so the line is true — a Critic on a
/// machine with no `fleet` binary is a Critic whose interview cannot be opened
/// in any useful sense, and that is exactly the failure this notice exists to
/// surface before it looks like a healthy pane.
pub fn machine_notices(host: &Host, pane: PaneId) -> Vec<(NoticeLevel, String)> {
    let mut notices = Vec::new();
    if matches!(pane, PaneId::Orch) {
        return notices;
    }
    if host.fleet_bin.is_none() {
        notices.push((NoticeLevel::Warn, MISSING_FLEET_BIN.to_string()));
    }
    if matches!(pane, PaneId::Worker(_)) && host.toolchain.is_none() {
        notices.push((NoticeLevel::Warn, MISSING_RUSTUP.to_string()));
    }
    notices
}

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
/// **Private, and that is the point of this ticket.** It had one caller outside
/// this module — the old sequence's evaluator arm — and `tests/write_guardrail.rs`
/// had a hand-typed copy of it that had already drifted: it called
/// [`guardrail::roots_for`] unconditionally, so it could never have produced the
/// narrower pair the branch below gives the evaluator. Both are gone. The rule now
/// lives here, has no second spelling, and is reached from a test the only way
/// production reaches it — through [`place`].
fn guardrail_notices(
    harness: &'static dyn Harness,
    layout: &Layout,
    pane: PaneId,
    config_dir: &Path,
    cwd: &Path,
    context: &PaneContext,
    operators_own_seat: bool,
) -> Result<Vec<(NoticeLevel, String)>, String> {
    let shell = layout.shell();
    // **A pane that reads a run gets roots narrower than any pane's,
    // deliberately.** It must read everything the run produced and change almost
    // nothing — the evaluator's stated boundary is that running a test suite is
    // fine and editing a tracked file, committing or touching a branch is not, and
    // the Critic's remit is narrower still: it runs nothing at all. So each gets
    // its working directory and nothing else: not `_shell` (which would let it
    // write the live event log it is reading), and not the operator's
    // `[fence] allow` extras, which exist for panes that are doing the work. Reads
    // are untouched for every pane alike — the hook is not registered for `Read`
    // at all (D-065), which is what makes "read everything" true without a rule.
    //
    // **Two names on one branch, not a shared predicate** (D-076). They are
    // separate identities (the arc's D5) that happen to need the same roots for
    // the same reason; a `PaneId::reads_a_run()` would read as one identity with a
    // mode flag, which is the shape that decision refused.
    let roots = if pane.is_evaluator() || pane.is_critic() {
        vec![cwd.to_path_buf()]
    } else {
        guardrail::roots_for(cwd, &shell, &context.launch.fence_allow)
    };
    // **Checkpoint 7 is a harness method now** (#31, C32): the settings document
    // is the vendor's — Claude Code's JSON, codex's TOML — so the installer is
    // theirs, exactly as checkpoint 4's seeder is. What crosses the seam is this
    // value, and the roots on it are *placement's own*: no implementation of
    // `install_guardrail` is handed anything it could widen them with, which is
    // Tier 1.7 held structurally rather than by review.
    harness.install_guardrail(&guardrail::GuardrailPlacement {
        // **The same boolean the seeder branches on** (#28, C21, C43, C49). It is
        // handed down rather than derived here, so a harness whose hook trust is
        // all-or-nothing can own a fenced pane's hook table without ever guessing
        // which seat it is on — and so the orchestrator's own hooks are never
        // touched.
        operators_own_seat,
        pane,
        config_dir,
        roots: &roots,
        policy: &guardrail::policy_dir(&shell),
        journal: &guardrail::journal_path(&shell),
    })
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
fn orch_config_dir_notice(config_dir: &Path, spec: &HarnessSpec) -> String {
    // The binary is named off checkpoint 1 rather than spelled here (#23): this
    // sentence tells the operator their *own* install is untouched, and naming the
    // wrong vendor's binary in it is a sentence that reassures about nothing.
    format!(
        "orch is running on the fleet's own config dir at {}, so its transcript is \
         archived with the run. It keeps your login. If the orch pane says \
         “Not logged in · Run /login”, run `/login` inside that pane once — it \
         persists, and it cannot disturb your own `{}`.",
        config_dir.display(),
        spec.program.bin,
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
        let spec = harness::claude_code().spec();
        let text =
            orch_config_dir_notice(Path::new("/Users/me/.fleetor/_shell/pane-config/orch"), spec);
        assert!(text.contains(spec.program.bin), "checkpoint 1 names the binary: {text}");
        assert!(text.contains("/Users/me/.fleetor/_shell/pane-config/orch"), "{text}");
        assert!(text.contains("archived with the run"), "why it moved at all: {text}");
        assert!(text.contains("Not logged in"), "the exact string the pane would show: {text}");
        assert!(text.contains("/login"), "the move that fixes it: {text}");
        assert!(
            text.contains("cannot disturb your own"),
            "and that the fix is safe to run — this is the operator's own claude: {text}",
        );
    }

    /// Moved here from `fleet` with the function it pins, when the worktree
    /// fallback moved onto the placement seam. Letter unchanged.
    ///
    /// The shared-checkout fallback is the one arrangement where a `done` can be
    /// reviewed by somebody looking at their own edits (WP-06). The notice has to
    /// say that in words, not only that a worktree failed — an operator reading
    /// "sharing the target checkout instead" has no way to know the review step
    /// stopped meaning anything.
    #[test]
    fn the_shared_checkout_warning_says_review_is_what_breaks() {
        let text = shared_checkout_warning(
            2,
            "git worktree add failed: not a repository",
            Path::new("/tmp/target"),
        );
        assert!(text.contains("worker-2"), "{text}");
        assert!(text.contains("not a repository"), "the cause survives: {text}");
        assert!(text.contains("/tmp/target"), "and where it landed: {text}");
        assert!(text.contains("Peer review is degraded"), "{text}");
        assert!(
            text.contains("git diff fleet/worker-N"),
            "names the move that stops working: {text}",
        );
        assert!(
            text.contains("treat a reviewed `done` as unreviewed"),
            "the operator needs what to do about it, not only what happened: {text}",
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
