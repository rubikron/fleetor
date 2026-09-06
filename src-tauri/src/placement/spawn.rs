//! How a pane's `claude` is launched (D-030, Phase 3).
//!
//! Two commands, deliberately different postures:
//!
//!  - [`orch_command_with`] — the operator's **own** `claude`. Full environment
//!    inherit, their login, their Opus, their `HOME`. We override only what
//!    the fleet needs, and ours must win, so every override lands *after* the
//!    inherit. Since WP-14 that list includes a fleet-owned `CLAUDE_CONFIG_DIR`
//!    — see [`orch_command_with`] for the pair of variables that keeps the login.
//!  - [`worker_command_with`] — an isolated Flash worker: its own `CLAUDE_CONFIG_DIR`,
//!    its own worktree, its own private `HOME` (WP-08), `--permission-mode
//!    auto`, and the DeepSeek endpoint.
//!
//! Everything here that looks like a detail was measured in Phase 0
//! (`docs/notes/tui-spawn-notes.md`) against a real interactive `claude`. The four that
//! each silently wedge a pane forever:
//!
//!  - **[`seed_config_dir`] is not optional (L1).** A virgin config dir does not
//!    "sometimes" hit onboarding — it never reaches a prompt at all, and every
//!    `fleet send` into it reports success into a theme picker.
//!  - **`ANTHROPIC_API_KEY` must not be set (L2).** Headless didn't care;
//!    interactive `claude` asks "use this API key?" and never reaches the input
//!    box. It is `env_remove`d rather than merely not-set, because the worker
//!    inherits the operator's environment and they may well have one.
//!  - **`--permission-mode auto`.** The default is `manual`, which wedges on the
//!    first tool call. It is `prompts/launch.conf`'s `permission_mode`, which
//!    ships as `auto` and is documented there with that consequence attached.
//!  - **[`seed_worker_home`] is not optional either (WP-08, the Fence).** Once a
//!    worker's `HOME` is a private dir instead of the operator's real one, the
//!    global `user.name`/`user.email` git used to find there are gone too — a
//!    worker's first commit fails outright, and `fleet done`'s receipt (WP-06)
//!    points at a commit that was never made.
//!
//! What a pane is *told* is not here — it is in `prompts/`, resolved once at
//! bootstrap by [`crate::prompts`] and handed in as a [`PaneContext`]. This module
//! decides how a process is shaped; that one decides what goes in its head. The
//! two `env_remove` calls below are the deliberate exception: they are not
//! settings, they are the three ways a pane wedges forever (D-042).
//!
//! **The brief is `--system-prompt`, not `--append-system-prompt` (D-043).** It
//! *replaces* Claude Code's own prompt rather than following it, so the rendered
//! file is the whole of what a pane is told. `docs/notes/system-prompt-notes.md` is the
//! measurement behind that switch, against CC 2.1.223. Note the one thing it made
//! this module responsible for: `cwd` is passed into the renderer as well as onto
//! the command, because CC's `# Environment` section carried the working directory
//! and that section is gone.
//!
//! ## Where the vendor's answers come from now (WP-25)
//!
//! **Checkpoints 1, 2, 3 and 5 are read off the harness spec here, not spelled as
//! literals.** The program and its base arguments, the flag the brief travels
//! behind, the model and permission channels, and the names scrubbed out of a
//! worker's inherited environment all come from
//! [`Harness`](crate::placement::Harness) — one lookup, in
//! [`place`](crate::placement::place), handed down to every builder below. What
//! did *not* move onto the spec is the asymmetry itself: which seat gets a
//! posture, a model and a credential at all is this module's to state, because it
//! is the product (D-030, D-052) rather than a fact about a vendor.
//!
//! Checkpoints 4, 6 and 14 followed (WP-25, issue #19): the configuration
//! directory this module points every pane at, the credential namespace the
//! attended seats are given, the private `HOME` a fenced one gets, and the seed
//! that writes both the onboarding keys and the per-project trust record — all of
//! them read off the spec now, and the project key the trust record is filed under
//! is asked of the harness rather than canonicalized here.
//!
//! Checkpoint 11's export half followed in the contract batch (WP-25, issue #23):
//! the variable the fleet's asserted context window is exported to a worker
//! through is `gauge.window_env` and the number is `gauge.window_tokens`, so no
//! literal remains here. What is still spelled out in this module is
//! [`ENV_CC_SECURESTORAGE_DIR`], which the spec *names* rather than repeats, and
//! [`project_key`] / [`seed_config_dir`] — Claude Code's own answers to
//! checkpoints 14 and 4, reached through [`Harness`] rather than called directly
//! (C31). Those are one harness's implementation, not the fleet's assumptions.

use std::path::{Path, PathBuf};

use fleetor_core::brief::{render_orch, render_worker};
use fleetor_core::pane::{PaneId, WORKER_SLOTS};
use portable_pty::CommandBuilder;

use super::harness::{Harness, HarnessSpec, Seed};
use crate::prompts::PaneContext;

/// Overrides the program every pane runs. Set by `src-tauri/tests/panes.rs` to
/// `tests/fake-pane/fake-pane.sh` so the registry is exercised end-to-end without
/// spending a token. When set, the `claude` flags are dropped — a stand-in is not
/// obliged to understand them.
const ENV_PANE_CMD: &str = "FLEETOR_PANE_CMD";
/// First rung of the `fleet` binary ladder (L4).
const ENV_FLEET_BIN: &str = "FLEETOR_FLEET_BIN";
/// Which macOS Keychain entry `claude` reads its OAuth credential from (WP-14).
///
/// Set to the empty string it selects the **unsuffixed** service name — the entry
/// the operator's own `/login` already wrote. Left unset while `CLAUDE_CONFIG_DIR`
/// is set, `claude` hashes the config dir into the service name instead and finds
/// an empty namespace. `docs/notes/orch-config-dir-notes.md` §2 reads the
/// derivation out of the CC 2.1.224 binary and measures both outcomes.
///
/// **Checkpoint 6 names this constant rather than repeating its string** (WP-25),
/// the way the spec names [`crate::guardrail::HOOK_FILE`] and the gauge's window:
/// `ConfigAndCredentialIsolation::credential_env` *is* this item, so the two
/// cannot drift by editing one side. What reads it is the spec; what reads the
/// spec is the three attended builders below.
pub(super) const ENV_CC_SECURESTORAGE_DIR: &str = "CLAUDE_SECURESTORAGE_CONFIG_DIR";

/// The full pane roster the briefs describe. Every pane is told about every other
/// one, whether or not it has been spawned yet — a brief is written once at spawn
/// and the fleet fills in around it.
fn roster() -> Vec<PaneId> {
    PaneId::roster(&WORKER_SLOTS)
}

/// **Checkpoints 4 and 6 for an attended seat, off the spec (WP-25):** point the
/// pane at its own configuration directory, and at the credential namespace the
/// operator's own login already wrote.
///
/// **One function because the pair moves together or the seat is silently logged
/// out.** Setting the configuration variable alone makes `claude` hash that
/// directory into its Keychain service name and find an empty namespace: the pane
/// reaches its input box, every `fleet send` reports `accepted`, and the first
/// turn fails (D-062). Three builders setting two variables each is three chances
/// to write one of them; this is one.
///
/// A harness whose configuration and credential are not separable answers `None`
/// for [`ConfigAndCredentialIsolation::credential_env`](super::harness::ConfigAndCredentialIsolation::credential_env)
/// and gets the configuration half alone — which is a harness fact rather than a
/// seat fact, so it is read off the spec here rather than branched on by the
/// caller.
fn apply_attended_config(cmd: &mut CommandBuilder, spec: &'static HarnessSpec, config_dir: &Path) {
    cmd.env(spec.config_dir.env_var, config_dir);
    if let Some(var) = spec.isolation.credential_env {
        cmd.env(var, "");
    }
}

// --- the orchestrator ---------------------------------------------------------

/// The operator's `claude`, as the fleet orchestrator, in `cwd`.
///
/// `CommandBuilder::new` already seeds the parent environment, so this is an
/// inherit-then-override — their login, their `HOME`, their model, plus the
/// things that make it a pane.
///
/// **`config_dir` is WP-14, and it is the one place `orch` is deliberately
/// *unlike* the operator's daily `claude`.** It must already have been through
/// [`seed_config_dir`] for this exact `cwd`: once `orch` stops using the
/// operator's already-onboarded directory, L1 applies to it exactly as it applies
/// to a worker — an unseeded dir lands on the theme picker and never reaches a
/// prompt (`docs/notes/orch-config-dir-notes.md` §1). What this buys is that
/// `orch`'s session transcript lands under `~/.fleetor`, where rotation archives
/// it with the run instead of leaving the deciding pane's reasoning unreadable
/// (D-059's named gap, closed by D-062).
///
/// **The two variables move together or `orch` is silently logged out.**
/// `claude` namespaces its Keychain service name by a hash of `CLAUDE_CONFIG_DIR`,
/// so setting that alone points it at an empty credential namespace: the pane
/// still reaches its input box, every `fleet send` still reports `accepted`, and
/// the first turn fails. `CLAUDE_SECURESTORAGE_CONFIG_DIR`, **defined and empty**,
/// selects the unsuffixed service name — the entry the operator's own `/login`
/// already wrote. Nothing is read out of the operator's config dir to achieve
/// that; `claude` performs the identical keychain read it performs today.
///
/// What `orch` still does **not** get, and must not: a private `HOME`, a worker's
/// PATH, `--permission-mode`, or any `ANTHROPIC_*` override. The asymmetry with
/// [`worker_command_with`] is the product (D-030, D-052), not an oversight.
///
/// **`program` and `path` are handed in, never read here (WP-21, D-075).** They
/// were the only two things this function reached into the process for; a reading
/// wrapper carried them until every pane kind came up through
/// [`crate::placement`], and it went with the last caller that wanted it. What is
/// left is one implementation with one entry point, and the two process reads
/// live on [`Host`](crate::placement::Host) where a test can supply them.
pub(super) fn orch_command_with(
    harness: &'static dyn Harness,
    cwd: &Path,
    socket: &Path,
    config_dir: &Path,
    ctx: &PaneContext,
    program: Option<&str>,
    path: &str,
) -> CommandBuilder {
    // Checkpoints 1, 2 and 3, off the spec (WP-25): the program, the base
    // arguments, and the brief behind whichever flag carries it. `None` is the
    // permission posture, and it is the asymmetry rather than an omission — this
    // seat is watched by a human who approves its calls (D-030, D-052).
    let mut cmd = base_command_with(
        harness,
        program,
        &harness.command_args(
            &render_orch(&ctx.orch_template, &roster(), &cwd.display().to_string()),
            None,
        ),
    );
    cmd.cwd(cwd);
    apply_pane_env(&mut cmd, PaneId::Orch, socket, path.to_string());
    apply_attended_config(&mut cmd, harness.spec(), config_dir);
    cmd
}

// --- the evaluator ------------------------------------------------------------

/// The evaluator's `claude` (WP-15), in the directory the run was laid out in.
///
/// **Shaped like `orch`, not like a worker, and for the same reason `orch` is:**
/// it is a judge, so it runs on the operator's own account and model with a full
/// environment inherit. It gets no private `HOME` — the Fence is worker-only
/// (D-052) and this pane has to reach a real `git` and a real toolchain to check
/// the fleet's claims — and no `ANTHROPIC_*` override.
///
/// Two things it does **not** share with `orch`:
///
///  - **Its brief is not from `prompts/`.** It is `brief` here, compiled in from
///    a separate repo under the `devmode` feature (`crate::evaluator`), and
///    there is no `~/.fleetor/prompts/` override for it.
///  - **`--permission-mode auto`, which `orch` does not get.** `orch` is watched
///    by an operator who approves its calls; this pane is expected to read a few
///    hundred archived files and run the target's own test suite unattended, and
///    a `manual` posture would park it on its first `Read` while looking exactly
///    like a healthy pane — the risk register's worst entry. This is **not** a
///    widening of Tier 1.7: its write guardrail is narrower than any worker's
///    (`placement::guardrail_notices` gives it its own directory and not
///    `_shell`), so what auto-approve can actually change here is a strict subset
///    of what a worker's already could. Said plainly so it is not re-litigated,
///    exactly as WP-17 said the inverse — and, since WP-21, asserted rather than
///    said: `tests/write_guardrail.rs` runs the installed hook and watches this
///    pane be refused a write a worker is allowed.
///
/// `config_dir` must already have been through [`seed_config_dir`] for this
/// exact `cwd` (L1), and the `CLAUDE_SECURESTORAGE_CONFIG_DIR` pairing is
/// `orch`'s (D-062): set and empty, so the operator's own login is found rather
/// than an empty namespace keyed by a hash of the config dir.
/// **`program` and `path` are handed in, never read here (WP-21, D-075)**, exactly
/// as for [`orch_command_with`]. The `PATH` it is given is `orch`'s — this pane is
/// the operator's own `claude` and gets their tool rungs — never a worker's fenced
/// one.
#[allow(clippy::too_many_arguments)]
pub(super) fn evaluator_command_with(
    harness: &'static dyn Harness,
    cwd: &Path,
    socket: &Path,
    config_dir: &Path,
    brief: &str,
    permission_mode: &str,
    program: Option<&str>,
    path: &str,
) -> CommandBuilder {
    let mut cmd =
        base_command_with(harness, program, &harness.command_args(brief, Some(permission_mode)));
    cmd.cwd(cwd);
    apply_pane_env(&mut cmd, PaneId::Evaluator, socket, path.to_string());
    apply_attended_config(&mut cmd, harness.spec(), config_dir);
    cmd
}

// --- the Critic ---------------------------------------------------------------

/// The Critic's `claude` (WP-20, D-076), in the directory the run was laid out in.
///
/// **Shaped like `orch`, and it is handed `FLEETOR_PANE` and `FLEET_SOCKET`
/// exactly as the evaluator is** (WP-21, D-079). It called
/// [`apply_terminal_env`] rather than [`apply_pane_env`] in WP-20, and the
/// absence of a socket was the whole identity; that absence is gone, and the
/// reason it had to go is mechanical rather than a change of mind. Both
/// variables are baked into this `CommandBuilder` at spawn, so "hand it a socket
/// when the operator opens the interview" cannot be built here without
/// respawning the pane — which would destroy the operator's conversation with
/// the pane they just decided to let speak. **The switch moved to the hub
/// instead** (`fleetor_server::hub::Hub::handle`): while the interview is
/// closed, an op from `critic` is refused at accept time, before anything is
/// resolved, asked or logged.
///
/// What did *not* move: [`PaneId::Critic::is_fleet_member`](PaneId::is_fleet_member)
/// is still `false` with the socket in hand — no roster row, no broadcast leg, in
/// no rendered brief's peer list — and this pane's write guardrail is still its
/// own working directory alone. It gained a voice, not a pen.
///
/// Otherwise it is `orch`: the operator's own account and model, their `HOME`, a
/// full environment inherit, no Fence — because it has to reach a real `git` and
/// read a few hundred archived files.
///
/// **`--permission-mode` is the worker's, for the evaluator's reason.** This pane
/// reads an archive unattended, and a `manual` posture would park it on its first
/// `Read` while looking exactly like a healthy pane. It is not a widening of Tier
/// 1.7: its write guardrail is its own working directory alone
/// (`placement::guardrail_notices`), which is a strict subset of what any worker's
/// auto-approve could already change.
///
/// `config_dir` must already have been through [`seed_config_dir`] for this exact
/// `cwd` (L1), and the `CLAUDE_SECURESTORAGE_CONFIG_DIR` pairing is `orch`'s
/// (D-062): set and empty, so the operator's own login is found rather than an
/// empty credential namespace keyed by a hash of the config dir.
#[allow(clippy::too_many_arguments)]
pub(super) fn critic_command_with(
    harness: &'static dyn Harness,
    cwd: &Path,
    socket: &Path,
    config_dir: &Path,
    brief: &str,
    permission_mode: &str,
    program: Option<&str>,
    path: &str,
) -> CommandBuilder {
    let mut cmd =
        base_command_with(harness, program, &harness.command_args(brief, Some(permission_mode)));
    cmd.cwd(cwd);
    apply_pane_env(&mut cmd, PaneId::Critic, socket, path.to_string());
    apply_attended_config(&mut cmd, harness.spec(), config_dir);
    cmd
}

// --- the workers --------------------------------------------------------------

/// One worker pane: isolated config dir, its own worktree, its own private
/// `HOME`, Flash on DeepSeek.
///
/// `config_dir` must already have been through [`seed_config_dir`] for this exact
/// `cwd` — the trust flag is keyed by absolute project path, so a worker pointed
/// at a new target with an old seed sits on a trust dialog while `fleet send`
/// reports success (L1). `home` must already have been through
/// [`seed_worker_home`] — see this module's doc comment for what an unseeded one
/// costs.
/// `toolchain` is `Some` only when this machine actually has a rustup
/// ([`operator_toolchain`], read into [`Host`](crate::placement::Host)) and both
/// fleet directories have been through
/// [`seed_fleet_toolchain`]. When it is `None` the three settings are **absent
/// rather than pointing at nothing**: a `RUSTUP_HOME` naming a directory that
/// does not exist makes rustup try to *install* there, which is a worse failure
/// than the `command not found` a worker gets today.
/// **`program` and `path` are handed in, never read here (WP-21, D-075)**, so
/// [`crate::placement`] can build a worker's command without touching the process.
///
/// The sibling of [`orch_command_with`], and everything the two do differently is
/// the Fence: a private `HOME`, a PATH with no operator rung, the fleet's own
/// toolchain homes, and four variables removed rather than merely not set.
///
/// **It returns a [`Worker`] rather than a bare command** (WP-25, C27), because
/// the last of those four is the one a caller could not see: `env_remove` deletes
/// an entry, so on the finished command a scrubbed name and a name the machine
/// never exported are indistinguishable. The names go back with the command, from
/// the same call that removed them.
#[allow(clippy::too_many_arguments)]
pub(super) fn worker_command_with(
    harness: &'static dyn Harness,
    slot: u8,
    cwd: &Path,
    home: &Path,
    config_dir: &Path,
    socket: &Path,
    api_key: &str,
    toolchain: Option<&FleetToolchain>,
    ctx: &PaneContext,
    program: Option<&str>,
    path: &str,
) -> Worker {
    let spec = harness.spec();
    let pane = PaneId::Worker(slot);
    // Checkpoints 1, 2 and 3, off the spec (WP-25): the program, the base
    // arguments, the brief behind its carrier, and — unlike the attended seats —
    // a permission posture, because nobody is watching this one.
    let mut cmd = base_command_with(
        harness,
        program,
        &harness.command_args(
            &render_worker(&ctx.worker_template, pane, &roster(), &cwd.display().to_string()),
            Some(&ctx.launch.worker_permission_mode),
        ),
    );
    cmd.cwd(cwd);
    apply_pane_env(&mut cmd, pane, socket, path.to_string());

    // The Fence learns about rustup (D-069). Both homes are the fleet's own, not
    // the operator's, and that is the whole decision: a `cargo install` a worker
    // decides to run would otherwise plant a binary in `~/.cargo/bin`, which is
    // on the *operator's* login PATH, and a `rust-toolchain.toml` naming an
    // uninstalled channel would download 1.2 GB into `~/.rustup`. Both measured
    // in `docs/notes/fence-notes.md` (arms 7b and 6b); neither is something the
    // write guardrail can refuse, because neither command names a path.
    if let Some(t) = toolchain {
        cmd.env("CARGO_HOME", &t.cargo_home);
        cmd.env("RUSTUP_HOME", &t.rustup_home);
    }

    // Checkpoint 6's private-HOME half, off the spec (WP-25). The Fence (WP-08):
    // a private HOME so `~/.ssh`, the operator's real Claude config and shell
    // profiles stop being reachable *by name*. Set after the parent-environment
    // inherit like every other override here, so ours wins. `home` must already
    // exist and carry a seeded `.gitconfig` — see `seed_worker_home` — or a
    // worker's first commit fails with no `user.name`/`user.email` and WP-06's
    // receipt points at nothing.
    //
    // Read off the spec rather than assumed because it is a harness fact, not a
    // seat fact: `private_home` is `true` for a harness that can be fenced this
    // way and `false` for one whose login is only reachable through the HOME it
    // would replace — and C17 renamed this checkpoint precisely because those two
    // had been conflated.
    if spec.isolation.private_home {
        cmd.env("HOME", home);
    }
    // Checkpoint 4, off the spec: where this pane's own configuration lives. The
    // credential half of checkpoint 6 is deliberately absent here rather than set
    // empty — a fenced pane is never handed the key to the operator's login, and
    // the scrub below is what makes that true against an inherit.
    cmd.env(spec.config_dir.env_var, config_dir);
    // Checkpoint 5's first half, off the spec (WP-25): the endpoint a worker talks
    // to and the fleet's own key, which is the whole of D-062 — a worker holds the
    // fleet's credential, never the operator's. `None` on either is a harness that
    // does not take it that way, not a worker that goes without: checkpoint 5's
    // `provider_keys` is the config-dir channel for one that is configured
    // instead, and the conformance suite refuses a harness with neither.
    if let Some(var) = spec.credentials.base_url_env {
        cmd.env(var, &ctx.launch.worker_base_url);
    }
    if let Some(var) = spec.credentials.token_env {
        cmd.env(var, api_key);
    }
    // Checkpoint 3's model channel. The attended seats get no model at all — that
    // asymmetry is the product (D-030, D-052) and is why this is here rather than
    // in `base_command_with`.
    if let Some(var) = spec.posture.model_env {
        cmd.env(var, &ctx.launch.worker_model);
    }
    // Checkpoint 11's export half, off the spec (WP-25 #23). A harness that does
    // not recognize the worker model's name would assume its own default window
    // and auto-compact early (WP-02 finding), so where the fleet is the one
    // asserting the window it exports that same number to the pane — one value,
    // read from `gauge.window_tokens`, so the vendor's bookkeeping and the rail's
    // display cannot disagree (D-054). Both `None` is a harness that publishes its
    // own window, which is the strictly better answer and gets nothing exported.
    if let (Some(var), Some(window)) = (spec.gauge.window_env, spec.gauge.window_tokens) {
        cmd.env(var, window.to_string());
    }
    // Checkpoint 5's second half, and the half that is easier to get wrong.
    //
    // Not "don't set it" — *unset* it. The worker inherits the operator's
    // environment, and an `ANTHROPIC_API_KEY` sitting in their shell profile is
    // enough to park the pane on an api-key approval prompt forever (L2). The
    // same is true of `CLAUDE_SECURESTORAGE_CONFIG_DIR` for a different reason
    // (WP-14): `orch` sets it, empty, to reach the operator's own Keychain entry,
    // and removing it here is what stops a fenced pane ever being handed the key
    // to the operator's login — it has the fleet's token and needs nothing from
    // the keychain.
    let scrubbed = scrub(&mut cmd, spec.credentials.scrubbed_env);
    Worker { command: cmd, scrubbed }
}

/// What building one worker's command produced: the command, and **the names it
/// removed from the inherited environment** (WP-25, C27).
///
/// The second field exists because `env_remove` deletes an entry rather than
/// marking it, so on the finished command a name that was scrubbed and a name the
/// machine never exported are the same observation — which is what left the
/// conformance suite able to assert checkpoint 5's scrub only as absence. Both
/// fields come out of the one call that performed the removal, so they cannot
/// disagree with each other.
pub(super) struct Worker {
    /// What the registry will spawn.
    pub(super) command: CommandBuilder,
    /// Checkpoint 5's scrub, as data: the credential names this command had
    /// removed from the environment it inherited. Attended seats scrub nothing,
    /// and that emptiness is the asymmetry rather than a missing answer.
    pub(super) scrubbed: &'static [&'static str],
}

/// Remove `names` from the inherited environment, and report what was removed.
///
/// One function so that "which names were scrubbed" is answered by the act of
/// scrubbing rather than beside it — a caller that wants the list cannot get one
/// that the command does not match.
fn scrub(cmd: &mut CommandBuilder, names: &'static [&'static str]) -> &'static [&'static str] {
    for name in names {
        cmd.env_remove(name);
    }
    names
}

// --- shared -------------------------------------------------------------------

/// The program plus its arguments, with the stand-in override handed in rather
/// than read from the process, so [`crate::placement`] can build a command without
/// touching it (WP-21).
///
/// There was a reading sibling of this, and there were reading siblings of all
/// three `*_command_with` functions, until every pane kind moved onto the placement
/// seam. They are gone (D-075): nothing in this module reads the process to build a
/// command any more, and there is one implementation per pane kind with one entry
/// point each.
fn base_command_with(
    harness: &'static dyn Harness,
    program: Option<&str>,
    args: &[String],
) -> CommandBuilder {
    if let Some(stand_in) = program.map(str::trim).filter(|s| !s.is_empty()) {
        return CommandBuilder::new(stand_in);
    }
    // Checkpoint 1 (WP-25): the program is the harness's, not this module's. The
    // base arguments are already at the head of `args` — `Harness::command_args`
    // puts them there, before anything per-seat — so there is one place that
    // decides argument order rather than one per pane kind.
    let mut cmd = CommandBuilder::new(harness.spec().program.bin);
    for arg in args {
        cmd.arg(arg);
    }
    cmd
}

/// The stand-in program every pane runs instead of `claude`, when one is set
/// (`FLEETOR_PANE_CMD`). `None` is the ordinary case.
///
/// A [`Host`](crate::placement::Host) field in disguise: this is the one read that
/// discovers it, and placement receives the answer rather than performing it. It is
/// called from [`Host::discover`](crate::placement::Host::discover) and nowhere
/// else.
pub(super) fn pane_program() -> Option<String> {
    std::env::var(ENV_PANE_CMD).ok().filter(|s| !s.trim().is_empty())
}

/// What makes any process one of *this app's* terminals: a truecolor terminal, a
/// PATH that can find `claude`, and no inherited child-session marker.
///
/// `path` is the caller's to choose (WP-08, the Fence):
/// [`Host::orch_path`](crate::placement::Host) for orch, the evaluator and the
/// Critic, `Host::worker_path` for a worker. Both resolve `fleet`'s location the
/// identical way; they differ only in whether the operator's own HOME contributes
/// rungs.
///
/// **Split from [`apply_pane_env`] so that one pane kind can have this and not
/// that** (WP-20, D-076). Everything here is about being a terminal; everything
/// there is about being addressable in the fleet's record, and the Critic is the
/// first identity that is the first without being the second.
fn apply_terminal_env(cmd: &mut CommandBuilder, path: String) {
    cmd.env("PATH", path);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    // Phase 0 saw this on every spike run: launching the shell from inside a
    // `claude` session leaks the marker through the environment inherit and
    // silently disables transcript saving in every pane below it.
    cmd.env_remove("CLAUDE_CODE_CHILD_SESSION");
}

/// The two variables that make a terminal a participant in the fleet's record:
/// its own name, and the socket to reach the hub on.
///
/// **A pane that is handed neither has no route to the fleet at all**: the
/// `fleet` CLI reads `FLEET_SOCKET` before it does anything else and refuses
/// with a sentence naming the variable, so a `fleet send` typed inside such a
/// pane exits non-zero and reaches nothing.
///
/// **Every pane kind that exists is handed both, as of WP-21 (D-079).** The
/// Critic was the one exception — that was its whole shape in WP-20 — and it
/// stopped being one for a mechanical reason: these two variables are fixed on
/// the `CommandBuilder` at spawn, so a runtime control over a pane's route
/// cannot live here without respawning the pane. It lives in the hub instead.
/// Being handed these is therefore *not* a claim of fleet membership, and it
/// never was one: [`PaneId::is_fleet_member`] answers that, and it is `false`
/// for two of the panes this function is called for.
fn apply_pane_env(cmd: &mut CommandBuilder, pane: PaneId, socket: &Path, path: String) {
    apply_terminal_env(cmd, path);
    cmd.env("FLEETOR_PANE", pane.to_string());
    cmd.env("FLEET_SOCKET", socket);
}

/// A PATH that finds `claude` and `fleet` even when the app was launched from a
/// GUI context whose environment never saw the login shell's additions.
///
/// **Orch only.** `home` is the operator's real HOME, since orch is their own
/// `claude` (D-030's "orch is untouched"). [`worker_augmented_path_from`] is the
/// worker's version and deliberately is handed no `home` at all: it must not bake
/// the operator's HOME into a worker's PATH, which is the other half of the fix
/// this module's doc comment promises alongside the private `HOME` itself.
/// **Its three inputs are handed in, never read here (WP-21, D-075)** — the
/// `fleet` binary, the operator's `HOME` and the inherited `PATH` all arrive on
/// the [`Host`](crate::placement::Host), so `orch`'s PATH can be computed against
/// a machine a test describes rather than the one the test is running on.
pub(super) fn augmented_path_from(fleet_bin: Option<&Path>, home: &str, existing: &str) -> String {
    let mut prefix = String::new();
    if let Some(dir) = fleet_bin.and_then(Path::parent) {
        prefix.push_str(&dir.to_string_lossy());
        prefix.push(':');
    }
    format!("{prefix}{home}/.local/bin:{home}/.bun/bin:/opt/homebrew/bin:/usr/local/bin:{existing}")
}

/// The worker's PATH (WP-08, the Fence): the fleet-bin rung and the system
/// dirs, none of the operator-HOME rungs [`augmented_path_from`] adds. Before this fix,
/// every worker's PATH carried `{operator's real $HOME}/.local/bin` and
/// `.../.bun/bin` regardless of the worker's own (now private) `HOME` — a name
/// pointed straight at the operator's tooling, defeating the point of fencing
/// `HOME` at all.
///
/// `existing` — the PATH inherited from the app's own process — is left alone.
/// It is not an operator-HOME rung by construction (it is whatever launched the
/// app), and stripping it is a sandboxing decision this package's spec rules
/// out; see `docs/notes/fence-notes.md` for what that leaves reachable.
///
/// `cargo_bin` is the fleet's own `_shell/cargo/bin` (D-069), or `None` when this
/// machine has no rustup — in which case the result is byte-for-byte what it was
/// before that decision. It is a rung and not merely two env vars because
/// **neither `/opt/homebrew/bin` nor `/usr/local/bin` holds a `cargo`**: the only
/// cargo on this machine is `~/.cargo/bin/cargo`, inside the operator's HOME, so
/// a worker launched with anything but the operator's login PATH inherited gets
/// `cargo: command not found` before rustup is ever reached (`fence-notes.md`,
/// arms 1 and 2). Seeded shims are a rung whose contents the fleet enumerated;
/// `~/.cargo/bin` is a rung whose contents change whenever the operator installs
/// anything, and it holds real binaries, not only shims.
/// **Its two remaining inputs are handed in, never read here (WP-21, D-075)**, so
/// a worker's PATH is computed from a [`Host`](crate::placement::Host) rather than
/// from the process.
///
/// The sibling of [`augmented_path_from`], and the difference between them is the
/// whole of the Fence's PATH half: this one is handed no `home` at all, so there
/// is no operator-HOME rung it *could* add.
pub(super) fn worker_augmented_path_from(
    fleet_bin: Option<&Path>,
    cargo_bin: Option<&Path>,
    existing: &str,
) -> String {
    let mut prefix = String::new();
    if let Some(dir) = fleet_bin.and_then(Path::parent) {
        prefix.push_str(&dir.to_string_lossy());
        prefix.push(':');
    }
    if let Some(dir) = cargo_bin {
        prefix.push_str(&dir.to_string_lossy());
        prefix.push(':');
    }
    format!("{prefix}/opt/homebrew/bin:/usr/local/bin:{existing}")
}

/// Where the `fleet` binary is, if it exists — an explicit ladder, checked for
/// existence at every rung.
///
/// The shim this replaces resolved its binary through a **hand-made symlink**
/// under `src-tauri/target/`, untracked and reproducible by nothing, behind a doc
/// comment claiming cargo put it there. It did not: these are two separate cargo
/// workspaces with two target directories (L4). So: no symlinks, no guessing, and
/// a caller that can tell the operator when the answer is "nowhere".
pub(super) fn fleet_bin_path() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os(ENV_FLEET_BIN).map(PathBuf::from) {
        return explicit.is_file().then_some(explicit);
    }
    let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
    let mut candidates: Vec<PathBuf> = Vec::new();
    // Bundled: `fleet` ships next to the shell binary.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("fleet"));
        }
    }
    // `tauri dev` runs with cwd = `src-tauri/`; a bare `cargo run` from the repo
    // root does not. Both root-workspace target dirs, checked explicitly.
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("..").join("target").join(profile).join("fleet"));
        candidates.push(cwd.join("target").join(profile).join("fleet"));
    }
    candidates.into_iter().find(|p| p.is_file())
}

// --- the config seed (L1) -----------------------------------------------------

/// **Claude Code's implementation of [`Harness::seed_config_dir`]** — checkpoint 4
/// and checkpoint 14's trust half, in one write, because this harness records both
/// in one document.
///
/// Give `dir` the keys an interactive `claude` needs to reach its prompt in `cwd`,
/// without touching anything else that is already there.
///
/// **Which file, which keys, and whether to merge come off the spec** (WP-25).
/// JSON, `true` as the affirmative answer and `"projects"` as the container do
/// not, and that is exactly the line the trait draws: they are this vendor's
/// document format rather than a harness universal — the next harness answers the
/// same two checkpoints with TOML and a `trust_level` string (C16) — so they
/// belong to a `Harness` method rather than to a shared seeder that would be
/// general only by accident.
///
/// **Merge, never clobber**, where [`ConfigDir::seed_merges`](super::harness::ConfigDir::seed_merges)
/// says so. The dir may hold a real `machineID`, cached experiment data, and —
/// after the operator switches targets — trust flags for other project paths.
/// Overwriting the file would work exactly once.
///
/// **Re-run this for every pane cwd whenever the target changes.** The trust
/// record is keyed by absolute project path, not global; Phase 0 bisected this. It
/// is the single most likely way to reintroduce L1 immediately after fixing it,
/// which is why [`place`](super::place) calls it on the placement path for every
/// pane rather than once at the target picker.
pub(super) fn seed_config_dir(harness: &dyn Harness, seed: &Seed<'_>) -> Result<(), String> {
    let (dir, cwd) = (seed.config_dir, seed.cwd);
    let spec = harness.spec();
    // This harness keeps the first-run gate in the document it seeds, so one
    // read-modify-write covers both checkpoints. A harness that split them would
    // need two writes, and would not be sharing this implementation.
    debug_assert_eq!(
        spec.config_dir.seed_file, spec.project_identity.trust_file,
        "this seeder writes one document; a harness whose trust file differs needs its own",
    );

    std::fs::create_dir_all(dir).map_err(|e| format!("create config dir {}: {e}", dir.display()))?;
    let file = dir.join(spec.config_dir.seed_file);

    let mut root = match (spec.config_dir.seed_merges, std::fs::read_to_string(&file)) {
        (true, Ok(text)) => {
            serde_json::from_str::<serde_json::Value>(&text).unwrap_or_else(|_| json_object())
        }
        _ => json_object(),
    };
    if !root.is_object() {
        root = json_object();
    }

    let object = root.as_object_mut().expect("just ensured it is an object");
    for key in spec.config_dir.seed_keys {
        object.insert((*key).into(), true.into());
    }

    // Checkpoint 14's key, **asked of the harness rather than canonicalized here**
    // (WP-25, M23). The trust flag and the context gauge now take the same answer
    // from the same harness instead of agreeing only because both happened to call
    // one shared function — which is what has to be true before a second harness
    // gives a different answer.
    let project = harness.project_key(cwd);
    let projects = object
        .entry("projects")
        .or_insert_with(json_object)
        .as_object_mut()
        .ok_or_else(|| {
            format!("existing \"projects\" in {} is not an object", spec.config_dir.seed_file)
        })?;
    let entry = projects
        .entry(project)
        .or_insert_with(json_object)
        .as_object_mut()
        .ok_or_else(|| {
            format!("existing project entry in {} is not an object", spec.project_identity.trust_file)
        })?;
    for key in spec.project_identity.trust_keys {
        entry.insert((*key).into(), true.into());
    }

    // Write via a sibling temp file: a half-written seed file is a pane that boots
    // into onboarding, which is the failure this whole function exists to prevent.
    let tmp = file.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(&root).map_err(|e| format!("encode config: {e}"))?;
    std::fs::write(&tmp, text).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &file).map_err(|e| format!("install {}: {e}", file.display()))
}

/// The key `claude` will look itself up under: its resolved working directory.
/// `canonicalize` matters on macOS, where `/tmp` and `/var` are symlinks and a
/// child's own `cwd` comes back resolved — an unresolved key would never match.
///
/// **This is Claude Code's own answer to checkpoint 14, not the fleet's** (WP-25,
/// M23). It was one shared function with two consumers — the trust flag and the
/// context gauge — and it can no longer be, because each harness declares its own
/// project identity. Both consumers now reach it through
/// [`Harness::project_key`](super::harness::Harness::project_key), which is the
/// only caller inside `src/` that matters; what is left `pub` here is the
/// implementation behind that method, kept reachable for the delivery and gauge
/// tests that pin the slug rule against it until their own batches move.
pub fn project_key(cwd: &Path) -> String {
    std::fs::canonicalize(cwd)
        .unwrap_or_else(|_| cwd.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn json_object() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

// --- the private HOME (WP-08, the Fence) ---------------------------------------

/// Give a worker's private `HOME` the one file its commits need: a minimal
/// `.gitconfig` naming it as the commit author.
///
/// This is the WP-06 interaction `00-index.md` records: once `HOME` stops
/// pointing at the operator's real one, the global `user.name`/`user.email` git
/// used to inherit are gone too, and a worker's first `git commit` (`fleet
/// done`'s first step) fails outright — no name, no receipt, nothing for a
/// reviewer to read.
///
/// Unlike [`seed_config_dir`], this does **not** merge-write on every spawn: a
/// worker's private HOME is not a shared, evolving config dir the way
/// `CLAUDE_CONFIG_DIR` is (nothing else legitimately writes into it), so
/// touching an existing file on every relaunch would only risk clobbering
/// something a future breakage-catalogue entry seeded on purpose. Written once,
/// left alone after that.
pub(super) fn seed_worker_home(dir: &Path, slot: u8) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("create worker home {}: {e}", dir.display()))?;
    let file = dir.join(".gitconfig");
    if file.exists() {
        return Ok(());
    }
    let text = format!("[user]\n\tname = fleet worker-{slot}\n\temail = worker-{slot}@fleetor.local\n");
    let tmp = dir.join(".gitconfig.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &file).map_err(|e| format!("install {}: {e}", file.display()))
}

// --- the Rust toolchain (D-069) -----------------------------------------------

/// Where the operator's Rust toolchain lives. Read, never written to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorToolchain {
    /// The real `rustup` binary. Every name in `~/.cargo/bin` — `cargo`,
    /// `rustc`, `rustfmt` and the rest — is a **symlink to this one file**;
    /// rustup dispatches on `argv[0]`. Measured in `docs/notes/fence-notes.md`.
    pub rustup_bin: PathBuf,
    /// The operator's `RUSTUP_HOME`: its `settings.toml` and its installed
    /// toolchains are what the fleet's mirror points at.
    pub rustup_home: PathBuf,
}

/// The fleet's own toolchain directories, both under `_shell/` so `rm -rf
/// ~/.fleetor` reaches everything a build made (Tier 1.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FleetToolchain {
    pub cargo_home: PathBuf,
    pub rustup_home: PathBuf,
}

/// The names a worker's PATH has to resolve, all symlinked to the same `rustup`
/// binary exactly as `~/.cargo/bin` does it.
///
/// **All or nothing, and that is measured.** Seeding `cargo` alone fails at
/// `could not execute process 'rustc -vV' (never executed)` — cargo resolves,
/// then looks for `rustc` by name and finds none (`fence-notes.md`, arm 5a).
const TOOLCHAIN_SHIMS: [&str; 8] = [
    "cargo",
    "rustc",
    "rustup",
    "rustdoc",
    "rustfmt",
    "cargo-fmt",
    "cargo-clippy",
    "clippy-driver",
];

/// The operator's toolchain, if this machine has one — an explicit ladder,
/// existence-checked at every rung, `None` rather than a guess.
///
/// Same shape and same reasoning as [`fleet_bin_path`]: a caller that can tell
/// the operator the answer is "nowhere" beats a path that looks plausible and
/// resolves to nothing. rustup's own documented overrides are honoured first,
/// because an operator who moved their toolchain said where it went.
pub(super) fn operator_toolchain() -> Option<OperatorToolchain> {
    let home = std::env::var("HOME").unwrap_or_default();
    let rustup_home = std::env::var_os("RUSTUP_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .or_else(|| Some(PathBuf::from(&home).join(".rustup")).filter(|p| p.is_dir()))?;

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(cargo_home) = std::env::var_os("CARGO_HOME").map(PathBuf::from) {
        candidates.push(cargo_home.join("bin").join("rustup"));
    }
    candidates.push(PathBuf::from(&home).join(".cargo").join("bin").join("rustup"));
    candidates.push(PathBuf::from("/opt/homebrew/bin/rustup"));
    candidates.push(PathBuf::from("/usr/local/bin/rustup"));
    let rustup_bin = candidates.into_iter().find(|p| p.is_file())?;

    Some(OperatorToolchain { rustup_bin, rustup_home })
}

/// Seed both fleet-owned toolchain directories. They are seeded together for the
/// same reason they are set together: a `CARGO_HOME` with no shims on PATH is a
/// worker that cannot find cargo, and shims with no `RUSTUP_HOME` mirror is a
/// worker whose first toolchain download lands in the operator's home.
pub(super) fn seed_fleet_toolchain(
    fleet: &FleetToolchain,
    operator: &OperatorToolchain,
) -> Result<(), String> {
    seed_fleet_cargo_home(&fleet.cargo_home, operator)?;
    seed_fleet_rustup_home(&fleet.rustup_home, operator)
}

/// The fleet's `CARGO_HOME`: one `bin/` of shims, and nothing else. Everything
/// after that — the registry, `.crates.toml`, anything `cargo install` puts
/// there — cargo writes itself.
///
/// A **shared** directory rather than one per worker, and that is measured: two
/// workers building concurrently against one cargo home both succeed, serializing
/// on cargo's own package-cache lock (`fence-notes.md`, arm 8). Per-worker
/// `target/` directories already live in each worktree, so the only thing a split
/// would buy is a duplicated registry.
fn seed_fleet_cargo_home(dir: &Path, operator: &OperatorToolchain) -> Result<(), String> {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).map_err(|e| format!("create fleet cargo bin {}: {e}", bin.display()))?;
    for shim in TOOLCHAIN_SHIMS {
        link(&operator.rustup_bin, &bin.join(shim))?;
    }
    Ok(())
}

/// The fleet's `RUSTUP_HOME`: a **mirror**, not a copy. `settings.toml` is
/// copied (98 bytes); each of the operator's installed toolchains becomes one
/// symlink into their real `~/.rustup/toolchains/`. Seeded, it is 4 KB against
/// the 1.9 GB it points at.
///
/// This exists because "the operator's `~/.rustup` is read-only in practice" is
/// **false**: a `rust-toolchain.toml` naming an uninstalled channel — an
/// ordinary thing for a repository to carry — silently downloads and installs
/// it, 35,981 files and ~1.2 GB, with no prompt (`fence-notes.md`, arm 6b).
/// Against the mirror the identical download lands under `_shell/` instead
/// (arm 9b), which is the difference between Tier 1.1 holding and not.
fn seed_fleet_rustup_home(dir: &Path, operator: &OperatorToolchain) -> Result<(), String> {
    let toolchains = dir.join("toolchains");
    std::fs::create_dir_all(&toolchains)
        .map_err(|e| format!("create fleet rustup toolchains {}: {e}", toolchains.display()))?;

    // Copied once, then left alone — same reasoning as the seeded `.gitconfig`.
    // Re-copying every spawn would clobber a `rustup default` a worker set for
    // itself, which is a legitimate thing for it to have done.
    let settings = dir.join("settings.toml");
    if !settings.exists() {
        let source = operator.rustup_home.join("settings.toml");
        if source.is_file() {
            std::fs::copy(&source, &settings)
                .map_err(|e| format!("copy {}: {e}", source.display()))?;
        }
    }

    // Re-linked on every spawn, unlike `settings.toml`: a toolchain the operator
    // installed since the last run should become visible, and a link left
    // dangling by one they removed should not stay broken. A real directory here
    // is a toolchain a *worker* downloaded into the mirror — left alone, because
    // clobbering it would throw away a 1.2 GB download.
    let entries = match std::fs::read_dir(operator.rustup_home.join("toolchains")) {
        Ok(entries) => entries,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        let mirrored = toolchains.join(entry.file_name());
        if mirrored.exists() && !mirrored.is_symlink() {
            continue;
        }
        link(&entry.path(), &mirrored)?;
    }
    Ok(())
}

/// One symlink, idempotent, and **replaced when it is dangling or points
/// somewhere else**.
///
/// The deliberate difference from [`seed_worker_home`]'s write-once rule: there
/// is no operator edit worth preserving in a symlink FLEETOR made, and a
/// dangling `cargo` is a `command not found` whose cause — the operator moved
/// their toolchain three weeks ago — is invisible from inside the pane.
fn link(target: &Path, at: &Path) -> Result<(), String> {
    if let Ok(existing) = std::fs::read_link(at) {
        if existing == target {
            return Ok(());
        }
    }
    if at.is_symlink() || at.exists() {
        std::fs::remove_file(at).map_err(|e| format!("replace {}: {e}", at.display()))?;
    }
    std::os::unix::fs::symlink(target, at)
        .map_err(|e| format!("link {} -> {}: {e}", at.display(), target.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- the process reads, spelled in the tests that want them ------------------
    //
    // These four were `pub fn` in this module until D-075. Each was one line: the
    // `*_with` implementation below, plus the process reads that
    // [`Host`](crate::placement::Host) now performs once, at
    // [`Host::discover`](crate::placement::Host::discover). Production has no
    // caller for them — `placement` hands every one of those values in — so what
    // was left was scaffolding for the tests in this file, and scaffolding belongs
    // in the test module. The tests below keep their letter; what moved is where
    // the environment is read, and it is read here rather than in the shipped
    // module.

    fn augmented_path() -> String {
        augmented_path_from(
            fleet_bin_path().as_deref(),
            &std::env::var("HOME").unwrap_or_default(),
            &std::env::var("PATH").unwrap_or_default(),
        )
    }

    fn worker_augmented_path(cargo_bin: Option<&Path>) -> String {
        worker_augmented_path_from(
            fleet_bin_path().as_deref(),
            cargo_bin,
            &std::env::var("PATH").unwrap_or_default(),
        )
    }

    fn orch_command(
        cwd: &Path,
        socket: &Path,
        config_dir: &Path,
        ctx: &PaneContext,
    ) -> CommandBuilder {
        orch_command_with(
            super::super::harness::claude_code(),
            cwd,
            socket,
            config_dir,
            ctx,
            pane_program().as_deref(),
            &augmented_path(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn worker_command(
        slot: u8,
        cwd: &Path,
        home: &Path,
        config_dir: &Path,
        socket: &Path,
        api_key: &str,
        toolchain: Option<&FleetToolchain>,
        ctx: &PaneContext,
    ) -> CommandBuilder {
        let cargo_bin = toolchain.map(|t| t.cargo_home.join("bin"));
        worker_command_with(
            super::super::harness::claude_code(),
            slot,
            cwd,
            home,
            config_dir,
            socket,
            api_key,
            toolchain,
            ctx,
            pane_program().as_deref(),
            &worker_augmented_path(cargo_bin.as_deref()),
        )
        .command
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("fleetor-spawn-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The harness the seeding tests below place against — the sole registered
    /// one, handed in because checkpoint 4's seed now asks it for the key and the
    /// file rather than spelling either out (WP-25).
    fn cc() -> &'static dyn super::super::harness::Harness {
        super::super::harness::claude_code()
    }

    fn read_config(dir: &Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(dir.join(".claude.json")).unwrap()).unwrap()
    }

    /// The exact two keys Phase 0 bisected to, on a dir that had nothing.
    #[test]
    fn a_virgin_config_dir_gets_the_two_keys_that_clear_onboarding() {
        let dir = temp_dir("virgin");
        let cwd = temp_dir("virgin-cwd");
        seed_config_dir(cc(), &Seed::new(&dir, &cwd, None)).unwrap();

        let config = read_config(&dir);
        assert_eq!(config["hasCompletedOnboarding"], serde_json::json!(true));
        let project = &config["projects"][project_key(&cwd)];
        assert_eq!(project["hasTrustDialogAccepted"], serde_json::json!(true));
        assert_eq!(project["hasCompletedProjectOnboarding"], serde_json::json!(true));

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&cwd);
    }

    /// Merge, never clobber: the four real worker config dirs on disk carry a
    /// `machineID` and a `userID`, and a pane that loses them is a pane that
    /// re-registers itself on every launch.
    #[test]
    fn seeding_keeps_everything_that_was_already_in_the_file() {
        let dir = temp_dir("merge");
        let cwd = temp_dir("merge-cwd");
        std::fs::write(
            dir.join(".claude.json"),
            r#"{"machineID":"abc","projects":{"/somewhere/else":{"hasTrustDialogAccepted":true}}}"#,
        )
        .unwrap();

        seed_config_dir(cc(), &Seed::new(&dir, &cwd, None)).unwrap();

        let config = read_config(&dir);
        assert_eq!(config["machineID"], serde_json::json!("abc"), "machineID survived");
        assert_eq!(
            config["projects"]["/somewhere/else"]["hasTrustDialogAccepted"],
            serde_json::json!(true),
            "an unrelated project's trust flag survived"
        );
        assert_eq!(config["projects"][project_key(&cwd)]["hasTrustDialogAccepted"], serde_json::json!(true));

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&cwd);
    }

    /// The L1-reintroduction test. One config dir, two targets: seeding for the
    /// second must not cost the first its trust flag, because a pane switched
    /// back would then sit on a dialog while `fleet send` reports success.
    #[test]
    fn seeding_a_second_target_leaves_the_first_targets_trust_intact() {
        let dir = temp_dir("two-targets");
        let first = temp_dir("target-a");
        let second = temp_dir("target-b");

        seed_config_dir(cc(), &Seed::new(&dir, &first, None)).unwrap();
        seed_config_dir(cc(), &Seed::new(&dir, &second, None)).unwrap();

        let config = read_config(&dir);
        for cwd in [&first, &second] {
            assert_eq!(
                config["projects"][project_key(cwd)]["hasTrustDialogAccepted"],
                serde_json::json!(true),
                "{} lost its trust flag",
                cwd.display()
            );
        }

        for dir in [&dir, &first, &second] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    /// A corrupt `.claude.json` must not stop a pane from booting. Onboarding is
    /// the failure we are preventing; refusing to spawn is a worse one.
    #[test]
    fn a_corrupt_config_is_replaced_rather_than_fatal() {
        let dir = temp_dir("corrupt");
        let cwd = temp_dir("corrupt-cwd");
        std::fs::write(dir.join(".claude.json"), "{not json").unwrap();

        seed_config_dir(cc(), &Seed::new(&dir, &cwd, None)).unwrap();
        assert_eq!(read_config(&dir)["hasCompletedOnboarding"], serde_json::json!(true));

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&cwd);
    }

    /// Both spawn paths must set the pane's own name and the socket, and must
    /// clear the child-session marker. Asserted through `CommandBuilder`'s own
    /// view so it covers the environment the child will actually get.
    #[test]
    fn every_pane_knows_its_name_and_where_the_fleet_is() {
        let socket = PathBuf::from("/tmp/fleetor-test.sock");
        let cwd = PathBuf::from("/tmp");
        let ctx = PaneContext::baked();
        let orch = orch_command(&cwd, &socket, Path::new("/tmp/orch-cfg"), &ctx);
        assert_eq!(orch.get_env("FLEETOR_PANE").unwrap(), "orch");
        assert_eq!(orch.get_env("FLEET_SOCKET").unwrap(), socket.as_os_str());
        assert!(orch.get_env("CLAUDE_CODE_CHILD_SESSION").is_none());

        let worker =
            worker_command(3, &cwd, Path::new("/tmp/home"), Path::new("/tmp/cfg"), &socket, "sk-test", None,
            &ctx);
        assert_eq!(worker.get_env("FLEETOR_PANE").unwrap(), "worker-3");
        assert_eq!(worker.get_env("FLEET_SOCKET").unwrap(), socket.as_os_str());
        assert!(worker.get_env("CLAUDE_CODE_CHILD_SESSION").is_none());
    }

    /// L2, as a test rather than a comment: the auth token is set, the api key is
    /// not. With both set, the interactive TUI never reaches its input box.
    #[test]
    fn a_worker_carries_the_auth_token_and_never_the_api_key() {
        let worker = worker_command(
            1,
            Path::new("/tmp"),
            Path::new("/tmp/home"),
            Path::new("/tmp/cfg"),
            Path::new("/tmp/s.sock"),
            "sk-secret",
            None,
            &PaneContext::baked(),
        );
        assert_eq!(worker.get_env("ANTHROPIC_AUTH_TOKEN").unwrap(), "sk-secret");
        assert!(worker.get_env("ANTHROPIC_API_KEY").is_none(), "L2: this wedges the pane on approval");
        assert_eq!(
            worker.get_env("ANTHROPIC_BASE_URL").unwrap(),
            PaneContext::baked().launch.worker_base_url.as_str(),
        );
        assert_eq!(worker.get_env("CLAUDE_CONFIG_DIR").unwrap(), "/tmp/cfg");
    }

    /// D-054: a worker is told its real window, and it is the gauge's number —
    /// spelled once. Orch never gets the override; its model is recognized and
    /// its window is not ours to state.
    #[test]
    fn a_worker_is_told_the_window_the_gauge_divides_by() {
        let ctx = PaneContext::baked();
        let worker = worker_command(
            2,
            Path::new("/tmp"),
            Path::new("/tmp/home"),
            Path::new("/tmp/cfg"),
            Path::new("/tmp/s.sock"),
            "sk-test",
            None,
            &ctx,
        );
        assert_eq!(
            worker.get_env("CLAUDE_CODE_MAX_CONTEXT_TOKENS").unwrap(),
            crate::context_gauge::WORKER_WINDOW_TOKENS.to_string().as_str(),
        );
        let orch =
            orch_command(Path::new("/tmp"), Path::new("/tmp/s.sock"), Path::new("/tmp/orch-cfg"), &ctx);
        assert!(orch.get_env("CLAUDE_CODE_MAX_CONTEXT_TOKENS").is_none());
    }

    /// `--permission-mode auto` is load-bearing: the default is `manual`, which
    /// wedges on the first tool call. And the brief goes in as a system prompt,
    /// never as a `CLAUDE.md` the worker could see in `git status` and delete —
    /// now *replacing* CC's own rather than appending to it (D-043).
    #[test]
    fn a_worker_runs_prompt_free_and_is_briefed_through_its_system_prompt() {
        let worker = worker_command(
            2,
            Path::new("/tmp"),
            Path::new("/tmp/home"),
            Path::new("/tmp/cfg"),
            Path::new("/tmp/s.sock"),
            "k",
            None,
            &PaneContext::baked(),
        );
        let args: Vec<String> =
            worker.get_argv().iter().map(|a| a.to_string_lossy().into_owned()).collect();
        assert!(args.windows(2).any(|w| w == ["--permission-mode", "auto"]), "{args:?}");
        assert!(
            !args.iter().any(|a| a == "--append-system-prompt"),
            "D-043: the brief replaces CC's system prompt, it no longer appends to it",
        );
        let brief_at = args.iter().position(|a| a == "--system-prompt").expect("briefed");
        assert!(args[brief_at + 1].contains("You are `worker-2`"), "the brief names the pane");
        assert!(args[brief_at + 1].contains("/tmp"), "and says where the pane is working");
    }

    // --- WP-08: the Fence ------------------------------------------------------

    /// The whole point of the fence: a worker's `HOME` is the private dir it was
    /// handed, never the operator's real one it would otherwise inherit — and orch
    /// is untouched, because it is the operator's own `claude` (D-030).
    ///
    /// `CommandBuilder::new` seeds the *parent* environment at construction
    /// (`get_base_env`), so orch's `HOME` is never literally absent — it is
    /// whatever this test process's own `HOME` is, exactly as inherited. The
    /// assertion is that `spawn.rs` never overrides it, not that the key is
    /// unset.
    #[test]
    fn a_worker_gets_a_private_home_and_orch_keeps_its_own() {
        let socket = PathBuf::from("/tmp/s.sock");
        let ctx = PaneContext::baked();
        let real_home = std::env::var_os("HOME");

        let orch = orch_command(Path::new("/tmp"), &socket, Path::new("/tmp/orch-cfg"), &ctx);
        assert_eq!(
            orch.get_env("HOME").map(|s| s.to_os_string()),
            real_home,
            "orch must inherit the operator's real HOME untouched"
        );

        let worker = worker_command(
            1,
            Path::new("/tmp"),
            Path::new("/tmp/private-home"),
            Path::new("/tmp/cfg"),
            &socket,
            "k",
            None,
            &ctx,
        );
        assert_eq!(worker.get_env("HOME").unwrap(), "/tmp/private-home");
    }

    /// The other half of the fix, in the same commit as the private HOME: a
    /// worker's PATH must not carry the rungs `augmented_path` derives from the
    /// operator's real HOME (`~/.local/bin`, `~/.bun/bin`) — otherwise a fenced
    /// `HOME` still leaves the operator's own tooling reachable by name through
    /// PATH instead. Orch keeps today's PATH unchanged.
    ///
    /// **This pin changed letter in D-069, and the change is argued there rather
    /// than slipped in.** It used to assert `!worker_path.contains("/Users/operator")`
    /// — a substring sweep that happened to hold. D-069 adds one rung that *is*
    /// under the operator's HOME (`~/.fleetor/_shell/cargo/bin`), because that is
    /// where `~/.fleetor` lives, so the sweep can no longer be the test. What
    /// replaces it is **stricter, not looser**: every operator tool rung is
    /// forbidden *by name* — including `.cargo/bin`, which the old test never
    /// named — and at most one rung may sit under the operator's HOME, which must
    /// be the fleet's own.
    #[test]
    fn worker_path_drops_every_operator_tool_rung_and_adds_only_the_fleets_own_cargo_bin() {
        let previous = std::env::var("HOME").ok();
        std::env::set_var("HOME", "/Users/operator");

        // Orch's half, byte-identical to before.
        let orch_path = augmented_path();
        assert!(orch_path.contains("/Users/operator/.local/bin"), "{orch_path}");
        assert!(orch_path.contains("/Users/operator/.bun/bin"), "{orch_path}");
        assert!(orch_path.contains("/opt/homebrew/bin"), "{orch_path}");

        let fleet_cargo_bin = PathBuf::from("/Users/operator/.fleetor/_shell/cargo/bin");
        let worker_path = worker_augmented_path(Some(&fleet_cargo_bin));
        let rungs: Vec<&str> = worker_path.split(':').collect();

        for forbidden in ["/Users/operator/.local/bin", "/Users/operator/.bun/bin",
                          "/Users/operator/.cargo/bin", "/Users/operator/.rustup"] {
            assert!(
                !rungs.contains(&forbidden),
                "a worker's PATH must not reach the operator's own tooling: {forbidden} in {worker_path}"
            );
        }

        let under_operator_home: Vec<&&str> =
            rungs.iter().filter(|r| r.starts_with("/Users/operator/")).collect();
        assert_eq!(
            under_operator_home,
            vec![&"/Users/operator/.fleetor/_shell/cargo/bin"],
            "exactly one rung may sit under the operator's HOME, and it is the fleet's own: {worker_path}"
        );

        assert!(rungs.contains(&"/opt/homebrew/bin"), "{worker_path}");
        assert!(rungs.contains(&"/usr/local/bin"), "{worker_path}");

        // And with no toolchain on the machine, the string is what it always was.
        assert_eq!(
            worker_augmented_path(None).split(':').filter(|r| r.starts_with("/Users/operator/")).count(),
            0,
            "no rustup means no new rung at all",
        );

        match previous {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }

    /// The WP-06 interaction: a worker's private HOME must come seeded with a
    /// gitconfig, or its first `git commit` — `fleet done`'s first step — fails
    /// with no `user.name`/`user.email` once the global one stops being reachable.
    #[test]
    fn seeding_a_worker_home_gives_it_a_gitconfig_naming_the_worker() {
        let dir = temp_dir("worker-home");
        seed_worker_home(&dir, 2).unwrap();

        let text = std::fs::read_to_string(dir.join(".gitconfig")).unwrap();
        assert!(text.contains("fleet worker-2"), "{text}");
        assert!(text.contains("worker-2@fleetor.local"), "{text}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- WP-14: orch's own config dir ------------------------------------------

    /// The whole of WP-14 at the spawn seam: `orch` gets a fleet-owned config
    /// dir, and the variable that keeps its login travels with it.
    ///
    /// The pair is the test, not either half. `CLAUDE_CONFIG_DIR` alone points
    /// `claude` at a Keychain namespace derived from that path, which has never
    /// been logged into — the pane reaches its input box, every `fleet send`
    /// reports `accepted`, and the first turn fails
    /// (`docs/notes/orch-config-dir-notes.md` §1–2 measured both).
    #[test]
    fn orch_gets_its_own_config_dir_and_keeps_the_operators_login() {
        let orch = orch_command(
            Path::new("/tmp"),
            Path::new("/tmp/s.sock"),
            Path::new("/tmp/fleetor/pane-config/orch"),
            &PaneContext::baked(),
        );
        assert_eq!(orch.get_env("CLAUDE_CONFIG_DIR").unwrap(), "/tmp/fleetor/pane-config/orch");
        assert_eq!(
            orch.get_env(ENV_CC_SECURESTORAGE_DIR).expect("without this orch is silently logged out"),
            "",
            "defined and EMPTY selects the operator's own keychain entry; any value is a namespace",
        );
    }

    /// The asymmetry is the product (D-030, D-052). `orch` moving onto its own
    /// config dir must not drag any of the worker posture along with it — a
    /// private `HOME`, a DeepSeek endpoint or an auth-token override would each
    /// take its login away by a different route.
    #[test]
    fn orch_takes_a_config_dir_and_none_of_the_worker_isolation() {
        let ctx = PaneContext::baked();
        let orch =
            orch_command(Path::new("/tmp"), Path::new("/tmp/s.sock"), Path::new("/tmp/orch-cfg"), &ctx);

        for key in [
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_MODEL",
            "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
        ] {
            assert!(orch.get_env(key).is_none(), "orch must not carry the worker's {key}");
        }
        assert_eq!(
            orch.get_env("HOME").map(|s| s.to_os_string()),
            std::env::var_os("HOME"),
            "the Fence is worker-only: orch keeps the operator's real HOME",
        );

        let args: Vec<String> =
            orch.get_argv().iter().map(|a| a.to_string_lossy().into_owned()).collect();
        assert!(
            !args.iter().any(|a| a == "--permission-mode"),
            "orch keeps the operator's own permission posture: {args:?}",
        );
    }

    /// The other direction of the same seam: a worker inherits the app's
    /// environment, and the app is about to set the variable that unlocks the
    /// operator's own login. Unset it, the way `ANTHROPIC_API_KEY` is unset.
    #[test]
    fn a_worker_never_inherits_the_key_to_the_operators_keychain_entry() {
        let worker = worker_command(
            1,
            Path::new("/tmp"),
            Path::new("/tmp/home"),
            Path::new("/tmp/cfg"),
            Path::new("/tmp/s.sock"),
            "sk-test",
            None,
            &PaneContext::baked(),
        );
        assert!(worker.get_env(ENV_CC_SECURESTORAGE_DIR).is_none());
    }

    /// L1 is not worker-only any more. `orch`'s dir is a fleet-owned directory
    /// like any other, so it needs the same two keys for the same reason — an
    /// unseeded one lands on the theme picker and never reaches a prompt
    /// (`docs/notes/orch-config-dir-notes.md` §1, arm `virgin`).
    #[test]
    fn orchs_config_dir_needs_the_same_two_keys_a_workers_does() {
        let dir = temp_dir("orch-cfg");
        let cwd = temp_dir("orch-target");

        seed_config_dir(cc(), &Seed::new(&dir, &cwd, None)).unwrap();

        let config = read_config(&dir);
        assert_eq!(config["hasCompletedOnboarding"], serde_json::json!(true));
        assert_eq!(
            config["projects"][project_key(&cwd)]["hasTrustDialogAccepted"],
            serde_json::json!(true),
        );

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&cwd);
    }

    /// Written once, left alone after that (unlike `seed_config_dir`'s
    /// merge-on-every-spawn): a second seed on an existing gitconfig must not
    /// clobber whatever is already there.
    #[test]
    fn seeding_a_worker_home_a_second_time_does_not_clobber_an_existing_gitconfig() {
        let dir = temp_dir("worker-home-existing");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".gitconfig"), "[user]\n\tname = hand-edited\n").unwrap();

        seed_worker_home(&dir, 4).unwrap();

        let text = std::fs::read_to_string(dir.join(".gitconfig")).unwrap();
        assert_eq!(text, "[user]\n\tname = hand-edited\n", "an existing gitconfig must survive re-seeding");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- D-069: the Fence learns about rustup ---------------------------------

    /// A helper standing in for a real rustup install: a `rustup` binary and a
    /// `RUSTUP_HOME` with two toolchains in it.
    fn fake_operator_toolchain(tag: &str) -> (PathBuf, OperatorToolchain) {
        let root = temp_dir(tag);
        let rustup_bin = root.join("cargo").join("bin").join("rustup");
        std::fs::create_dir_all(rustup_bin.parent().unwrap()).unwrap();
        std::fs::write(&rustup_bin, "#!/bin/sh\n").unwrap();
        let rustup_home = root.join("rustup");
        std::fs::create_dir_all(rustup_home.join("toolchains").join("stable-aarch64-apple-darwin")).unwrap();
        std::fs::create_dir_all(rustup_home.join("toolchains").join("1.95.0-aarch64-apple-darwin")).unwrap();
        std::fs::write(rustup_home.join("settings.toml"), "default_toolchain = \"stable\"\n").unwrap();
        (root.clone(), OperatorToolchain { rustup_bin, rustup_home })
    }

    fn fleet_dirs(root: &Path) -> FleetToolchain {
        FleetToolchain { cargo_home: root.join("_shell/cargo"), rustup_home: root.join("_shell/rustup") }
    }

    /// The decision itself: a worker is handed *the fleet's* two toolchain homes,
    /// and a PATH rung that can resolve `cargo` without the operator's own.
    #[test]
    fn a_worker_is_pointed_at_the_fleets_own_toolchain_homes_and_can_find_cargo() {
        let fleet = FleetToolchain {
            cargo_home: PathBuf::from("/tmp/fleetor/_shell/cargo"),
            rustup_home: PathBuf::from("/tmp/fleetor/_shell/rustup"),
        };
        let worker = worker_command(
            1,
            Path::new("/tmp"),
            Path::new("/tmp/home"),
            Path::new("/tmp/cfg"),
            Path::new("/tmp/s.sock"),
            "k",
            Some(&fleet),
            &PaneContext::baked(),
        );
        assert_eq!(worker.get_env("CARGO_HOME").unwrap(), "/tmp/fleetor/_shell/cargo");
        assert_eq!(worker.get_env("RUSTUP_HOME").unwrap(), "/tmp/fleetor/_shell/rustup");
        let path = worker.get_env("PATH").unwrap().to_string_lossy().into_owned();
        assert!(
            path.split(':').any(|r| r == "/tmp/fleetor/_shell/cargo/bin"),
            "the two env vars are useless without a rung that resolves cargo: {path}",
        );
    }

    /// The security argument, as a test. Measured in `fence-notes.md` arm 7b: with
    /// `CARGO_HOME` pointed at the operator's own, `cargo install` — which names
    /// no path, so the write guardrail passes it — plants a binary in
    /// `~/.cargo/bin`, on the operator's login PATH. Fleet-owned, the same install
    /// lands somewhere `rm -rf ~/.fleetor` reaches (Tier 1.1).
    #[test]
    fn a_workers_cargo_home_is_never_the_operators_own_so_a_cargo_install_lands_where_rm_rf_reaches() {
        let previous = std::env::var("HOME").ok();
        std::env::set_var("HOME", "/Users/operator");
        let fleet = FleetToolchain {
            cargo_home: PathBuf::from("/Users/operator/.fleetor/_shell/cargo"),
            rustup_home: PathBuf::from("/Users/operator/.fleetor/_shell/rustup"),
        };
        let worker = worker_command(
            1, Path::new("/tmp"), Path::new("/tmp/home"), Path::new("/tmp/cfg"),
            Path::new("/tmp/s.sock"), "k", Some(&fleet), &PaneContext::baked(),
        );
        for key in ["CARGO_HOME", "RUSTUP_HOME"] {
            let value = worker.get_env(key).unwrap().to_string_lossy().into_owned();
            assert!(
                value.starts_with("/Users/operator/.fleetor/"),
                "{key} must be inside ~/.fleetor or Tier 1.1 is false by construction: {value}",
            );
            assert_ne!(value, "/Users/operator/.cargo");
            assert_ne!(value, "/Users/operator/.rustup");
        }
        match previous {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }

    /// Orch is the operator's own `claude` (D-030), so it already has their
    /// toolchain — overriding either variable for it would move its builds into
    /// the fleet's cache for no reason and break the asymmetry D-052 exists for.
    #[test]
    fn orch_is_told_nothing_about_cargo_or_rustup_because_it_already_has_the_operators_environment() {
        let orch = orch_command(
            Path::new("/tmp"), Path::new("/tmp/s.sock"), Path::new("/tmp/orch-cfg"), &PaneContext::baked(),
        );
        // `CommandBuilder` seeds the parent env, so "not overridden" means equal to
        // this process's own — absent here, since the test runner has neither set.
        assert_eq!(orch.get_env("CARGO_HOME").map(|s| s.to_os_string()), std::env::var_os("CARGO_HOME"));
        assert_eq!(orch.get_env("RUSTUP_HOME").map(|s| s.to_os_string()), std::env::var_os("RUSTUP_HOME"));
    }

    /// Absent, not pointing at nothing. A `RUSTUP_HOME` naming a directory that
    /// does not exist makes rustup try to *install* there — a worse failure than
    /// the `command not found` a worker gets with no toolchain at all.
    #[test]
    fn the_toolchain_vars_are_absent_rather_than_pointing_at_nothing_when_the_operator_has_no_rustup() {
        let worker = worker_command(
            1, Path::new("/tmp"), Path::new("/tmp/home"), Path::new("/tmp/cfg"),
            Path::new("/tmp/s.sock"), "k", None, &PaneContext::baked(),
        );
        assert_eq!(worker.get_env("CARGO_HOME").map(|s| s.to_os_string()), std::env::var_os("CARGO_HOME"));
        assert_eq!(worker.get_env("RUSTUP_HOME").map(|s| s.to_os_string()), std::env::var_os("RUSTUP_HOME"));
    }

    /// rustup's own documented overrides come first: an operator who moved their
    /// toolchain said where it went, and this must not out-guess them.
    #[test]
    fn an_operator_who_moved_their_rustup_is_honoured_over_the_default_location() {
        let (root, expected) = fake_operator_toolchain("moved-rustup");
        let previous = (std::env::var("RUSTUP_HOME").ok(), std::env::var("CARGO_HOME").ok());
        std::env::set_var("RUSTUP_HOME", &expected.rustup_home);
        std::env::set_var("CARGO_HOME", root.join("cargo"));

        let found = operator_toolchain().expect("a moved toolchain is still a toolchain");
        assert_eq!(found, expected);

        match previous.0 { Some(v) => std::env::set_var("RUSTUP_HOME", v), None => std::env::remove_var("RUSTUP_HOME") }
        match previous.1 { Some(v) => std::env::set_var("CARGO_HOME", v), None => std::env::remove_var("CARGO_HOME") }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The seed's whole job: every shim a build needs, all pointing at the one
    /// `rustup` binary. All eight, because seeding `cargo` alone fails at
    /// `could not execute process 'rustc -vV'` (`fence-notes.md`, arm 5a).
    #[test]
    fn seeding_the_fleets_cargo_home_puts_a_whole_shim_set_on_the_workers_path() {
        let (root, operator) = fake_operator_toolchain("seed-cargo");
        let fleet = fleet_dirs(&root);
        seed_fleet_toolchain(&fleet, &operator).unwrap();

        for shim in TOOLCHAIN_SHIMS {
            let at = fleet.cargo_home.join("bin").join(shim);
            assert_eq!(
                std::fs::read_link(&at).unwrap(),
                operator.rustup_bin,
                "{shim} must be the operator's rustup, which dispatches on argv[0]",
            );
        }
        assert!(TOOLCHAIN_SHIMS.contains(&"cargo") && TOOLCHAIN_SHIMS.contains(&"rustc"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The mirror: settings copied, toolchains symlinked, kilobytes not gigabytes.
    /// This is what keeps a `rust-toolchain.toml` download inside `~/.fleetor`
    /// (`fence-notes.md`, arm 9b).
    #[test]
    fn the_fleets_rustup_home_mirrors_the_operators_toolchains_without_copying_them() {
        let (root, operator) = fake_operator_toolchain("seed-rustup");
        let fleet = fleet_dirs(&root);
        seed_fleet_toolchain(&fleet, &operator).unwrap();

        assert_eq!(
            std::fs::read_to_string(fleet.rustup_home.join("settings.toml")).unwrap(),
            "default_toolchain = \"stable\"\n",
        );
        for name in ["stable-aarch64-apple-darwin", "1.95.0-aarch64-apple-darwin"] {
            let at = fleet.rustup_home.join("toolchains").join(name);
            assert_eq!(
                std::fs::read_link(&at).unwrap(),
                operator.rustup_home.join("toolchains").join(name),
                "a mirrored toolchain is a link, never a copy",
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Idempotent, and one working link either way — the seed runs on every spawn.
    #[test]
    fn seeding_the_fleets_toolchain_a_second_time_leaves_one_working_link() {
        let (root, operator) = fake_operator_toolchain("seed-twice");
        let fleet = fleet_dirs(&root);
        seed_fleet_toolchain(&fleet, &operator).unwrap();
        seed_fleet_toolchain(&fleet, &operator).unwrap();

        let cargo = fleet.cargo_home.join("bin").join("cargo");
        assert_eq!(std::fs::read_link(&cargo).unwrap(), operator.rustup_bin);
        assert_eq!(std::fs::read_dir(fleet.cargo_home.join("bin")).unwrap().count(), TOOLCHAIN_SHIMS.len());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The one deliberate difference from the write-once `.gitconfig` seed: a
    /// link the operator broke by moving their rustup is *replaced*. There is no
    /// operator edit to preserve in a symlink FLEETOR made, and a dangling `cargo`
    /// is a `command not found` whose cause is invisible from inside the pane.
    #[test]
    fn a_toolchain_link_left_dangling_by_a_moved_rustup_is_replaced_rather_than_left_broken() {
        let (root, operator) = fake_operator_toolchain("dangling");
        let fleet = fleet_dirs(&root);
        let bin = fleet.cargo_home.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink(root.join("gone/rustup"), bin.join("cargo")).unwrap();
        assert!(std::fs::metadata(bin.join("cargo")).is_err(), "arranged: the link is dangling");

        seed_fleet_toolchain(&fleet, &operator).unwrap();

        assert_eq!(std::fs::read_link(bin.join("cargo")).unwrap(), operator.rustup_bin);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A toolchain a *worker* downloaded into the mirror is a real directory, not
    /// a link — 1.2 GB of it (`fence-notes.md`, arm 9b). Re-seeding must not
    /// clobber it the way it happily replaces a stale link.
    #[test]
    fn a_toolchain_a_worker_downloaded_into_the_mirror_survives_the_next_seed() {
        let (root, operator) = fake_operator_toolchain("worker-downloaded");
        let fleet = fleet_dirs(&root);
        seed_fleet_toolchain(&fleet, &operator).unwrap();
        let downloaded = fleet.rustup_home.join("toolchains").join("1.74.0-aarch64-apple-darwin");
        std::fs::create_dir_all(downloaded.join("bin")).unwrap();
        std::fs::write(downloaded.join("bin/cargo"), "real").unwrap();

        seed_fleet_toolchain(&fleet, &operator).unwrap();

        assert_eq!(std::fs::read_to_string(downloaded.join("bin/cargo")).unwrap(), "real");
        assert!(!downloaded.is_symlink());
        let _ = std::fs::remove_dir_all(&root);
    }
}
