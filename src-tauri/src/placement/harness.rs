//! What a harness *is*: fourteen checkpoints, answered once per vendor (WP-25,
//! M23, C17, C19).
//!
//! A "harness" here is the agent TUI a pane actually runs. Until this module
//! existed there was exactly one, and its answers to these fourteen questions were
//! spelled out as literals in eight files — `placement/spawn.rs` carries the bulk
//! of them, with more in [`crate::guardrail`], [`crate::deliver`], [`crate::pty`],
//! [`crate::context_gauge`], `crate::orphans` and [`crate::runs`]. Nothing named
//! the set, so "what would a second vendor have to answer" could only be
//! discovered by reading all eight.
//!
//! ## What this module is
//!
//! **The single source of truth, and as of the contract step it is the only one.**
//! Phase 1 ran expand–migrate–contract: this spec was written beside the literals
//! (#14), the conformance suite became the safety net (#15, #16), five batches
//! pointed the call sites here (#18–#22), and #23 deleted what was left over. No
//! production file spells a vendor answer any more, and
//! `src-tauri/tests/harness_literals.rs` is the tripwire that says so by reading
//! the source — because a call site that re-hardcodes `CLAUDE_CONFIG_DIR` builds
//! byte for byte the same command as one that reads [`ConfigDir::env_var`], so no
//! test that *runs* the code can tell them apart.
//!
//! **Every one of the fourteen is defined here, and as of phase 2 every one of
//! them has a reader.** Three had none through the whole of phase 1 —
//! [`Posture::sandbox_keys`], [`Credentials::provider_keys`] and
//! [`Outbound::reachability_keys`] — left that way three times deliberately (#18,
//! #19, #21) because all three are the same absent reader rather than three
//! separate ones: the config-dir seeding a harness that is *configured* rather
//! than flagged would need. C36 said that reader would be
//! [`Harness::seed_config_dir`], and #26 wrote it, in
//! [`codex`](crate::placement::codex) — one pass over all three, each `(key,
//! value)` a dotted path into the seeded document. Claude Code's three lists are
//! empty because it is flagged rather than configured, so its own seeder does not
//! read them, and that is the honest answer rather than a general mechanism for a
//! harness that has no use for one.
//!
//! ## The one rule this module inherits
//!
//! [`crate::placement`]'s rule holds here without exception: **nothing in here
//! reads the process.** A [`HarnessSpec`] is `'static` data — no path resolved at
//! call time, no environment variable read, no vendor binary shelled out to. A
//! machine fact belongs on [`Host`](crate::placement::Host), which already takes
//! its facts as values. What is *discovered* about an installed harness (login
//! shape, model list, the posture the vendor actually resolved) is `Host`'s to
//! carry; what a harness *is* is this.
//!
//! ## Why a trait and a flat struct, rather than one or the other
//!
//! The [`HarnessSpec`] half is data, and it is flat and complete: it has one field
//! per checkpoint and no [`Default`], so a second harness cannot be registered
//! without answering all fourteen — the compiler is the checklist. The [`Harness`]
//! half is the three checkpoints that are genuinely functions rather than values:
//! writing a config seed, keying a project, and turning a brief into argv. Those
//! cannot be `'static` data because they take the pane's own cwd and brief.
//!
//! A separate crate was rejected (M23): 79 of the vendor-specific references are
//! already inside `placement` and `guardrail`, so a crate would invert the
//! dependency direction and drag the guardrail, delivery, the orphan sweep and the
//! context gauge into it.
//!
//! ## Where a spec comes from
//!
//! [`PaneSpec::harness`](crate::placement::PaneSpec::harness) answers "which
//! harness is this pane", [`place`](crate::placement::place) binds it once, and
//! every arm hands its answer back on [`Placed::harness`](crate::placement::Placed::harness).
//! That is one lookup in one place. A later ticket that needs `spec.program.bin`
//! at a call site inside `placement` takes the value that is already in scope; it
//! does not re-derive it.

use std::path::{Path, PathBuf};

use fleetor_core::event::NoticeLevel;

use crate::guardrail;

// --- the fourteen checkpoints -------------------------------------------------

/// One harness's answers to the fourteen checkpoints — the data half of the seam
/// (M23).
///
/// **Flat, `'static`, and without a [`Default`].** Flat because a migrating call
/// site should read one field rather than navigate a tree; `'static` because
/// nothing here may read the process; without a `Default` because a harness that
/// could be half-registered is the failure this type exists to prevent — "add a
/// harness" has to mean "answer fourteen questions", and a defaulted field is a
/// question silently answered wrong.
///
/// The fields are in checkpoint order and the order is not decorative: it is the
/// order a pane is brought up in, so reading the struct top to bottom is reading
/// the bring-up sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessSpec {
    /// This harness's name, for the operator's rail and for `manifest.json`
    /// (M24) — **not** a fifteenth checkpoint. It is identity rather than
    /// mechanism: the fourteen below say how a pane of this harness is brought
    /// up, and this says what to call it when a run is written down.
    ///
    /// The panes are never told it (M25): the roster is harness-free, so a worker
    /// cannot start reasoning about which peer is "weaker".
    pub name: &'static str,

    /// This harness's **mark** — a monogram of two characters or fewer, for the
    /// operator's rail (#50, C56). Identity like [`name`](Self::name), and **not**
    /// a fifteenth checkpoint: nothing in a bring-up reads it.
    ///
    /// **It is here so that no interface has to switch on a harness's name.** A
    /// rail that told a codex pane apart from a Claude Code one with
    /// `harness === "codex" ? … : …` would be a branch written for the first two
    /// vendors, and the third would be invisible on the day it registered — which
    /// is the archaeology this seam exists to end (C57). The mark travels beside
    /// the name on [`FleetEvent::PaneState`](fleetor_core::FleetEvent::PaneState),
    /// so a harness supplies its own the way it supplies its name.
    ///
    /// **A monogram rather than the vendor's logo**, deliberately: an interface
    /// carrying trademarked artwork would have to hold one file per vendor, which
    /// is the same switch with images in it.
    ///
    /// It is deliberately **not** written into `manifest.json`. The archive stores
    /// the name and looks the spec up with [`by_name`] (M24, C56); the interface
    /// has no registry to look anything up in, and that asymmetry is why this
    /// rides the event and not the record.
    pub mark: &'static str,

    /// **How an operator logs this harness in, in their own terminal** (#51).
    ///
    /// Identity like [`name`](Self::name) and [`mark`](Self::mark), and **not** a
    /// fifteenth checkpoint: nothing in a bring-up reads it. A pane is placed the
    /// same way whether the operator has logged in or not — what changes is what
    /// the gate can tell them when they have not.
    ///
    /// **It is here for the reason the mark is here.** A gate that answered "what
    /// do I run to fix this" with `harness === "codex" ? … : …` would answer it
    /// for the first two vendors and answer it wrong for the third, which is the
    /// archaeology this seam exists to end (C57). It travels the channel the name
    /// already travels — onto `HarnessOffer` and out to the card.
    pub login: LoginInstruction,

    /// **Checkpoint 1 — program and base arguments.**
    pub program: Program,
    /// **Checkpoint 2 — brief carrier.**
    pub brief: BriefCarrier,
    /// **Checkpoint 3 — model flag and permission or sandbox posture.**
    pub posture: Posture,
    /// **Checkpoint 4 — configuration directory and its seeding.**
    pub config_dir: ConfigDir,
    /// **Checkpoint 5 — credential wiring and environment scrub.**
    pub credentials: Credentials,
    /// **Checkpoint 6 — config and credential isolation mechanism** (C17,
    /// generalized from "private HOME seeding").
    pub isolation: ConfigAndCredentialIsolation,
    /// **Checkpoint 7 — write-guardrail install.**
    pub guardrail: GuardrailInstall,
    /// **Checkpoint 8 — outbound reachability.**
    pub outbound: Outbound,
    /// **Not a checkpoint** — what has to happen between a running process and a
    /// pane that can receive, read only by `pty::PaneRegistry::spawn` (#42, C26).
    /// It sits here because this is where it happens: a pane has to be able to
    /// receive before checkpoint 9 has anything to type into.
    pub bring_up: BringUp,
    /// **Checkpoint 9 — typing profile.**
    pub typing: TypingProfile,
    /// **Checkpoint 10 — command-channel spellings.**
    pub commands: CommandChannel,
    /// **Checkpoint 11 — context-gauge source and window constant.**
    pub gauge: GaugeSource,
    /// **Checkpoint 12 — orphan-sweep process names.**
    pub orphans: OrphanNames,
    /// **Checkpoint 13 — transcript location and format.**
    pub transcript: Transcript,
    /// **Checkpoint 14 — project identity and trust seeding** (C17, generalized
    /// from "project-key canonicalization").
    pub project_identity: ProjectIdentityAndTrust,
}

/// **What the operator types to log one harness in** (#51).
///
/// **`'static` data, like everything else here**: nothing in this struct is
/// discovered by running the vendor. It is the answer to "what would somebody type
/// in a terminal", which a harness knows about itself before any machine is
/// probed — the same class of fact as [`Program::bin`], and the reason a login
/// instruction that *did* need a subprocess would belong on
/// [`Host`](super::Host) instead.
///
/// **FLEETOR never runs it and never collects what it produces.** #49 measured
/// where each vendor's credential actually lives —
/// [`credential_home`](Self::credential_home) is that measurement, in the
/// operator's words — and only the vendor's own login writes there. A field on
/// this card that took a pasted token would write it somewhere the vendor does not
/// read, which is a control that looks like it worked and changes nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginInstruction {
    /// The command, exactly as it is typed. Claude Code's is the binary itself,
    /// because its login is a command *inside* its own session; codex's is a
    /// subcommand and completes on its own.
    pub command: &'static str,
    /// The second step, for a harness whose login lives inside what
    /// [`command`](Self::command) starts. `None` when the command above is the
    /// whole of it.
    ///
    /// Two fields rather than one sentence because the interface renders each as
    /// something to type, and a sentence with a command inside it is a command
    /// nobody can copy.
    pub then: Option<&'static str>,
    /// Where the vendor writes the credential that login produces, named for the
    /// operator rather than as a path — the measured half of why FLEETOR does not
    /// collect one (#49, C9).
    pub credential_home: &'static str,
}

/// **Checkpoint 1 — program and base arguments.**
///
/// The binary a pane runs and whatever every pane of this harness gets before any
/// per-seat argument. Today's literal is `spawn::base_command_with`'s
/// `CommandBuilder::new("claude")`.
///
/// The stand-in override (`FLEETOR_PANE_CMD`) is deliberately not here: it
/// replaces the program *and drops its flags*, which makes it a property of the
/// machine rather than of the harness, and it already lives on
/// [`Host::pane_program`](crate::placement::Host::pane_program).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    /// The executable name, resolved on the pane's `PATH`.
    pub bin: &'static str,
    /// Arguments every pane of this harness gets, before the per-seat ones
    /// [`Harness::command_args`] adds. Empty for a harness that needs none.
    pub base_args: &'static [&'static str],
}

/// **Checkpoint 2 — brief carrier.**
///
/// How `orch.md` / `worker.md` reach the pane. D-042 holds across every harness —
/// one orchestrator brief, one worker brief, and only the *carrier* varies — so
/// this checkpoint is about transport and nothing else.
///
/// Exactly one of [`argv_flag`](Self::argv_flag) and
/// [`config_key`](Self::config_key) is set. Two nullable fields rather than an
/// enum because this struct is read at migrating call sites and never matched on:
/// a caller that builds argv wants the flag or nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BriefCarrier {
    /// The argv flag the brief travels behind, when it travels in argv at all.
    /// Claude Code: `--system-prompt`.
    pub argv_flag: Option<&'static str>,
    /// The configuration key naming a file the brief is written to, for a harness
    /// that carries it through the config dir instead of argv. `None` here.
    pub config_key: Option<&'static str>,
    /// Whether the brief **replaces** the vendor's own system prompt rather than
    /// being appended to it (D-043). A carrier that appends puts the fleet's
    /// instructions underneath the vendor's, which is a different product.
    pub replaces_system_prompt: bool,
    /// Whether the carrier writes a file **inside the pane's checkout**. `false`
    /// is the good answer: a brief file in the worktree shows up in the operator's
    /// `git status` and has to be excluded.
    pub writes_into_worktree: bool,
}

/// **Checkpoint 3 — model flag and permission or sandbox posture.**
///
/// Two things that travel together because they are set together on the same
/// command: which model this pane runs, and how far it may act without being
/// asked.
///
/// `orch`, the evaluator and the Critic run the operator's own account and model,
/// so the model channel is unused for them and only the permission one is set —
/// that asymmetry is the product (D-030, D-052) and belongs at the call site, not
/// here. This says what channels *exist*.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Posture {
    /// The environment variable that names the model, when the harness takes it
    /// that way. Claude Code: `ANTHROPIC_MODEL`.
    pub model_env: Option<&'static str>,
    /// **Models this harness offers when its vendor publishes no catalog** (C78)
    /// — `(slug, display name)`, in the order an operator should see them.
    ///
    /// **Data on the spec rather than a literal in the interface**, and
    /// `tests/gate_pickers.rs` is what forces that: every harness's name, label,
    /// model list and reason reaches the gate over the wire from
    /// [`HarnessReadiness`], because a vendor name written into a component is a
    /// third harness offered and then handled by a branch written for the first
    /// two. This is the same argument C72 made for [`LoginInstruction`], and the
    /// same shape: `'static` strings, because what a vendor calls its own models
    /// is a fact it knows before any machine is probed.
    ///
    /// **Empty for a harness that publishes a real catalog**, which is codex —
    /// its `models.json` is read live and sorted by the vendor's own priority, and
    /// a hand-written list beside that would be a second answer going stale.
    /// Claude Code publishes none (measured: `models: Vec::new()` in its
    /// diagnostic), so its aliases live here.
    ///
    /// **A floor, never a ceiling.** The gate's model field stays free text with
    /// these as suggestions, so a full id the vendor accepts is always reachable
    /// and this list going stale costs an operator one typed word.
    pub declared_models: &'static [(&'static str, &'static str)],
    /// The argv flag that names the model, for a harness that takes it that way
    /// instead. `None` here.
    pub model_flag: Option<&'static str>,
    /// The argv flag carrying the permission posture. Claude Code:
    /// `--permission-mode`, whose value is
    /// [`LaunchConfig::worker_permission_mode`](crate::prompts::LaunchConfig).
    pub permission_flag: Option<&'static str>,
    /// Configuration keys that set a sandbox posture, for a harness whose
    /// containment is configured rather than flagged. Empty here: Claude Code has
    /// no sandbox of its own, and what bounds a pane is the write guardrail
    /// (checkpoint 7) plus the Fence.
    ///
    /// **Tier 1.7 reads this field.** Anything a harness puts here narrows what a
    /// pane may do; nothing here may widen auto-approve past a worker's worktree.
    ///
    /// **Written by [`Harness::seed_config_dir`]**, which reads this list,
    /// [`Credentials::provider_keys`] and [`Outbound::reachability_keys`] together
    /// and nothing else — one pass over dotted paths into the harness's own
    /// configuration document (C36).
    pub sandbox_keys: &'static [(&'static str, &'static str)],
    /// **What the vendor must be seen to have resolved from those keys** (WP-25
    /// #37; C8).
    ///
    /// Writing `sandbox_keys` proves nothing about the fence. A release that
    /// retires one of them keeps parsing the configuration and silently ignores
    /// the row — measured on `codex-cli 0.153.4`, where an unknown `-c` key leaves
    /// `doctor` emitting an ordinary report — so the pane comes up unfenced with
    /// nothing anywhere saying so. This is the read-back: one entry per key, naming
    /// the row of [`ResolvedPosture`] that key decides and the word the vendor
    /// reports having resolved from the value `sandbox_keys` and
    /// [`Outbound::reachability_keys`] write.
    ///
    /// **The keys are bound here, not respelled.** `tests/gate_posture_tripwire.rs`
    /// fails if this list and those two ever name different keys, in either
    /// direction — a fourth containment key added without a row here would be a key
    /// nothing reads back, which is the state this field exists to end.
    ///
    /// Empty for a harness with no such diagnostic, which is a real answer rather
    /// than a gap: Claude Code publishes nothing that reports a resolved posture,
    /// so there is nothing to compare and the gate says nothing about it.
    pub verified_as: &'static [PostureExpectation],
    /// **The diagnostic report schema [`Self::verified_as`] was measured against**
    /// (WP-25 #37, acceptance criterion 3).
    ///
    /// Every word in that list is a reading off one report shape. A vendor that
    /// bumps this has changed the document those readings were taken from, and a
    /// posture compared across that change is a comparison of two different things
    /// that happen to have the same field names. So the arc refuses the fleet and
    /// says which version it understands, rather than reporting a green fence it
    /// can no longer justify.
    ///
    /// `None` for a harness with no schema to pin, which is every harness with an
    /// empty [`Self::verified_as`].
    pub verified_against_schema: Option<&'static str>,
}

/// **One row of the posture tripwire: a key FLEETOR writes, and the word the vendor
/// must be seen to have resolved from it** (WP-25 #37; C8).
///
/// **Every `resolved` here is a measurement, never a guess.** They were read off
/// `codex-cli 0.153.4` by running its own diagnostic with the trio applied and with
/// each alternative applied, so the words are discriminating rather than merely
/// present: `sandbox_mode` moves `filesystem` between `restricted` and
/// `unrestricted`, `network_access` moves `network` between `restricted` and
/// `enabled`, and `approval_policy` moves `approval` between `Never` and
/// `OnRequest`. A row whose word never changes would be a tripwire that cannot
/// fire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostureExpectation {
    /// Which row of [`ResolvedPosture`] this is about, spelled the way
    /// [`ResolvedPosture::rows`] spells it: `filesystem`, `network`, `approval`.
    pub row: &'static str,
    /// The configuration key that decides it, spelled exactly as
    /// [`Posture::sandbox_keys`] or [`Outbound::reachability_keys`] spells it.
    /// **This is what the refusal names**, because "the posture disagreed" is not
    /// something an operator can act on and "`approval_policy` disagreed" is.
    pub written_key: &'static str,
    /// The word the vendor reports having resolved, when that key carries the value
    /// those lists write.
    pub resolved: &'static str,
}

/// **Checkpoint 4 — configuration directory and its seeding.**
///
/// Where a pane's own configuration lives, and the file that has to exist inside
/// it before the process does. The seeding itself is behaviour and lives on
/// [`Harness::seed_config_dir`]; this is the shape it writes.
///
/// **The L1 requirement is this checkpoint's whole reason for existing.** An
/// unseeded config dir is a pane that boots into onboarding while every
/// `fleet send` reports `accepted`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigDir {
    /// The environment variable pointing the harness at its configuration
    /// directory. Claude Code: `CLAUDE_CONFIG_DIR`.
    pub env_var: &'static str,
    /// The file inside it that the seed writes. Claude Code: `.claude.json`.
    pub seed_file: &'static str,
    /// The top-level keys the seed sets so a pane reaches its prompt. The
    /// per-project ones are checkpoint 14's, not these.
    pub seed_keys: &'static [&'static str],
    /// Whether the seed must **merge** into an existing file rather than replace
    /// it. `true` here: the file accumulates a real `machineID`, cached
    /// experiment data and trust flags for other targets, so overwriting it works
    /// exactly once.
    pub seed_merges: bool,
}

/// **Checkpoint 5 — credential wiring and environment scrub.**
///
/// What a worker pane is given to authenticate with, and — the half that is
/// easier to get wrong — what has to be *removed* from the inherited environment
/// so the operator's own credential cannot leak into a fenced pane.
///
/// D-062 across every harness: **a worker holds the fleet's credential, never the
/// operator's.** The scrub is how that stays true against an environment inherit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    /// The variable naming the endpoint a worker talks to. Claude Code:
    /// `ANTHROPIC_BASE_URL`, set from
    /// [`LaunchConfig::worker_base_url`](crate::prompts::LaunchConfig).
    pub base_url_env: Option<&'static str>,
    /// The variable carrying the fleet's own key. Claude Code:
    /// `ANTHROPIC_AUTH_TOKEN`, from the `.env` walk.
    pub token_env: Option<&'static str>,
    /// Configuration keys a harness needs written into its config dir to reach
    /// the fleet's endpoint, for one that is configured rather than
    /// environment-driven. Empty here.
    ///
    /// Written by [`Harness::seed_config_dir`], alongside
    /// [`Posture::sandbox_keys`] and [`Outbound::reachability_keys`] (C36).
    pub provider_keys: &'static [(&'static str, &'static str)],
    /// **Removed from the inherited environment, not merely left unset.** A
    /// worker inherits the app's environment, and an operator's key sitting in a
    /// shell profile is enough to park a pane on an approval prompt forever (L2).
    pub scrubbed_env: &'static [&'static str],
}

/// **Checkpoint 6 — config and credential isolation mechanism** (C17).
///
/// **Renamed, because the old name asserted something measurably false.** It was
/// "private HOME seeding", which described one vendor's mechanism as though it
/// were the harness universal it is not: for Claude Code the configuration is
/// relocated by [`ConfigDir::env_var`] and the credential is selected out of the
/// system keychain by [`credential_env`](Self::credential_env), neither of which
/// follows `HOME`. A checkpoint named after one vendor's mechanism is a stale name
/// waiting to happen.
///
/// The private `HOME` is still real and still seeded — it is the Fence (WP-08),
/// and its job is `~/.ssh`, shell profiles and the operator's tooling. It is not
/// this harness's credential mechanism, and
/// [`token_follows_home`](Self::token_follows_home) is the field that says so.
///
/// **Two fields, because there are two channels** (C73). The token channel and
/// the operator's own credential store answer this question differently on the
/// same harness, and the single boolean that used to answer for both recorded the
/// second one wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigAndCredentialIsolation {
    /// The variable that relocates *configuration*. The same variable as
    /// [`ConfigDir::env_var`] for a harness that isolates both together; named
    /// again here because a harness may split them, and a reader of this
    /// checkpoint should not have to know that it did not.
    pub config_env: &'static str,
    /// The variable that selects the *credential* namespace, when it is separate
    /// from the configuration one. Claude Code:
    /// `CLAUDE_SECURESTORAGE_CONFIG_DIR` — **defined and empty** selects the
    /// unsuffixed keychain service name, which is the entry the operator's own
    /// `/login` wrote (D-062). Left undefined, a fleet-owned config dir would
    /// give `orch` an empty credential namespace keyed by a hash of that
    /// directory, and the pane would sit at a login prompt looking healthy.
    pub credential_env: Option<&'static str>,
    /// Whether the **fleet's token channel** — [`Credentials::token_env`] — is
    /// reachable only through `HOME`. `false` on every harness registered here,
    /// and it is the fact C17 renamed this checkpoint over: an environment
    /// variable cares nothing for `HOME`, so a fenced pane carrying the fleet's
    /// key authenticates with a private one.
    ///
    /// **This was one boolean answering for two channels until C73.** The field
    /// it replaces was documented as covering the token *and* the keychain entry;
    /// the first clause was true and the second was measured false, which is C17's
    /// own error in mirror image. See
    /// [`operator_store_follows_home`](Self::operator_store_follows_home).
    pub token_follows_home: bool,
    /// Whether the **operator's own credential store** is reachable only through
    /// `HOME` — the second channel, and the one that behaves differently.
    ///
    /// `true` for Claude Code, and it is macOS's fact rather than the vendor's: a
    /// private `HOME` removes the operator's login keychain from the process's
    /// keychain search list, so the entry
    /// [`credential_env`](Self::credential_env) selects is not merely unselected
    /// but *unsearchable* (`plan-worker-notes.md` §2). `false` for codex, whose
    /// `auth.json` is found through [`config_env`](Self::config_env) instead.
    ///
    /// **What this field decides is not whether a plan seat can work** — C73
    /// measured that it can, by planting the credential in the pane's own
    /// configuration directory. It decides whether a fenced pane can reach the
    /// operator's store *by itself*, which is what the conformance suite asserts
    /// the failure of.
    pub operator_store_follows_home: bool,
    /// Whether a fenced pane of this harness is given a private `HOME` at all.
    /// `true`: the Fence still wants one for `~/.ssh`, shell profiles and the
    /// operator's tool rungs, and a worker's first `git commit` needs the
    /// `.gitconfig` seeded into it.
    pub private_home: bool,
    /// Whether the isolated directory is seeded as a **snapshot of the
    /// operator's own**, rather than created empty. `false` here: a Claude Code
    /// pane gets a fresh directory and reaches the operator's login through the
    /// keychain instead.
    pub seeds_from_operator: bool,
}

/// **Checkpoint 7 — write-guardrail install.**
///
/// Every pane, every harness, gets a pre-edit refusal that no pane can rewrite —
/// the roots are its own cwd plus `_shell`, and Tier 1.7 says that is never
/// widened. This checkpoint is the *event* the harness offers to hang it on and
/// the file the policy is written into.
///
/// **The installer is a [`Harness`] method** ([`Harness::install_guardrail`]),
/// because the settings *document* is the vendor's: Claude Code's is JSON with a
/// top-level `hooks` object and codex's is the same `config.toml` as everything
/// else. C32 predicted exactly this and named the measurement that would force it.
/// What stays free functions in [`crate::guardrail`] is what is the same
/// everywhere — the script, the interpreter, the command line, the temp-file
/// install and the journal.
///
/// **Tier 1.7 is not on this type and never will be.** The roots are
/// `placement::guardrail_notices`' own computation from the pane's cwd, handed
/// down on [`crate::guardrail::GuardrailPlacement`]; a harness supplies *where*
/// its refusal is registered, never *what* it refuses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardrailInstall {
    /// The settings file inside the config dir the hook is registered in.
    /// Claude Code: `settings.json`. Codex: `config.toml`, the same document the
    /// rest of its seed goes into.
    pub settings_file: &'static str,
    /// The hook event fired before a tool call, which is the only place a refusal
    /// can be a refusal rather than a report. Claude Code: `PreToolUse`.
    pub hook_event: &'static str,
    /// **The vendor's names for the tools that write**, and the one spelling of
    /// them anywhere in the fleet (#23's carry-forward, closed here).
    ///
    /// It is a list rather than a matcher string because the two are not the same
    /// fact and pretending they were is how they drift: the matcher is whatever
    /// syntax the settings document wants, rendered by the harness's own
    /// installer, while *these* are the names that arrive in a hook payload and
    /// that `write_guardrail.py` is handed on its command line. A harness whose
    /// document wants an alternation joins them; one that registers for
    /// everything ignores them here and the script still filters on them.
    ///
    /// **Accuracy is load-bearing rather than cosmetic.** The script allows any
    /// tool not on this list rather than guessing at it — a deny-by-default layer
    /// nobody asked for is worse — so a name missing here is a write the guardrail
    /// waves through while looking perfectly installed.
    pub write_tools: &'static [&'static str],
    /// The script file dropped into the config dir, shared by every harness
    /// because the policy is the fleet's rather than the vendor's.
    pub hook_file: &'static str,
}

/// **Checkpoint 8 — outbound reachability.**
///
/// Whether a pane of this harness can reach the fleet's unix socket at
/// `~/.fleetor/_shell/fleet.sock`. This is not a networking detail: a pane that
/// cannot reach it has no route to the fleet at all, and `fleet send` typed inside
/// it exits non-zero having reached nothing.
///
/// **There is no bridge, and that is settled** (M28): one transport, one CLI. A
/// harness whose sandbox refuses the socket has exactly one lever, and if that
/// lever is all-or-nothing then a network-restricted pane and a talking pane are
/// mutually exclusive for that harness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outbound {
    /// Whether this harness confines its pane in a sandbox of its own at all.
    /// `false`: Claude Code is bounded by the write guardrail and the Fence, not
    /// by a seatbelt profile.
    pub sandboxed: bool,
    /// Whether a pane can `connect()` the fleet socket as configured. Must be
    /// `true` for any harness that is registered — a `false` here is a fleet of
    /// mute panes.
    pub socket_reachable: bool,
    /// The configuration keys that have to be set to make it reachable, for a
    /// harness whose default posture refuses. Empty here.
    ///
    /// Written by [`Harness::seed_config_dir`], alongside
    /// [`Posture::sandbox_keys`] and [`Credentials::provider_keys`] (C36).
    pub reachability_keys: &'static [(&'static str, &'static str)],
}

/// **How a pane of this harness gets from a running process to one that can
/// receive a message** — and **not** a fifteenth checkpoint (WP-25 #42, C26).
///
/// A pty is not a prompt. Checkpoint 1 says what to run; this says what has to
/// happen after it is running before checkpoint 9 has anything to type into.
/// For Claude Code the answer is *nothing*, which is why this seam did not exist
/// until a harness needed something.
///
/// **Measured, on `codex-cli 0.153.4`:** a codex pane opens on an animated splash
/// that ends on a keypress rather than on a timer — still animating after 75 s
/// untouched (`docs/notes/codex-clear-notes.md`) — and it **discards** everything
/// written to it until that keypress. A first message delivered into it is
/// silently gone while the pane looks perfectly healthy, which is exactly the lie
/// Tier 1.5 and D-034 exist to prevent.
///
/// **Why this is an enum and not a duration.** C26 refused a `startup_wait` field,
/// including a zero-valued one, on the grounds that a number encodes a guess where
/// a readiness check exists — and that a behaviour-preserving placeholder is how
/// such a field lands unopposed. Both objections are answered rather than dodged:
/// this carries no number for anyone to tune, and Claude Code's [`AtOnce`] is not
/// a placeholder awaiting a value but a measured statement about a harness that
/// can receive the moment it has a pty. The *when* stays in `pty.rs`, where it is
/// a condition on what the pane paints rather than a constant to be believed.
///
/// **Nothing here reaches the message path.** Both variants are read by
/// `PaneRegistry::spawn` and by nothing else; the difference between them is when
/// a pane is announced, never what happens to a message once one is.
///
/// [`AtOnce`]: BringUp::AtOnce
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BringUp {
    /// **The pane can receive as soon as it has a pty.** Claude Code's answer, and
    /// the one that changes nothing: the pane is announced inside `spawn`, exactly
    /// as every pane has been since there was a registry.
    AtOnce,
    /// **The pane opens on something that ends on a keypress and throws away what
    /// it is sent until then.** It is woken — and *observed* until it settles —
    /// before it is announced, so it is never addressable while it cannot receive.
    ///
    /// The wake happens on the bring-up path, before the pane is in the registry,
    /// which is where C26 said a harness that must be waited for belongs: it
    /// delays nothing `fleet send` can already reach, because `fleet send` cannot
    /// reach a pane that is not announced. A message sent into that window is
    /// refused for want of a pane and comes back `accepted: false` — the same
    /// answer a pane nobody has spawned gives, and an honest one, where a message
    /// swallowed by a splash gets a green `accepted` (Tier 1.5).
    AfterWaking,
}

/// **Checkpoint 9 — typing profile.**
///
/// How bytes get from the hub into this harness's input box and become a
/// submitted turn.
///
/// **This checkpoint is deliberately narrower than the measurement that motivated
/// it, and Tier 1.4 is why** (see `decisions.md` C26). What it carries is the
/// paste framing, the submit byte, and the one gap that already exists between the
/// paste and the carriage return — `pty::SUBMIT_GAP`, documented there as the only
/// delay anywhere between `fleet send` and a pty (D-034). What it deliberately
/// does **not** carry is a startup wait: a per-harness "wait N seconds after spawn
/// before typing" is a delay between `fleet send` and a pty, which Tier 1.4
/// forbids and which has been argued and lost twice. A harness that needs one
/// needs *readiness detection*, which is a mechanism rather than a number and does
/// not belong in a table of constants.
///
/// That mechanism now exists: [`BringUp`], read by `pty::PaneRegistry::spawn`
/// before the pane is announced. It is the thing C26 said the answer would have
/// to be, and it is still not a field here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypingProfile {
    /// Whether the message is framed as a bracketed paste, so a multi-line body
    /// arrives as one block rather than as N submitted turns.
    pub bracketed_paste: bool,
    /// The bytes that open a bracketed paste, when it is used.
    pub paste_start: &'static [u8],
    /// The bytes that close it.
    pub paste_end: &'static [u8],
    /// What submits the turn once the paste is closed.
    pub submit_bytes: &'static [u8],
    /// Milliseconds between closing the paste and submitting — enough for the
    /// TUI to have processed the paste, and the **only** gap on the message path.
    pub submit_gap_ms: u64,
}

/// **Checkpoint 10 — command-channel spellings.**
///
/// `fleet cmd` carries a small allowlist of slash commands (Tier 2,
/// `fleetor_core::command::ALLOWED_COMMANDS`), and this is how each one is spelled
/// for this harness. A row per allowlisted command, canonical name first.
///
/// A table rather than four named fields because the allowlist is Tier 2 and
/// widening it should cost a row here, not a field on a type five harnesses
/// implement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandChannel {
    /// `(the fleet's canonical spelling, this harness's spelling)`. A command the
    /// harness does not have is absent from the table rather than mapped to
    /// something close — a pane silently sent a command that does nothing is the
    /// failure this table exists to make visible.
    pub spellings: &'static [(&'static str, &'static str)],
}

/// **Checkpoint 11 — context-gauge source and window constant.**
///
/// Where the live gauge (WP-04) reads a worker's context usage from, and what it
/// divides by. The location of the transcript itself is checkpoint 13; this is the
/// *window*, which is a separate fact and a more dangerous one.
///
/// **D-054's rule holds for every harness: never synthesize a number.** A harness
/// that does not persist usage gets `window_tokens: None` and the rail says
/// `unavailable` — an estimate reconstructed from what the fleet sent is the
/// number that looks right and is wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GaugeSource {
    /// Whether the gauge has a reader for this harness's usage at all, under the
    /// pane's own configuration directory. `false` means the number is somewhere
    /// this module cannot reach, and the rail says unavailable.
    ///
    /// **The name is now narrower than the thing** (#41, C65), and saying so is
    /// cheaper than a rename that would touch every checkpoint-11 literal while
    /// three tickets are in flight. It was written when the only reader was Claude
    /// Code's `assistant`-line `usage` object, so "reads the transcript" and "has a
    /// reader" were the same sentence. C61 measured that they are not: codex's
    /// *transcript* — the `thread_history_1.sqlite` checkpoint 13 names — holds no
    /// usage field at all, and its number lives in two other files in the same
    /// `CODEX_HOME`. Both harnesses still answer `true`, and what that gates is
    /// unchanged: placement hands the gauge a source, and [`Harness::read_usage`]
    /// reads under it. **What would force the rename** is a third harness that
    /// needs to distinguish *which* reader, at which point this bool becomes a
    /// named enum the way `file_move_is_safe` became [`Transport`] (C54) — one
    /// bool that means more than it says is exactly what that decision corrected.
    pub reads_transcript: bool,
    /// The window the gauge divides by, when the fleet is the one asserting it.
    /// `Some(WORKER_WINDOW_TOKENS)` here: Claude Code does not recognize the
    /// worker model's name and would assume 200k and auto-compact early, so the
    /// fleet states the window and exports the identical constant to the pane
    /// through [`window_env`](Self::window_env) — one number, so the vendor's
    /// bookkeeping and the fleet's display cannot disagree (D-054).
    ///
    /// `None` for a harness that publishes the window itself, which is a strictly
    /// better answer than asserting one.
    pub window_tokens: Option<u32>,
    /// The variable the asserted window is exported to the pane through.
    pub window_env: Option<&'static str>,
}

/// **Checkpoint 12 — orphan-sweep process names.**
///
/// The `comm` suffixes the sweep confirms before it signals a pid.
///
/// **The sweep does not search by name, and that is what makes this safe**
/// (`orphans.rs`): it reads pids the fleet itself recorded and uses these names
/// only to confirm that a recorded pid is still the process it was. The operator's
/// own harness is never at risk, because its pid was never in the registry. A
/// missing name here does not kill the wrong process — it leaks a crashed pane as
/// a terminal-less orphan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanNames {
    /// Suffixes of `ps -o comm=` that identify a live pane of this harness.
    /// More than one where a harness runs under an interpreter and the bare
    /// interpreter name must never match on its own.
    pub comm_suffixes: &'static [&'static str],
}

/// **Checkpoint 13 — transcript location and format.**
///
/// What a pane leaves behind, where, and how the archive is allowed to take it.
///
/// **Nothing is normalized** (M24): the archive keeps each harness's raw format
/// and `manifest.json` declares which one it is. A normalizer between two vendors'
/// undocumented formats and the permanent record is wrong forever the moment it
/// drifts, and by then the raw evidence is gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    /// The directory under the pane's config dir that holds them.
    ///
    /// **The empty string is a real answer** meaning the configuration directory
    /// itself, and it is codex's (#33). Whether there is a per-project directory
    /// *below* this one, and what it would be called, is deliberately not
    /// answered here — see [`Transcript::subdir`]'s note in the conformance suite
    /// and C54. The harvest looks in this directory and one level below it, which
    /// is the whole of the variation the registry has.
    pub subdir: &'static str,
    /// The extension the harvest looks for.
    pub file_ext: &'static str,
    /// What `manifest.json` records as this pane's `transcript_format`, so a
    /// Critic reading a mixed run cold knows what it is holding (M24).
    pub format: &'static str,
    /// **How the archive is allowed to take it** — the field that used to be
    /// `file_move_is_safe: bool` (#39).
    ///
    /// A bool said *whether* a rename was safe and left the archive to infer the
    /// mechanism from the negative, so every harness answering `false` would have
    /// been handed SQLite's backup path whether or not its transcript was a SQLite
    /// database. A harness whose store is something else again would have been
    /// `VACUUM INTO`'d and failed quietly. Naming the transport makes that a
    /// non-exhaustive `match` in [`crate::runs`] instead: a fourth mechanism
    /// cannot be declared without the harvest being made to implement it.
    pub transport: Transport,
}

/// How the archive takes one harness's transcript — checkpoint 13's third answer.
///
/// **Both arms are implemented by [`crate::runs`], and that is the point of the
/// type.** A harness may not declare a transport the harvest does not have; the
/// conformance suite drives whichever one this names through rotation and asserts
/// the file arrives, so a new variant here is a compile error there until it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// Append-only files. A plain `rename` takes one whole, and the archive is
    /// byte-identical to what the pane wrote.
    Rename,
    /// A live SQLite database — `db` plus `-wal` plus `-shm`.
    ///
    /// **Never a rename and never a file copy.** Renaming the `.sqlite` alone
    /// leaves every transaction still in the write-ahead log behind; copying the
    /// three files gives an *untorn* copy only because rotation happens to run
    /// before any pane exists, and untorn is not the same property as complete.
    /// The archive is taken with SQLite's own backup path (`VACUUM INTO`), which
    /// reads the log as part of the database and writes one self-contained file
    /// with no journal beside it.
    SqliteBackup,
}

/// **Checkpoint 14 — project identity and trust seeding** (C17).
///
/// **Renamed, because the old name covered half of it.** It was "project-key
/// canonicalization", which named the key's shape but not the thing the key is
/// *for*: every harness parks a fresh pane on a first-run trust gate, and a pane
/// sitting on one looks completely healthy while every `fleet send` reports
/// `accepted` (L1). The key and the gate are one checkpoint because getting the
/// key wrong and not writing the flag at all produce the identical symptom.
///
/// The canonicalization itself is behaviour and lives on
/// [`Harness::project_key`] — `spawn::project_key` is Claude Code's own, and
/// `crate::deliver` and [`crate::context_gauge`] read it so the trust flag, the
/// delivery path and the gauge all agree on one spelling of a cwd.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectIdentityAndTrust {
    /// Whether the key is the resolved absolute path. `true`: on macOS `/tmp` and
    /// `/var` are symlinks and a child's own cwd comes back resolved, so an
    /// unresolved key would never match.
    pub canonicalize: bool,
    /// Whether the harness matches the key **exactly**. `true` here. A harness
    /// that resolves trust by repository root instead matches a parent of the
    /// pane's cwd, and seeding it per worktree would write rows it never reads.
    pub exact_path_match: bool,
    /// The file the trust record is written into — the same file as
    /// [`ConfigDir::seed_file`] for a harness that keeps both together.
    pub trust_file: &'static str,
    /// The per-project keys that have to be set for a pane to reach its prompt
    /// without a dialog. Re-applied for every pane cwd whenever the target
    /// changes; this is the single most likely way to reintroduce L1.
    pub trust_keys: &'static [&'static str],
}

// --- the behavioural half -----------------------------------------------------

/// Everything seeding one pane's configuration directory is allowed to look at
/// (WP-25 P2, #26).
///
/// **A struct rather than three arguments, because the list grows and the call
/// sites should not.** Checkpoints 4 and 6 need the pane's own directory, the cwd
/// its trust record is keyed by, and — for a harness whose isolated directory is a
/// snapshot of the operator's ([`ConfigAndCredentialIsolation::seeds_from_operator`])
/// — where the operator's own installation is. #29 adds the fleet's endpoint and
/// credential here when the FLEETOR provider lands; #30 adds whatever per-worktree
/// trust turns out to need. Each of those is a field here and no change at all in
/// the four `place_*` arms.
///
/// **Nothing on it is read from the process**, which is [`crate::placement`]'s one
/// rule: [`operator_home`](Self::operator_home) is
/// [`Host::operator_home`](crate::placement::Host::operator_home), discovered once
/// where machine facts are discovered and handed down as a value. That is what
/// lets a test seed against a *fabricated* operator installation — and it is the
/// mechanism by which the operator's real one is never touched, since a test that
/// hands a scratch path cannot reach `~/.codex` even by accident.
/// **`Clone` but no longer `Copy`** (#30). [`main_repository`](Self::main_repository)
/// is an owned path because it is *derived* from the cwd rather than handed in
/// alongside it, and there is nowhere for a borrow of it to live.
/// **The operator's own credential, read once before any pane exists** (C73, C75).
///
/// An opaque document and nothing more. What it contains is the harness's own
/// business — Claude Code's is a JSON object, codex's is the contents of an
/// `auth.json` — and this type deliberately cannot be inspected, only written,
/// so no code path outside a harness can grow a dependency on one vendor's shape.
///
/// **Constructed in exactly one place**, [`Harness::read_operator_login`], which
/// is reached from `Host::discover_for_the_gate` and from nothing else. That is
/// the property the whole design rests on: the read happens in an *unfenced*
/// process, on the operator's own `HOME`, with their login keychain on the search
/// list — the one moment it is reachable at all (`plan-worker-notes.md` §2).
///
/// **It is narrowed by the harness that reads it.** Claude Code's login keychain
/// item carries more than Claude Code's credential — on a machine with MCP
/// servers configured it also holds third-party OAuth tokens with their own
/// refresh tokens and client secrets — so the reader hands over the
/// `claudeAiOauth` object and never the item (`plan-worker-notes.md` §9).
///
/// `Debug` is implemented by hand and prints no bytes: this value ends up on a
/// [`Seed`], and a `Seed` is `Debug`.
#[derive(Clone, PartialEq, Eq)]
pub struct OperatorLogin {
    document: String,
}

impl OperatorLogin {
    /// Wrap a credential document a harness has just read and narrowed.
    pub fn new(document: String) -> Self {
        Self { document }
    }

    /// The document, for the one caller that writes it into a pane's own
    /// configuration directory.
    pub fn document(&self) -> &str {
        &self.document
    }
}

impl std::fmt::Debug for OperatorLogin {
    /// **Says the length and never the bytes.** Every value on a [`Seed`] reaches
    /// a `{:?}` somewhere eventually — a test failure, a notice, a log line — and
    /// a credential that formats itself is a credential in a transcript.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OperatorLogin({} bytes, redacted)", self.document.len())
    }
}

/// **Whose usage a seat spends** — the field C75 split out of
/// [`Seed::operators_own_seat`].
///
/// Those two questions had been one boolean, and `placement/codex.rs` said so in
/// as many words: it was "the one question deciding both halves", meaning
/// *whether a pane is the operator's own* and *whether it carries the operator's
/// credential*. A plan-backed worker is the combination that boolean cannot
/// spell — fenced, not the operator's own pane, and running on their login.
///
/// **The credential rides on the variant**, so "this seat is on the plan" cannot
/// be said without supplying the thing that makes it true. The alternative was a
/// bool beside an `Option`, which is the shape where a `true` with a `None` is a
/// pane that reaches its prompt logged out and looking healthy — D-062's failure
/// mode, and the one this whole arc exists to keep legible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialSource<'a> {
    /// The fleet's own metered key and endpoint — [`Credentials::token_env`] and
    /// [`Credentials::base_url_env`], with [`Credentials::scrubbed_env`] removed
    /// from the inherit. D-062's original rule, unchanged and still the default
    /// on a seat nobody thought about.
    FleetKey,
    /// The operator's existing login, planted in this pane's own configuration
    /// directory by its harness's seeder.
    ///
    /// **A seat on this variant is given no endpoint and no fleet token.** Both
    /// were measured to be wrong here rather than merely redundant: the fleet's
    /// endpoint is a different provider, and setting the token variable at all
    /// selects the metered path (C71, C73).
    OperatorsPlan(&'a OperatorLogin),
}

impl CredentialSource<'_> {
    /// Whether this seat spends the operator's plan. The one predicate the gate's
    /// count and the cost line both read, so they cannot disagree.
    pub fn is_the_operators_plan(&self) -> bool {
        matches!(self, CredentialSource::OperatorsPlan(_))
    }
}

#[derive(Debug, Clone)]
pub struct Seed<'a> {
    /// **Whether this is the operator's own pane** — the field #28 added, and the
    /// one that lets a harness narrow a pane FLEETOR drives without narrowing the
    /// pane the operator sits at.
    ///
    /// Codex is the first harness that needs it. Four of its 47 default-on
    /// features reach past the seatbelt it configures (C21), and they are turned
    /// off on every seat except the orchestrator's, which inherits the operator's
    /// flags untouched *because it is their own pane*. A harness with no per-seat
    /// answer ignores it, the way Claude Code's seeder does.
    ///
    /// **`false` is the default and that is the load-bearing half.** The narrowed
    /// answer is the safe one, so a seat nobody thought about — a fifth `place_*`
    /// arm, a harness added later — is fenced rather than trusted, and only
    /// [`place_orch`](crate::placement) says otherwise, in one line, on purpose.
    /// A `PaneId` here would read more precisely and would have no safe default at
    /// all under the builder shape C39 settled on.
    pub operators_own_seat: bool,
    /// **Whose usage this seat spends** (C75). See [`CredentialSource`] for why
    /// this is not a widening of [`operators_own_seat`](Self::operators_own_seat).
    ///
    /// **[`CredentialSource::FleetKey`] is the default here, and that stays true
    /// even though the operator-facing default is the plan.** The two are not the
    /// same default: the gate makes the choice explicitly and hands it down, so a
    /// seat that arrives here without one is a seat nobody thought about — a
    /// fifth `place_*` arm, a harness added later — and it must not spend the
    /// operator's plan by omission. What C75 changed is what the *operator* is
    /// offered, not what a forgotten code path gets.
    pub credential_source: CredentialSource<'a>,
    /// The pane's own configuration directory. **Every byte this call writes lands
    /// under here**, and a harness that writes anywhere else is the bug this field
    /// exists to make obvious.
    pub config_dir: &'a Path,
    /// The pane's working directory, which checkpoint 14's trust record is keyed
    /// by through [`Harness::project_key`].
    pub cwd: &'a Path,
    /// **The main repository [`cwd`](Self::cwd) belongs to**, canonicalized, or
    /// `None` where there is no git — checkpoint 14's *second* trust candidate
    /// (#30, C34).
    ///
    /// A harness that resolves trust by the cwd alone ignores it, as Claude Code's
    /// seeder does. Codex does not: it resolves a directory against *two* exact
    /// candidates — the canonicalized cwd, or the git root that cwd resolves to —
    /// and for a linked worktree that root is the **main repository**. A pane
    /// whose seed carries only the worktree key boots correctly at the worktree
    /// root and parks on the first-run gate the moment its cwd is one directory
    /// below it, which is the trap C34 measured (row 11).
    ///
    /// **The field is filled by [`Seed::new`] rather than by the four `place_*`
    /// arms** (C39's rule: a ticket adds a field here, not a fourth argument
    /// there), from [`main_repository`](crate::placement::main_repository) — the
    /// parent of `git rev-parse --path-format=absolute --git-common-dir`, never
    /// `--show-toplevel`. That is a subprocess, and it is still not a *process
    /// read* in [`crate::placement`]'s sense: it derives a fact from the path it
    /// was handed, so a test seeding against a scratch cwd cannot reach the
    /// operator's repository any more than it can reach their `~/.codex`.
    /// [`in_repository`](Self::in_repository) is how a test fabricates one
    /// without git.
    pub main_repository: Option<PathBuf>,
    /// The operator's own `HOME`, when this machine has one — the root the
    /// snapshot is taken from, for a harness that takes one. `None` on a machine
    /// nobody looked at, and a harness that seeds from the operator then seeds a
    /// clean directory rather than refusing: a pane with the fleet's own keys and
    /// none of the operator's preferences is a working pane, and refusing to place
    /// would be a fleet that cannot start because of a missing preference file.
    ///
    /// **It is the operator's `HOME`, not their config directory.** Which
    /// subdirectory of it holds this harness's installation is the harness's own
    /// answer and lives in its [`Harness::seed_config_dir`], for the same reason
    /// the document format does (C31, C32).
    pub operator_home: Option<&'a Path>,
    /// **This pane's rendered brief** — the same text a Claude Code pane of the
    /// same seat is handed, because D-042 holds and only the *carrier* varies
    /// (#27, C3).
    ///
    /// It is here because for a harness whose [`BriefCarrier::config_key`] is set,
    /// the brief is a **file inside the configuration directory** and the key
    /// naming it is a line in the same seed document — so writing it anywhere but
    /// here would be a second write of `config.toml` on every spawn, and a second
    /// chance for a pane to reach its prompt with half a configuration. A harness
    /// that carries its brief in argv ignores this field, exactly as the Claude
    /// Code seeder ignores [`operator_home`](Self::operator_home).
    ///
    /// Empty is the *unset* value rather than a legitimate one: a harness whose
    /// carrier is a config key refuses an empty brief rather than seeding a pane
    /// that would run on the vendor's own prompt while looking perfectly healthy.
    pub brief: &'a str,
}

impl<'a> Seed<'a> {
    /// The ordinary case: a pane's directory, its cwd, and the machine's operator
    /// `HOME`.
    ///
    /// The brief is unset. A harness that carries its brief in argv needs nothing
    /// more; one whose carrier is a config key refuses this seed, so the omission
    /// cannot pass quietly — see [`with_brief`](Self::with_brief).
    pub fn new(config_dir: &'a Path, cwd: &'a Path, operator_home: Option<&'a Path>) -> Self {
        Self {
            operators_own_seat: false,
            credential_source: CredentialSource::FleetKey,
            config_dir,
            cwd,
            main_repository: super::main_repository(cwd),
            operator_home,
            brief: "",
        }
    }

    /// The same seed, told which repository the cwd belongs to (#30, C34).
    ///
    /// **For a test that has no git**, and for a caller that already knows the
    /// answer. [`new`](Self::new) discovers it, so no production call site says
    /// this; what it buys is a unit test of the two-key trust record that does not
    /// have to build a repository and a linked worktree to reach the branch.
    pub fn in_repository(self, main_repository: &Path) -> Self {
        Self { main_repository: Some(main_repository.to_path_buf()), ..self }
    }

    /// The same seed, for the one pane the operator sits at (#28, C21).
    ///
    /// Said affirmatively and in exactly one place, so the default stays the
    /// narrow answer: a harness that fences what FLEETOR drives fences everything
    /// it was not explicitly told to leave alone.
    pub fn for_the_operator(self) -> Self {
        Self { operators_own_seat: true, ..self }
    }

    /// The same seed, spending the operator's own plan rather than the fleet's
    /// key (C73, C75).
    ///
    /// **Takes the credential, so the state cannot be half-set.** There is no way
    /// to mark a seat plan-backed and then fail to supply the login, which is the
    /// shape that produces a pane sitting at a prompt looking healthy while logged
    /// out.
    pub fn on_the_operators_plan(self, login: &'a OperatorLogin) -> Self {
        Self { credential_source: CredentialSource::OperatorsPlan(login), ..self }
    }

    /// The same seed carrying this pane's rendered brief.
    ///
    /// **A builder rather than a fourth positional argument**, for the reason C39
    /// gave for making this a struct at all: #29 and #30 add fields to the same
    /// call, and a constructor whose arity grows once per ticket is four `place_*`
    /// edits per ticket.
    pub fn with_brief(self, brief: &'a str) -> Self {
        Self { brief, ..self }
    }
}

/// Everything building one pane's argv is allowed to look at — checkpoints 1, 2
/// and 3's inputs (WP-25 #33, C39).
///
/// **A struct rather than a fourth positional argument, for the reason [`Seed`] is
/// one.** `command_args` began as `(brief, permission_mode)`, grew
/// `operators_own_seat` in #46, and grows the model here — checkpoint 3 declares
/// *two* channels for it, [`Posture::model_env`] and [`Posture::model_flag`], and
/// only the first had a call site. Claude Code takes its model in the
/// environment, so nothing was broken; codex names it in argv, so a harness that
/// answers `model_flag` could not be registered until this argument existed. C39
/// settled the shape the last time this happened: a ticket adds a field here, not
/// a fourth argument at three call sites.
///
/// **The seat asymmetry is the caller's to state, and every field says so.** The
/// operator's own pane runs their account, their model and their approval, so it
/// arrives with the brief alone; the two judges carry a posture and no model
/// (D-030, D-052); a worker carries both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seat<'a> {
    /// This pane's rendered brief, for a harness whose
    /// [`BriefCarrier::argv_flag`] carries it. One whose carrier is a config key
    /// ignores it here and reads [`Seed::brief`] instead.
    pub brief: &'a str,
    /// The model this pane runs, for a harness that names it in argv.
    ///
    /// `None` on every attended seat, and that is the product rather than an
    /// omission. A harness whose channel is [`Posture::model_env`] ignores it:
    /// the variable is set beside the command, not in it.
    pub model: Option<&'a str>,
    /// The permission posture, for the seats that get one. `None` for the
    /// operator's own, which is watched by a human who approves its calls.
    pub permission_mode: Option<&'a str>,
    /// **Whether this is the operator's own pane** — the same question
    /// [`Seed::operators_own_seat`] asks, and not derivable from
    /// [`permission_mode`](Self::permission_mode) (#46, C49, C50): the evaluator
    /// and the Critic both carry a posture *and* are seats FLEETOR drives, so
    /// inferring one from the other would hand the orchestrator's answer to two
    /// panes that must not have it.
    pub operators_own_seat: bool,
}

impl<'a> Seat<'a> {
    /// One unattended seat with nothing but its brief. The narrow answer is the
    /// default, exactly as [`Seed::new`]'s is.
    pub fn new(brief: &'a str) -> Self {
        Self { brief, model: None, permission_mode: None, operators_own_seat: false }
    }

    /// The same seat, running the model the caller named.
    pub fn with_model(self, model: &'a str) -> Self {
        Self { model: Some(model), ..self }
    }

    /// The same seat, bounded by the posture the caller named.
    pub fn with_permission_mode(self, permission_mode: &'a str) -> Self {
        Self { permission_mode: Some(permission_mode), ..self }
    }

    /// The same seat, for the one pane the operator sits at. Said affirmatively
    /// and in exactly one place, so the default stays the narrow answer.
    pub fn for_the_operator(self) -> Self {
        Self { operators_own_seat: true, ..self }
    }

}

/// A registered harness: its fourteen answers, plus the three checkpoints that
/// are functions rather than values (M23).
///
/// **Object-safe on purpose.** A harness is looked up as
/// `&'static dyn Harness` from
/// [`PaneSpec::harness`](crate::placement::PaneSpec::harness), so `placement` can
/// hold "whichever harness this pane runs" without being generic over it — which
/// would push the parameter through every `place_*` arm and every `spawn::*`
/// builder for no gain, since the choice is made once per pane and never varies
/// inside a placement.
///
/// **A method here is a checkpoint whose answer needs the pane's own inputs.**
/// Everything that can be `'static` data is on [`HarnessSpec`] instead, because
/// data can be asserted equal to the literal it replaces and a method can only be
/// asserted equal to itself.
pub trait Harness: std::fmt::Debug + Send + Sync + 'static {
    /// This harness's fourteen answers.
    fn spec(&self) -> &'static HarnessSpec;

    /// **Checkpoint 14's behavioural half:** the key this harness looks a project
    /// up under, given a pane's working directory.
    ///
    /// The trust flag, the delivery path and the context gauge must all agree on
    /// the answer, which is why it is one function rather than three spellings.
    fn project_key(&self, cwd: &Path) -> String;

    /// **Checkpoint 4's behavioural half**, which also writes checkpoint 14's
    /// trust record: give [`Seed::config_dir`] what a pane of this harness needs
    /// to reach its prompt in [`Seed::cwd`], without disturbing anything else
    /// already there.
    ///
    /// Merge rather than clobber where [`ConfigDir::seed_merges`] says so, and
    /// install through a temp file and a rename: a half-written config file is a
    /// pane that boots into onboarding, which is the failure this exists to
    /// prevent.
    ///
    /// **This is also the one reader of the three checkpoint key lists** —
    /// [`Posture::sandbox_keys`], [`Credentials::provider_keys`] and
    /// [`Outbound::reachability_keys`] (C36). One reader for all three, per
    /// harness, because a harness's config keys have exactly one place they get
    /// written and three readers would be three answers to that question.
    ///
    /// **What it returns is Activity feed lines, and #28 is why** (C21, story 23).
    /// A harness that seeds from the operator's own configuration
    /// ([`ConfigAndCredentialIsolation::seeds_from_operator`]) sometimes writes
    /// over a value the operator set on purpose — codex's sandbox mode and its
    /// sandbox network access are both FLEETOR's on every pane, and both are
    /// ordinary keys an operator may already have an opinion about. This is the
    /// only place that can tell: the call site sees a directory, and only the
    /// seeder sees the document it replaced. Returned rather than emitted, which
    /// is [`crate::prompts`]'s rule for the same reason — a notice a store has to
    /// exist to observe is a notice no test asserts.
    ///
    /// A harness with nothing to announce returns an empty vector, which is
    /// Claude Code's answer: its configuration directory is fleet-owned and
    /// nothing of the operator's is being overridden in it.
    fn seed_config_dir(&self, seed: &Seed<'_>) -> Result<Vec<(NoticeLevel, String)>, String>;

    /// **Read the operator's own credential, so a seat can spend their plan**
    /// (C73, C75) — or `None` when this harness has no such credential, or this
    /// machine has no login in it.
    ///
    /// **The one process read in the whole feature, and it is deliberately not in
    /// [`crate::placement`]'s call graph.** It is reached from
    /// `Host::discover_for_the_gate` and from nowhere else, which is what
    /// `tests/placement_reads_nothing.rs` exists to keep true. The timing is the
    /// mechanism, not a convenience: FLEETOR is unsandboxed when the gate runs, so
    /// the operator's `HOME` is theirs and their login keychain is on the search
    /// list. A pane cannot do this for itself — a private `HOME` removes the
    /// keychain from that list entirely (`plan-worker-notes.md` §2).
    ///
    /// `operator_home` is the operator's own `HOME`, the same value
    /// [`Seed::operator_home`] carries, and `None` on a machine nobody looked at.
    /// A harness whose credential is a system store rather than a file under that
    /// home ignores it, as Claude Code's reader does.
    ///
    /// **The implementation narrows before it returns.** A vendor's credential
    /// store may hold credentials that are not this vendor's — Claude Code's
    /// keychain item carries third-party MCP OAuth tokens on a machine with MCP
    /// servers configured — and handing those to a fenced pane would leak
    /// credentials with nothing to do with FLEETOR (`plan-worker-notes.md` §9).
    ///
    /// The default is `None`: a harness that has not answered this cannot take a
    /// plan seat, which fails toward the fleet's key rather than toward a pane
    /// that looks healthy while logged out.
    fn read_operator_login(&self, operator_home: Option<&std::path::Path>) -> Option<OperatorLogin> {
        let _ = operator_home;
        None
    }

    /// **Checkpoint 7's behavioural half:** register the fleet's write guardrail
    /// in this harness's own settings document, keeping everything the operator
    /// already put there and leaving exactly one of ours.
    ///
    /// **A method rather than a shared function, and C32 called this in advance.**
    /// The four names on [`GuardrailInstall`] migrated onto the spec for free; the
    /// settings *document model* did not, and could not — Claude Code's is JSON
    /// with a top-level `hooks` object and codex's `config.toml` is TOML, so a
    /// data-shaped description of the entry would have been a schema language
    /// invented for one vendor before the second was measured. This is checkpoint
    /// 4's precedent exactly (C16, C31): the document format is the vendor's, so
    /// it belongs to a method; the roots, the script, the interpreter, the hook
    /// command and the journal stay the fleet's free functions in
    /// [`crate::guardrail`] beneath it.
    ///
    /// **Tier 1.7 does not pass through here.** Every root arrives already
    /// computed on [`GuardrailPlacement::roots`], from the pane's own cwd, and an
    /// implementation of this method has no way to widen them — which is
    /// `building.md` §9.2 held structurally rather than by review.
    ///
    /// Merge, never clobber — **on the operator's own seat** (C49). D-062
    /// explicitly invites them to populate `pane-config/orch/` themselves, and
    /// their own hooks living in this file is the obvious way to do it. On a seat
    /// FLEETOR drives, a harness that cannot establish trust for a single hook may
    /// instead **own the pane's whole hook table** and trust the result, which
    /// narrows what the pane may execute rather than widening it: see
    /// [`GuardrailPlacement::operators_own_seat`].
    ///
    /// [`GuardrailPlacement::roots`]: crate::guardrail::GuardrailPlacement::roots
    /// [`GuardrailPlacement::operators_own_seat`]: crate::guardrail::GuardrailPlacement::operators_own_seat
    fn install_guardrail(
        &self,
        at: &crate::guardrail::GuardrailPlacement<'_>,
    ) -> Result<Vec<(NoticeLevel, String)>, String>;

    /// **Checkpoints 1, 2 and 3's behavioural half:** the argv one pane is
    /// launched with, given everything about that seat a command may look at.
    ///
    /// Every asymmetry the seats carry is on [`Seat`] and stated by the caller,
    /// because the caller is the only one that knows which seat this is (D-030,
    /// D-052). A harness that has no use for a field ignores it, exactly as Claude
    /// Code ignores [`Seat::model`] — its model travels in
    /// [`Posture::model_env`] beside the command rather than inside it.
    fn command_args(&self, seat: &Seat<'_>) -> Vec<String>;

    /// **Checkpoint 11's behavioural half:** this harness's own accounting of its
    /// own turn, read back off the pane's configuration directory (#41, C61, C65).
    ///
    /// **A method rather than a fifteenth field, and C54 called this in advance.**
    /// Where a number *lives* is a static fact and belongs on [`GaugeSource`];
    /// getting it out is a computation over files under the pane's own config
    /// directory, and C54 already found that shape — `project_transcript_dir`,
    /// `trust_candidates` — to be a method every time. Claude Code's answer is a
    /// `usage` object on the last `assistant` line of its own transcript, so that
    /// is the default; a harness that keeps the number somewhere else overrides
    /// this and no arm in the gauge has to name it.
    ///
    /// **The one rule, and it is absolute (D-054, M22, C24).** What is returned is
    /// a figure *the vendor itself wrote to disk*. It is never derived from the
    /// prompt FLEETOR assembled, never a chars/4 estimate, and never a running
    /// total standing in for occupancy. A harness with no such record on disk
    /// answers `None` and the rail says unavailable — an absent gauge is strictly
    /// better than a number that looks right and is wrong.
    ///
    /// [`UsageReading::window_tokens`](crate::context_gauge::UsageReading::window_tokens)
    /// is how a harness reports the window *it* was measured against, which takes
    /// precedence over [`GaugeSource::window_tokens`] for exactly the reason that
    /// field exists: a rail that disagrees with the vendor's own display is the
    /// two-numbers failure D-054 forbids.
    fn read_usage(
        &self,
        source: &crate::context_gauge::TranscriptSource,
    ) -> Option<crate::context_gauge::UsageReading> {
        crate::context_gauge::transcript_usage(source)
    }
}

// --- Claude Code --------------------------------------------------------------

/// Claude Code — the harness every pane has run since before there was a seam,
/// and for now the only registered one.
///
/// A unit struct: everything it knows is in [`CLAUDE_CODE_SPEC`], and two of its
/// three methods delegate to `spawn::project_key` and `spawn::seed_config_dir`.
/// **Those two are Claude Code's own answers to checkpoints 14 and 4** (C31), not
/// the fleet's assumptions — which is why the contract step left them where they
/// are and deleted nothing there. They are reached through [`Harness`] and called
/// directly by no one.
///
/// It is registered *and* wired through: every literal those eight files carried
/// now comes off this spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClaudeCode;

/// Claude Code's fourteen answers, each one stating what a literal somewhere in
/// `src-tauri/src` already does.
///
/// **Where a public constant already holds the value, this names it rather than
/// repeating it** — the guardrail's hook file, the gauge's window — so those two
/// cannot drift by editing one side. The rest are literals here and pinned by
/// the `tests` module below against their originals.
pub const CLAUDE_CODE_SPEC: HarnessSpec = HarnessSpec {
    name: "claude-code",
    mark: "CC",

    // How an operator logs it in (#51). **Two steps, because its login is a
    // command inside its own session** rather than a subcommand — the move
    // `placement::mod`'s own note has named since WP-14 ("run `/login` inside that
    // pane once"), stated here so the gate can say it before a pane exists.
    //
    // The credential lands in the operator's login keychain, which #49 measured
    // and which is the reason FLEETOR does not offer to take it: a token pasted
    // into this app would not be in the keychain, and the vendor reads the
    // keychain.
    login: LoginInstruction {
        command: "claude",
        then: Some("/login"),
        credential_home: "your login keychain",
    },

    // 1 — program and base arguments. `spawn::base_command_with`.
    program: Program { bin: "claude", base_args: &[] },

    // 2 — brief carrier. `--system-prompt` replaces the vendor prompt (D-043),
    // and nothing is written into the pane's checkout.
    brief: BriefCarrier {
        argv_flag: Some("--system-prompt"),
        config_key: None,
        replaces_system_prompt: true,
        writes_into_worktree: false,
    },

    // 3 — model and permission posture. No sandbox of its own; the write
    // guardrail and the Fence are what bound a pane.
    posture: Posture {
        model_env: Some("ANTHROPIC_MODEL"),
        model_flag: None,
        // **The vendor's own aliases, from its own `--help`** (C78): "an alias
        // for the latest model (e.g. 'fable', 'opus', or 'sonnet') or a model's
        // full name (e.g. 'claude-fable-5')". An alias is deliberately preferred
        // to a dated id here — it tracks the latest of each family, so this list
        // does not go stale every release.
        //
        // **Offered on a seat running the operator's own login and nowhere else.**
        // A worker on the fleet's key talks to FLEETOR's provider, where none of
        // these names resolves; the gate picks the list per credential.
        declared_models: &[
            ("fable", "Claude Fable 5.1"),
            ("opus", "Claude Opus 5"),
            ("sonnet", "Claude Sonnet 5"),
            ("haiku", "Claude Haiku 4.5"),
        ],
        permission_flag: Some("--permission-mode"),
        sandbox_keys: &[],
        // **Nothing to read back, which is an answer rather than a gap** (#37).
        // Claude Code has no sandbox of its own and publishes no diagnostic that
        // reports a resolved containment, so there is no vendor reading to compare
        // an empty `sandbox_keys` against. An expectation invented here would be a
        // tripwire with no measurement behind it.
        verified_as: &[],
        verified_against_schema: None,
    },

    // 4 — config dir and its seeding. `spawn::seed_config_dir`.
    config_dir: ConfigDir {
        env_var: "CLAUDE_CONFIG_DIR",
        seed_file: ".claude.json",
        seed_keys: &["hasCompletedOnboarding"],
        seed_merges: true,
    },

    // 5 — credential wiring and the scrub. `spawn::worker_command_with`.
    credentials: Credentials {
        base_url_env: Some("ANTHROPIC_BASE_URL"),
        token_env: Some("ANTHROPIC_AUTH_TOKEN"),
        provider_keys: &[],
        scrubbed_env: &[
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "CLAUDE_SECURESTORAGE_CONFIG_DIR",
            // **A D-062 hole found while measuring #49, and fixed independently of
            // it** (C74). This variable overrides `/login` — the vendor's own
            // `/login` warns when it is set — so an operator carrying one in a
            // shell profile authenticates every fenced worker on their personal
            // account, silently and without the plan seat being chosen. It is
            // scrubbed for `ANTHROPIC_API_KEY`'s reason and not for this ticket's:
            // a plan seat's credential arrives as a file in checkpoint 4's
            // directory, never in the environment, so removing this name costs the
            // feature nothing.
            "CLAUDE_CODE_OAUTH_TOKEN",
        ],
    },

    // 6 — config and credential isolation (C17, split per channel by C73).
    // Configuration relocates by variable; the fleet's token is a variable and
    // does not follow HOME; the *operator's* keychain entry does, transitively,
    // through a search list derived from HOME. Two channels, two answers.
    isolation: ConfigAndCredentialIsolation {
        config_env: "CLAUDE_CONFIG_DIR",
        credential_env: Some(super::spawn::ENV_CC_SECURESTORAGE_DIR),
        token_follows_home: false,
        // **Measured, and it is macOS's fact rather than this vendor's** (C73,
        // `plan-worker-notes.md` §2): `security list-keychains` under a private
        // `HOME` returns the System keychain alone, so the entry `credential_env`
        // names is not on the search list at all. Setting that variable correctly
        // buys nothing once `HOME` has been replaced.
        operator_store_follows_home: true,
        private_home: true,
        seeds_from_operator: false,
    },

    // 7 — write-guardrail install. The installer is `ClaudeCode::install_guardrail`
    // below, because the document is this vendor's JSON (C32); the matcher it
    // writes is `write_tools` joined with `|`, which is Claude Code's syntax for
    // the list rather than a second copy of it.
    guardrail: GuardrailInstall {
        settings_file: "settings.json",
        hook_event: "PreToolUse",
        write_tools: &["Bash", "Write", "Edit", "MultiEdit", "NotebookEdit"],
        hook_file: guardrail::HOOK_FILE,
    },

    // 8 — outbound reachability. No seatbelt, so the socket is simply reachable.
    outbound: Outbound { sandboxed: false, socket_reachable: true, reachability_keys: &[] },

    // Bring-up. Claude Code reaches a composer that accepts input on its own, so
    // a pane of it is announced the moment it has a pty — which is what every
    // pane has always done, and what keeps this a widening rather than a change.
    bring_up: BringUp::AtOnce,

    // 9 — typing profile. `pty::write_paste` reads it off the pane's own harness
    // (C35). The four values were `pty`'s constants until the contract batch
    // (#23); they are Claude Code's answers rather than the pty driver's, so they
    // live here now and `pty.rs` no longer spells them at all. See the type's doc
    // for why there is no startup wait.
    //
    // `submit_gap_ms` is D-034's one delay between `fleet send` and a pty: phase 0
    // measured 0/10/30 ms and all three submit, because pty stream ordering is
    // preserved; 30 ms anyway, so the fleet does not depend on the TUI batching
    // the end marker and the CR within one input-handler tick.
    typing: TypingProfile {
        bracketed_paste: true,
        paste_start: b"\x1b[200~",
        paste_end: b"\x1b[201~",
        submit_bytes: b"\r",
        submit_gap_ms: 30,
    },

    // 10 — command-channel spellings, one row per allowlisted command.
    commands: CommandChannel { spellings: &[("/clear", "/clear"), ("/compact", "/compact")] },

    // 11 — gauge source and window. The fleet asserts the window because the
    // vendor does not recognize the worker model's name (D-054).
    gauge: GaugeSource {
        reads_transcript: true,
        window_tokens: Some(crate::context_gauge::WORKER_WINDOW_TOKENS),
        window_env: Some("CLAUDE_CODE_MAX_CONTEXT_TOKENS"),
    },

    // 12 — orphan-sweep names. `orphans::sweep_at`.
    orphans: OrphanNames { comm_suffixes: &["claude"] },

    // 13 — transcript location and format. `runs::harvest_transcripts` moves
    // these, which is safe because they are append-only files.
    transcript: Transcript {
        subdir: "projects",
        file_ext: "jsonl",
        format: "claude-code-jsonl",
        transport: Transport::Rename,
    },

    // 14 — project identity and trust seeding (C17). `spawn::project_key` and the
    // per-project half of `spawn::seed_config_dir`.
    project_identity: ProjectIdentityAndTrust {
        canonicalize: true,
        exact_path_match: true,
        trust_file: ".claude.json",
        trust_keys: &["hasTrustDialogAccepted", "hasCompletedProjectOnboarding"],
    },
};

/// The macOS login-keychain item Claude Code's own `/login` writes.
///
/// **Named here rather than on the spec**, because it is not one of the fourteen:
/// the checkpoints are facts a bring-up reads, and this is the address of a store
/// only [`ClaudeCode::read_operator_login`] ever opens.
const KEYCHAIN_SERVICE: &str = "Claude Code-credentials";

/// The file Claude Code reads a credential out of, inside its configuration
/// directory (C73, `plan-worker-notes.md` §12).
const CREDENTIALS_FILE: &str = ".credentials.json";

/// Write the operator's login where this pane can read it, at `0600`.
///
/// **The mode is the vendor's own**, matching what its file store chmods its
/// writes to — and it is the whole of the protection this file gets. The
/// directory is fleet-owned and disposable, which is what makes that acceptable:
/// a refreshed credential the pane writes back lands in the same place and is
/// thrown away with the pane.
///
/// **Truncating rather than merging.** A credential is one document with one
/// writer; merging would mean a stale half surviving a re-seed, which is the
/// shape that produces a pane authenticating as something the operator no longer
/// is.
fn write_operator_login(config_dir: &Path, login: &OperatorLogin) -> Result<(), String> {
    let path = config_dir.join(CREDENTIALS_FILE);
    std::fs::write(&path, login.document())
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("chmod {}: {e}", path.display()))?;
    }
    Ok(())
}

impl Harness for ClaudeCode {
    fn spec(&self) -> &'static HarnessSpec {
        &CLAUDE_CODE_SPEC
    }

    fn project_key(&self, cwd: &Path) -> String {
        super::spawn::project_key(cwd)
    }

    fn seed_config_dir(&self, seed: &Seed<'_>) -> Result<Vec<(NoticeLevel, String)>, String> {
        // Nothing to announce: this directory is the fleet's own, so no key
        // written into it is standing on top of an answer the operator gave.
        super::spawn::seed_config_dir(self, seed)?;
        // **Checkpoint 6's plan-seat half** (C73, C75). The credential goes here
        // and only here: a fenced pane has no login keychain on its search list,
        // and its own configuration directory is the one store it can still read.
        // Measured, not assumed — `plan-worker-notes.md` §12.
        if let CredentialSource::OperatorsPlan(login) = seed.credential_source {
            write_operator_login(seed.config_dir, login)?;
        }
        Ok(Vec::new())
    }

    /// **Claude Code's login is a macOS login-keychain item**, not a file, which
    /// is why `operator_home` goes unread here (C73).
    ///
    /// **Narrowed to `claudeAiOauth` before it returns, and that is not caution
    /// for its own sake.** The item is a JSON document holding more than this
    /// vendor's credential: on a machine with MCP servers configured it also
    /// carries their OAuth access tokens, refresh tokens and client secrets
    /// (`plan-worker-notes.md` §9). Handing the *item* to a fenced pane would put
    /// credentials with nothing to do with FLEETOR inside an unattended worktree.
    ///
    /// Read-only: `find-generic-password -w` does not modify the item.
    fn read_operator_login(&self, _operator_home: Option<&Path>) -> Option<OperatorLogin> {
        let out = std::process::Command::new("security")
            .args(["find-generic-password", "-s", KEYCHAIN_SERVICE, "-w"])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let raw = String::from_utf8(out.stdout).ok()?;
        let item: serde_json::Value = serde_json::from_str(raw.trim()).ok()?;
        let oauth = item.get("claudeAiOauth")?;
        if !oauth.is_object() {
            return None;
        }
        let mut narrowed = serde_json::Map::new();
        narrowed.insert("claudeAiOauth".to_string(), oauth.clone());
        serde_json::to_string(&serde_json::Value::Object(narrowed)).ok().map(OperatorLogin::new)
    }

    /// Claude Code's settings document: JSON, a top-level `hooks` object keyed by
    /// event, each event an array of `{matcher, hooks:[{type, command}]}`.
    ///
    /// **This body is the thing C32 said could not be data.** It was
    /// `guardrail::merge_hook` and read as though it were general; it was one
    /// vendor's document all along, and codex's TOML is what made that visible.
    ///
    /// The matcher is rendered from [`GuardrailInstall::write_tools`] rather than
    /// stored beside it — an alternation is Claude Code's *syntax* for the same
    /// list the script is handed, and two fields holding one answer is the drift
    /// this ticket exists to close.
    fn install_guardrail(
        &self,
        at: &crate::guardrail::GuardrailPlacement<'_>,
    ) -> Result<Vec<(NoticeLevel, String)>, String> {
        let spec = &self.spec().guardrail;
        let (command, notices) = guardrail::prepare(spec, at)?;

        let mut root = guardrail::existing_document(spec, at.config_dir)
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .filter(serde_json::Value::is_object)
            .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));

        let object = root.as_object_mut().expect("just filtered to an object");
        let hooks = object
            .entry("hooks")
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        if !hooks.is_object() {
            *hooks = serde_json::Value::Object(serde_json::Map::new());
        }
        let hooks = hooks.as_object_mut().expect("just ensured it is an object");
        let pre =
            hooks.entry(spec.hook_event).or_insert_with(|| serde_json::Value::Array(Vec::new()));
        if !pre.is_array() {
            *pre = serde_json::Value::Array(Vec::new());
        }
        let pre = pre.as_array_mut().expect("just ensured it is an array");
        // Ours is replaced rather than appended to, so a relaunch does not
        // accumulate five copies that would each run and each report.
        pre.retain(|entry| !mentions_our_hook(entry, spec));
        pre.push(serde_json::json!({
            "matcher": spec.write_tools.join("|"),
            "hooks": [{ "type": "command", "command": command }],
        }));

        let text = serde_json::to_string_pretty(&root)
            .map_err(|e| format!("encode settings: {e}"))?;
        guardrail::install_document(spec, at.config_dir, &text)?;
        Ok(notices)
    }

    /// Claude Code's model channel is [`Posture::model_env`], so
    /// [`Seat::model`] is read here and produces nothing: `model_flag` is `None`,
    /// and a flag this harness does not have is a flag it must not be given.
    fn command_args(&self, seat: &Seat<'_>) -> Vec<String> {
        let spec = self.spec();
        let mut args: Vec<String> =
            spec.program.base_args.iter().map(|a| (*a).to_string()).collect();
        if let (Some(flag), Some(model)) = (spec.posture.model_flag, seat.model) {
            args.push(flag.to_string());
            args.push(model.to_string());
        }
        if let (Some(flag), Some(mode)) = (spec.posture.permission_flag, seat.permission_mode) {
            args.push(flag.to_string());
            args.push(mode.to_string());
        }
        if let Some(flag) = spec.brief.argv_flag {
            args.push(flag.to_string());
            args.push(seat.brief.to_string());
        }
        args
    }
}

/// Is this `PreToolUse` entry one of ours, from a previous launch?
///
/// Claude Code's document shape, so it lives with Claude Code. The *recognition*
/// is the fleet's — the hook file's name — and that half is
/// [`guardrail::is_our_command`].
fn mentions_our_hook(entry: &serde_json::Value, spec: &GuardrailInstall) -> bool {
    entry
        .get("hooks")
        .and_then(|h| h.as_array())
        .map(|hooks| {
            hooks.iter().any(|h| {
                h.get("command")
                    .and_then(|c| c.as_str())
                    .is_some_and(|c| guardrail::is_our_command(c, spec))
            })
        })
        .unwrap_or(false)
}

// --- the registry -------------------------------------------------------------

static CLAUDE_CODE: ClaudeCode = ClaudeCode;

/// Every registered harness. **Still exactly one, and #39 removed the last thing
/// standing in the way of two** (C20, C25, C53, C54).
///
/// Phase 1 held this at one on purpose: a seam green with one harness before a
/// second exists is a seam, and one written beside a second is a description of
/// that second one. #33 flipped this line to two, ran the suite over both, and
/// **found seven things the second pass was the only way to find** — six
/// checkpoint assertions shaped around the one harness they were written against,
/// all reshaped and landed here, and one that is not a reshape at all:
/// checkpoint 13 refused a harness whose transcript is a live database, because
/// the harvest had no mechanism for one. That refusal was correct, so the flip
/// was reverted and the findings kept.
///
/// **#39 landed the mechanism and the refusal is gone** (C54). [`crate::runs`]
/// takes a transcript by the [`Transport`] its harness declares, `SqliteBackup`
/// included, and the flip was made a second time, run, and reverted: all fifteen
/// conformance tests pass over both harnesses, checkpoint 13 among them.
/// **The follow-up is now exactly two lines with nothing in front of them** —
/// this array, and the pin below. Everything else a second entry needs is already
/// here, and #39 is deliberately not the ticket that types them: a registration
/// is a gesture an operator makes, not a side effect of the ticket that unblocked
/// it.
///
/// **Nothing may be added here without a conformance pass behind it.** The suite
/// runs one pass per entry, end to end through [`place`](super::place), and it
/// refuses the answers a half-implemented harness would give — that is what makes
/// that line the registration and not a declaration.
static REGISTERED: [&dyn Harness; 2] = [&CLAUDE_CODE, &super::codex::CODEX];

/// Every harness a pane may run.
pub fn registered() -> &'static [&'static dyn Harness] {
    &REGISTERED
}

/// Claude Code, by name — the harness
/// [`PaneSpec::harness`](crate::placement::PaneSpec::harness) answers for the two
/// judges (C15), and the one every caller names until the gate offers a choice
/// (#35).
pub fn claude_code() -> &'static dyn Harness {
    &CLAUDE_CODE
}

/// A registered harness by name, or `None`. The lookup a manifest read needs when
/// it has a `harness` string and wants the spec behind it (M24).
pub fn by_name(name: &str) -> Option<&'static dyn Harness> {
    registered().iter().copied().find(|h| h.spec().name == name)
}

// --- what a harness reports about this machine (WP-25 #34; C8, C14) ------------

/// **What one harness reports about *this machine*, read at the gate** (WP-25 #34;
/// C8, C14, C47).
///
/// **Not a fifteenth checkpoint, and deliberately not a method on [`Harness`].**
/// The fourteen are answers about a *vendor* — what it is called, which key carries
/// a brief, how it stores a transcript — and every one of them is a `'static` value
/// or a pure function of the pane's own inputs. This is the other kind of fact
/// entirely: it is what the operator's installation happens to be right now, it
/// changes when they run `codex login`, and reading it costs a subprocess and a
/// network round trip. That is [`Host`](super::Host)'s job, which is why this is a
/// value [`Host`] carries rather than a trait method a harness answers.
///
/// **Nothing in `placement`'s placing path constructs one.** It is built by
/// [`Host::discover_for_the_gate`](super::Host::discover_for_the_gate) and by
/// nothing else in production — asserted, not promised, in
/// `tests/placement_reads_nothing.rs`.
///
/// **Every field is public and every one is a plain owned value**, so a test builds
/// the machine it wants to describe rather than arranging for the operator's
/// installation to be in that state. That is [`Host::bare`](super::Host::bare)'s
/// property applied one level down.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HarnessReadiness {
    /// Which harness this describes.
    pub harness: &'static HarnessSpec,
    /// The program name as invoked — checkpoint 1's [`Program::bin`], resolved by
    /// the operator's own `PATH`.
    pub invoked: String,
    /// **The vendor binary behind that name, absolutely** (C47). `None` when the
    /// probe could not resolve one, and then [`Self::readings_agree`] is `false`
    /// because there was only ever one reading.
    ///
    /// C47's rule is that any measurement of flags or resolved configuration takes
    /// both readings, because the `codex` on a developer's `PATH` may be a wrapper
    /// that injects flags — and on the machine this was written on it is.
    pub resolved: Option<PathBuf>,
    /// Whether the two readings said the same thing about everything below.
    ///
    /// **The half of C47 that is for the operator rather than for the suite.** A
    /// wrapper that changes what the vendor resolves is a fleet whose gate
    /// describes a configuration no pane will run under, and the operator is the
    /// only one who can do anything about it.
    pub readings_agree: bool,
    /// The vendor's own version string, as it reported it.
    pub version: Option<String>,
    /// Whether this machine has a usable credential, and in which shape.
    pub login: LoginState,
    /// The model provider the vendor resolved — a fact displayed beside the model,
    /// never a control (C2 as amended by C9).
    pub provider: Option<String>,
    /// **What the vendor's own catalog resolution offers**, in the vendor's own
    /// order (C2).
    ///
    /// Read out of the vendor rather than out of its catalog file, so a picker's
    /// options are what the harness would actually accept — an operator pointing
    /// `model_catalog_json` at a file of their own gets *their* list here, and a
    /// reader of the file would have got it only by reimplementing the vendor's
    /// resolution and its `~` handling.
    ///
    /// Empty on a machine whose harness has no catalog to resolve, which is a real
    /// answer and not a failure: it means the gate offers no list and the operator
    /// types a name.
    pub models: Vec<ModelChoice>,
    /// The containment the vendor *resolved*, which is not the keys FLEETOR wrote
    /// (C8).
    pub posture: ResolvedPosture,
}

/// One model a harness would accept, as the vendor names it.
///
/// Two strings because a picker needs both and they differ: `gpt-5.6-sol` is what
/// goes on the argv, `GPT-5.6-Sol` is what a human recognises.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelChoice {
    /// What the harness accepts — checkpoint 3's `--model` value.
    pub slug: String,
    /// What the vendor calls it in front of a person.
    pub display_name: String,
}

/// Whether this machine can authenticate a seat on one harness, and how.
///
/// **The refusal is one variant and it is narrow on purpose** (C14). Codex has
/// three credential shapes and any of them is a login; refusing anything but "no
/// usable credential at all" would be story 9's *a supported feature looks
/// unimplemented*, at the gate. C9 is what makes that proportionate — the
/// orchestrator is the only seat that spends the operator's own credential, so the
/// gate is deciding about one pane rather than about a fleet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoginState {
    /// A usable credential, in the shape the vendor reported.
    LoggedIn(AccountShape),
    /// **The one refusal**: the vendor resolved no usable credential at all,
    /// carrying the vendor's own sentence about why.
    NoCredential {
        /// What the vendor said, verbatim — the operator's most actionable line.
        summary: String,
        /// **The named variable a custom provider authenticates through, when the
        /// vendor reported one and reported it missing** (#51, C14's third shape).
        ///
        /// `None` is the ordinary no-credential state, and its fix is the vendor's
        /// login command. `Some` is the state where **no login command helps at
        /// all**: the operator has configured a provider of their own and the key
        /// it names is not set, and #29 measured that a named `env_key` has no
        /// ambient fallback to pick up instead. One refusal, two different next
        /// actions — a field rather than a fifth status word, because the *seat*
        /// is refused identically either way and the four status words are pinned
        /// against `ui/src/fleet/types.ts` (C72).
        provider_key: Option<ProviderKey>,
    },
    /// The harness's binary is not on this machine, so there was nothing to ask.
    NotInstalled,
    /// The probe ran and its answer could not be read — a vendor that changed its
    /// report format, or one that would not start.
    ///
    /// **Not a refusal and not a login.** A gate that treated an unreadable probe
    /// as "logged out" would refuse a working installation on the strength of a
    /// parse error, which is the failure mode C14's narrowness exists to avoid.
    Unreadable {
        /// What went wrong, for the operator rather than for a log.
        why: String,
    },
}

impl LoginState {
    /// Whether the gate may offer this harness a seat.
    pub fn is_logged_in(&self) -> bool {
        matches!(self, LoginState::LoggedIn(_))
    }

    /// **What a passing probe does not prove, in the operator's words** (C14, M17).
    ///
    /// `Some` exactly when this machine looks logged in, because that is the only
    /// state where the caveat can mislead somebody. A vendor's diagnostic reports
    /// that a credential resolved and that its provider answered — not that the
    /// provider *accepted* it. Codex's probe returned **HTTP 401 against a live
    /// provider and still counted as reachable**, and Claude Code's login check is
    /// the same class: `~/.claude.json`'s presence proves a login happened, not
    /// that the token still works (M17).
    ///
    /// **Returned rather than logged, so it reaches the operator.** The rejected
    /// alternative was a live authenticated probe at gate time, which is real and
    /// which on a subscription plan spends quota before the operator has agreed to
    /// spend anything — precisely what the gate's single-screen cost statement
    /// exists to prevent. Having refused the check, the fleet owes the operator the
    /// sentence.
    pub fn caveat(&self) -> Option<&'static str> {
        self.is_logged_in().then_some(REACHABILITY_NOT_AUTHORIZATION)
    }

    /// **Whether a seat on this harness could actually spawn** (WP-25 #36, story
    /// 11).
    ///
    /// Two of the four states are a refusal and two are not, and the pair that is
    /// not is the whole of C14's narrowness. `Unreadable` is a *working*
    /// installation whose report this build could not parse; refusing it would stop
    /// a fleet on the strength of a parse error, which is the failure the narrow
    /// refusal exists to avoid. `LoggedIn` is obvious. The other two are the states
    /// in which a pane would come up on a login prompt.
    ///
    /// **The one spelling of the rule.** `ui/src/fleet/types.ts`'s
    /// `CANNOT_TAKE_A_SEAT` lists the same two status words for the interface, and
    /// `src-tauri/tests/gate_refusal.rs` reads both and fails if they part company —
    /// a gate that greys out a row the backend would happily start, or starts one
    /// the row greyed out, is exactly the silent disagreement that test exists for.
    pub fn can_take_a_seat(&self) -> bool {
        !matches!(self, LoginState::NoCredential { .. } | LoginState::NotInstalled)
    }
}

/// The sentence for a harness whose binary is not on the `PATH` this app inherited.
///
/// A free function because two callers need the identical words — the gate's row
/// (`HarnessOffer::from`) and the start refusal ([`HarnessReadiness::refusal`]) —
/// and an operator told two different things about one machine trusts neither.
pub fn not_on_the_path(invoked: &str) -> String {
    format!("`{invoked}` is not on the PATH this app was launched with, so no seat can run it.")
}

/// The sentence [`LoginState::caveat`] returns, pinned by a test for the reason
/// every other operator-facing constant in this module is: it is the whole of what
/// the gate says about a check it deliberately did not make.
pub const REACHABILITY_NOT_AUTHORIZATION: &str =
    "this only proves a credential resolved and its provider answered — not that the \
     provider accepted it. A revoked or expired key clears this check and fails on the \
     pane's first turn. The fleet does not spend a token to find out, so if a pane \
     reports an authentication error on turn one, this is why.";

/// **A custom provider and the variable it authenticates through** (#51).
///
/// The same two facts [`AccountShape::CustomProvider`] carries when the variable is
/// *there*, so the logged-in shape and the missing one are described in the same
/// words. It is a value rather than a formatted sentence for the reason
/// [`PostureDisagreement`] is: the gate's line and a test's assertion read the same
/// fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderKey {
    /// What the operator called the provider.
    pub provider: String,
    /// The environment variable it reads its key from.
    pub env_var: String,
}

/// **What the operator can do about a harness that will not take a seat** (#51).
///
/// The gate already says what is *wrong* — [`HarnessReadiness::refusal`] carries the
/// vendor's own sentence, and it is rendered verbatim. This is the other half, and
/// it is deliberately a different value: a reason and an instruction are not the
/// same sentence, and merging them would mean either respelling the vendor's words
/// or burying ours inside them.
///
/// **Three shapes, three answers, and the third is not "log in"** (C14, #36):
///
///  - no credential at all — [`command`](Self::command), the vendor's own login;
///  - a custom provider whose key is missing — [`variable`](Self::variable), because
///    no login command writes an environment variable;
///  - [`LoginState::Unreadable`] — neither, because it is a *working* installation
///    whose report this build could not parse, and telling that operator to log in
///    would be telling them to fix something that is not broken.
///
/// **It says nothing about what a passing check does not prove.** That is
/// [`REACHABILITY_NOT_AUTHORIZATION`], it is said once, and it is said about a
/// harness that *is* logged in — the opposite state to every one of these.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoginGuidance {
    /// The sentence that says which of the three shapes this is, and why the
    /// operator rather than FLEETOR is the one who has to act.
    pub sentence: String,
    /// What to type, from [`LoginInstruction::command`]. `None` when no command
    /// helps.
    pub command: Option<&'static str>,
    /// The second step, from [`LoginInstruction::then`], for a harness whose login
    /// lives inside what `command` starts.
    pub then: Option<&'static str>,
    /// The environment variable to set, for the shape no command fixes. `None`
    /// otherwise.
    pub variable: Option<String>,
}

impl LoginGuidance {
    /// The whole of it as one line, for a feed that has no card to lay it out on
    /// ([`HarnessReadiness::notices`]).
    ///
    /// The same three fields in the same order the gate renders them, so the
    /// Activity feed and the start card cannot end up telling an operator two
    /// different things about one machine.
    pub fn line(&self) -> String {
        let mut line = self.sentence.clone();
        if let Some(command) = self.command {
            line.push_str(&format!(" `{command}`"));
            if let Some(then) = self.then {
                line.push_str(&format!(" then `{then}`"));
            }
            line.push('.');
        }
        if let Some(variable) = &self.variable {
            line.push_str(&format!(" `{variable}`."));
        }
        line
    }
}

/// Which shape of credential a machine is logged in with (C14).
///
/// **Three variants because the vendor has three mechanisms, and each is named
/// individually rather than flattened to `true`.** An operator debugging a pane
/// that will not authenticate needs to know *which* credential the fleet is about
/// to spend: a plan, a stored key, and a provider of their own fail differently and
/// are fixed differently.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccountShape {
    /// A subscription plan the vendor stores tokens for.
    SubscriptionPlan {
        /// The plan's tier, where the vendor's diagnostic names one.
        ///
        /// **`None` on codex 0.153.4, and that is a measurement rather than a gap
        /// in this code** (#34). `doctor` reports `stored auth mode = chatgpt` and
        /// says nothing about the tier — the tier is inside the stored id token,
        /// and reading it would mean parsing the credential file this arc took
        /// pains to reach only through the vendor. C14 said the gate would show
        /// "ChatGPT &lt;plan&gt;"; what it can honestly show is the shape.
        plan: Option<String>,
    },
    /// A key the vendor stores or reads from the environment.
    ApiKey,
    /// A provider of the operator's own, by the name they gave it.
    CustomProvider {
        /// What the operator called it.
        name: String,
        /// The environment variable it authenticates through, where it names one.
        /// `None` for a provider carrying its credential inline, or one that needs
        /// none at all.
        env_var: Option<String>,
    },
}

impl AccountShape {
    /// What the gate puts beside the model — one short phrase, the shape named
    /// (C14).
    pub fn display(&self) -> String {
        match self {
            AccountShape::SubscriptionPlan { plan: Some(plan) } => {
                format!("subscription plan ({plan})")
            }
            AccountShape::SubscriptionPlan { plan: None } => "subscription plan".to_string(),
            AccountShape::ApiKey => "API key".to_string(),
            AccountShape::CustomProvider { name, env_var: Some(var) } => {
                format!("{name} (custom provider, via ${var})")
            }
            AccountShape::CustomProvider { name, env_var: None } => {
                format!("{name} (custom provider)")
            }
        }
    }
}

/// The containment a harness *resolved*, as opposed to the keys FLEETOR wrote into
/// its configuration (C8, checkpoint 3's `sandbox_keys`).
///
/// **The distinction is the whole point.** Placement writes `sandbox_mode` and
/// `approval_policy` into every codex pane's seed; nothing about having written
/// them proves the vendor read them, and a release that renames one produces a
/// fleet that is configured to be fenced and is not. This is the vendor's own
/// reading back.
///
/// Every field is `Option<String>` rather than an enum of postures, because these
/// are the vendor's words and a vendor that invents a fourth posture should show
/// the operator its name rather than be forced into the nearest of three.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResolvedPosture {
    /// What the vendor resolved for filesystem writes.
    pub filesystem: Option<String>,
    /// What it resolved for outbound network access.
    pub network: Option<String>,
    /// What it resolved for asking a human — the row that matters most to a fenced
    /// pane, because a worker has no human and a posture that asks one parks.
    pub approval: Option<String>,
    /// **The version the vendor stamped on the report the three rows above were read
    /// out of** (WP-25 #37).
    ///
    /// Here rather than beside [`HarnessReadiness::version`] because it is not a
    /// fact about the vendor, it is a fact about *this reading*: the same binary can
    /// change this the day it changes its report shape, and the three rows above
    /// mean whatever that shape says they mean. It is compared against
    /// [`Posture::verified_against_schema`], and a difference stops the fleet.
    ///
    /// `None` on a harness that stamps no version, and on every reading taken before
    /// the vendor answered at all.
    pub schema: Option<String>,
}

impl ResolvedPosture {
    /// The rows, in the order the operator should read them, skipping any the
    /// vendor did not report.
    pub fn rows(&self) -> Vec<(&'static str, &str)> {
        [
            ("filesystem", self.filesystem.as_deref()),
            ("network", self.network.as_deref()),
            ("approval", self.approval.as_deref()),
        ]
        .into_iter()
        .filter_map(|(name, value)| value.map(|v| (name, v)))
        .collect()
    }

    /// One row by the name [`Self::rows`] gives it, or `None`.
    ///
    /// **`None` has two meanings and they are the same refusal.** Either the vendor
    /// reported no such row — what a retired key looks like — or this build asked
    /// for a row name that does not exist, which is a [`PostureExpectation`] written
    /// against a shape [`Self`] no longer has. Both are "the reading this build
    /// wanted is not there", and both must stop a fleet rather than pass quietly.
    pub fn row(&self, name: &str) -> Option<&str> {
        self.rows().into_iter().find(|(row, _)| *row == name).map(|(_, value)| value)
    }
}

/// **One key the vendor did not resolve the way FLEETOR wrote it** (WP-25 #37).
///
/// **A value rather than a formatted string**, so the gate's sentence and a test's
/// assertion read the same fields. The one thing every field exists to serve is
/// acceptance criterion 4: the operator is told *which key* disagreed, because "the
/// posture is wrong" is not something anyone can act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostureDisagreement {
    /// The key that disagreed, as FLEETOR spells it in the configuration it
    /// writes — or `schemaVersion` for the report's own stamp, which FLEETOR reads
    /// rather than writes.
    pub key: &'static str,
    /// The row of [`ResolvedPosture`] it decides, for the operator who has to find
    /// it in the vendor's own output.
    pub row: &'static str,
    /// The value FLEETOR writes under that key. `None` for the schema stamp.
    pub wrote: Option<&'static str>,
    /// What this arc measured that value resolving to.
    pub expected: &'static str,
    /// What the vendor reported instead. **`None` is the loud case**: the row is not
    /// in the report at all, which is exactly what a retired key looks like.
    pub found: Option<String>,
}

impl PostureDisagreement {
    /// The operator's sentence — the key first, because that is the actionable half.
    pub fn sentence(&self) -> String {
        let found = match &self.found {
            Some(word) => format!("reports `{word}`"),
            None => "reports no such row at all".to_string(),
        };
        match self.wrote {
            Some(wrote) => format!(
                "`{}` disagrees: FLEETOR writes it as `{wrote}`, this build measured that \
                 resolving to `{}` in the `{}` row, and the vendor {found}. A pane would \
                 spawn under a containment nobody chose.",
                self.key, self.expected, self.row,
            ),
            None => format!(
                "`{}` disagrees: this build measured the containment rows against report \
                 schema `{}`, and the vendor {found}. They were read out of a document \
                 shape it no longer recognises, so it cannot say what a pane would spawn \
                 under.",
                self.key, self.expected,
            ),
        }
    }
}

impl HarnessReadiness {
    /// A harness whose binary this machine does not have.
    ///
    /// A real value describing a real machine rather than a placeholder — the same
    /// role [`Host::bare`](super::Host::bare) plays — so the "no codex installed"
    /// branch of the gate is a case a test can express.
    pub fn not_installed(harness: &'static HarnessSpec) -> Self {
        Self {
            harness,
            invoked: harness.program.bin.to_string(),
            resolved: None,
            readings_agree: false,
            version: None,
            login: LoginState::NotInstalled,
            provider: None,
            models: Vec::new(),
            posture: ResolvedPosture::default(),
        }
    }

    /// **Why the fleet will not start a seat on this harness, in the vendor's own
    /// words** — `None` when it will (WP-25 #36, story 11).
    ///
    /// The condition is [`LoginState::can_take_a_seat`] and nothing else, so a
    /// refusal and a greyed-out row can never disagree about the same machine. What
    /// this adds is the *sentence*, and it is the vendor's rather than ours wherever
    /// the vendor produced one: "no usable credential" is our word for the state,
    /// but the operator can only act on the line the vendor wrote.
    pub fn refusal(&self) -> Option<String> {
        match &self.login {
            LoginState::NoCredential { summary, .. } => Some(summary.clone()),
            LoginState::NotInstalled => Some(not_on_the_path(&self.invoked)),
            LoginState::LoggedIn(_) | LoginState::Unreadable { .. } => None,
        }
    }

    /// **What the operator can do about it, next to what is wrong** (#51).
    ///
    /// [`refusal`](Self::refusal) is the vendor's sentence about the state; this is
    /// the move out of it, and the two are separate values because they are separate
    /// sentences — "codex — not logged in" is correct and is a dead end, which is
    /// the whole of what this ticket is about.
    ///
    /// **The command is the spec's, never this file's.** It comes off
    /// [`HarnessSpec::login`], so a third harness registered tomorrow gets guidance
    /// on the day it is registered rather than on the day somebody remembers to
    /// widen a branch.
    ///
    /// `None` for a harness that is logged in — there is nothing to do — and for
    /// [`LoginState::NotInstalled`], where [`not_on_the_path`] already says the only
    /// actionable thing there is: the binary is absent from the `PATH` *this app*
    /// inherited, which a login command cannot change and an install may not either.
    pub fn login_guidance(&self) -> Option<LoginGuidance> {
        let name = self.harness.name;
        let login = &self.harness.login;
        match &self.login {
            LoginState::NoCredential { provider_key: Some(key), .. } => Some(LoginGuidance {
                sentence: format!(
                    "No login command will help here: this machine resolves {name} to {}, a \
                     provider of your own, and the key it names is not set. A named provider \
                     key has no ambient fallback, so nothing else is picked up instead. Set \
                     this in the environment FLEETOR is launched from:",
                    key.provider,
                ),
                command: None,
                then: None,
                variable: Some(key.env_var.clone()),
            }),
            LoginState::NoCredential { .. } => Some(LoginGuidance {
                sentence: format!(
                    "{name} keeps its credential in {}, and only its own login puts one \
                     there — FLEETOR writes no credential and asks you for none. In a \
                     terminal of your own, run:",
                    login.credential_home,
                ),
                command: Some(login.command),
                then: login.then,
                variable: None,
            }),
            LoginState::Unreadable { .. } => Some(LoginGuidance {
                sentence: format!(
                    "Nothing to log in to: this is a working {name} installation whose \
                     diagnostic this build could not read, not a logged-out one. A seat on \
                     it is still offered, and logging in again would change nothing.",
                ),
                command: None,
                then: None,
                variable: None,
            }),
            LoginState::NotInstalled | LoginState::LoggedIn(_) => None,
        }
    }

    /// **Which of the keys FLEETOR writes the vendor did not resolve the way it was
    /// written** — empty when they all agree (WP-25 #37; C8).
    ///
    /// **This is the posture tripwire, and it is the whole of it.** [`ResolvedPosture`]
    /// is what the vendor said it resolved; [`Posture::sandbox_keys`] and
    /// [`Outbound::reachability_keys`] are what FLEETOR wrote; and
    /// [`Posture::verified_as`] is the measured bridge between them. Every key with
    /// a row here is read back, and a disagreement stops the fleet at the gate
    /// through [`crate::fleet`]'s `StartVerdict` — before any pane spawns, which is
    /// the only moment at which a mute fleet is still a sentence instead of five
    /// panes that answer nothing.
    ///
    /// **The schema is checked first and short-circuits.** Once the report shape has
    /// changed, the three rows are readings out of a different document; reporting
    /// three key disagreements as well would bury the one fact that explains all of
    /// them. So a schema change produces exactly one disagreement, naming
    /// `schemaVersion`.
    ///
    /// **It runs only on a machine that is logged in, and that is C14's narrowness
    /// rather than an omission.** `NotInstalled` and `NoCredential` already refuse
    /// the seat and have no posture to compare — adding a second sentence about a
    /// fleet that is already stopped tells the operator nothing. `Unreadable` is a
    /// *working* installation whose report this build could not parse, and refusing
    /// it here would stop a fleet on a parse error, which is precisely the failure
    /// [`LoginState::Unreadable`] exists to avoid. The state this defends is the one
    /// that otherwise passes silently: a vendor that answers, authenticates, and
    /// quietly ignores a key it used to honour.
    ///
    /// Empty for a harness with no [`Posture::verified_as`] rows, which is not a
    /// pass — it is the honest "nothing to compare" of a harness that publishes no
    /// resolved posture at all.
    pub fn posture_disagreements(&self) -> Vec<PostureDisagreement> {
        if !matches!(self.login, LoginState::LoggedIn(_)) {
            return Vec::new();
        }
        if let Some(understood) = self.harness.posture.verified_against_schema {
            if self.posture.schema.as_deref() != Some(understood) {
                return vec![PostureDisagreement {
                    key: "schemaVersion",
                    row: "report schema",
                    wrote: None,
                    expected: understood,
                    found: self.posture.schema.clone(),
                }];
            }
        }
        self.harness
            .posture
            .verified_as
            .iter()
            .filter_map(|expectation| {
                let found = self.posture.row(expectation.row).map(str::to_string);
                (found.as_deref() != Some(expectation.resolved)).then(|| PostureDisagreement {
                    key: expectation.written_key,
                    row: expectation.row,
                    wrote: self.written_value(expectation.written_key),
                    expected: expectation.resolved,
                    found,
                })
            })
            .collect()
    }

    /// The value FLEETOR writes under one containment key, read out of the spec
    /// rather than respelled (C36's two lists, in the order the seeder reads them).
    ///
    /// `None` is impossible for any key `tests/gate_posture_tripwire.rs` allows, and
    /// is rendered by [`PostureDisagreement::sentence`] as the schema case rather
    /// than being unwrapped — a tripwire that panicked on its own bookkeeping would
    /// be worse than the failure it guards.
    fn written_value(&self, key: &str) -> Option<&'static str> {
        self.harness
            .posture
            .sandbox_keys
            .iter()
            .chain(self.harness.outbound.reachability_keys)
            .find(|(written, _)| *written == key)
            .map(|(_, value)| *value)
    }

    /// **What the operator is told, for the Activity feed** (WP-25 #34).
    ///
    /// Returned rather than emitted, which is the shape everything operator-facing
    /// in `placement` already uses (see the module header): this module holds no
    /// store, and a returned line is one a test can assert on.
    ///
    /// The caveat is a [`NoticeLevel::Warn`] on a machine that is *logged in*, and
    /// that inversion is deliberate. It is not a warning that something is wrong —
    /// it is a warning that a green check is narrower than it looks, and the moment
    /// it is useful is exactly the moment everything appears fine.
    pub fn notices(&self) -> Vec<(NoticeLevel, String)> {
        let name = self.harness.name;
        let mut notices = Vec::new();
        match &self.login {
            LoginState::NotInstalled => {
                notices.push((
                    NoticeLevel::Info,
                    format!(
                        "{name} is not installed on this machine — `{}` is not on the \
                         PATH the app was launched with, so no seat can run it.",
                        self.invoked,
                    ),
                ));
                return notices;
            }
            LoginState::Unreadable { why } => {
                notices.push((
                    NoticeLevel::Warn,
                    format!(
                        "{name} is installed and its diagnostic could not be read: {why}. \
                         The gate cannot say whether a seat on it would authenticate.",
                    ),
                ));
                return notices;
            }
            LoginState::NoCredential { summary, .. } => {
                // **The move out, from the spec rather than guessed from the binary
                // name** (#51). This line used to read "log in with `claude`", which
                // is not a login command for either registered harness — the
                // program name is checkpoint 1 and the login is
                // `HarnessSpec::login`, and they are only the same string by
                // coincidence.
                let guidance =
                    self.login_guidance().map(|g| format!(" {}", g.line())).unwrap_or_default();
                notices.push((
                    NoticeLevel::Warn,
                    format!("{name} has no usable credential on this machine: {summary}.{guidance}"),
                ));
                return notices;
            }
            LoginState::LoggedIn(shape) => {
                let version = self.version.as_deref().unwrap_or("an unreported version");
                let provider = match &self.provider {
                    Some(p) => format!(", provider {p}"),
                    None => String::new(),
                };
                notices.push((
                    NoticeLevel::Info,
                    format!(
                        "{name} {version} is logged in: {}{provider}. {} model{} offered.",
                        shape.display(),
                        self.models.len(),
                        if self.models.len() == 1 { "" } else { "s" },
                    ),
                ));
            }
        }

        let posture = self.posture.rows();
        if !posture.is_empty() {
            notices.push((
                NoticeLevel::Info,
                format!(
                    "{name} resolved its containment as {} — read back from the vendor, \
                     not from the keys FLEETOR wrote.",
                    posture
                        .iter()
                        .map(|(k, v)| format!("{k} {v}"))
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
            ));
        }

        if let Some(resolved) = &self.resolved {
            if !self.readings_agree {
                notices.push((
                    NoticeLevel::Warn,
                    format!(
                        "the `{}` on your PATH is a wrapper: it and the vendor binary at {} \
                         report different configurations. What a pane runs under is the \
                         second one, so the first is not what the gate above describes.",
                        self.invoked,
                        resolved.display(),
                    ),
                ));
            }
        }

        if let Some(caveat) = self.login.caveat() {
            notices.push((NoticeLevel::Warn, format!("{name}: {caveat}")));
        }
        notices
    }
}

// --- what Claude Code reports about this machine (WP-25 #35; M17) --------------

/// **Read what Claude Code reports about this machine** (WP-25 #35; M17, C14, C47).
///
/// **C58's named second push into [`Host::harnesses`](super::Host::harnesses)**, built
/// here because the gate is what needed it: C23 puts Claude Code on the orchestrator
/// row and the workers row beside codex, and a harness whose login the gate cannot
/// speak to sits next to one whose it can, looking half-built. That is story 9's *a
/// supported feature looks unimplemented*, at the one screen where the operator is
/// deciding what to spend.
///
/// **`home` and `path` arrive rather than being read**, which is the whole reason
/// this takes two arguments instead of none. It lives inside `placement`,
/// `tests/placement_reads_nothing.rs` holds the module to its one rule, and both
/// facts it needs — the operator's `HOME` and the `PATH` the application was
/// launched with — are already fields on [`Host`](super::Host). So it reads the same
/// values a spawn resolves against, and a test can hand it a scratch directory.
///
/// **The login check is `~/.claude.json`'s `oauthAccount`, and M17 recorded its
/// caveat when it chose it:** that file is Claude Code's internal state rather than a
/// supported API, and its presence proves a login *happened*, not that the token
/// still works. So a pass here carries the same [`REACHABILITY_NOT_AUTHORIZATION`]
/// sentence codex's does — two harnesses failing this way for different reasons, and
/// the operator told the same thing about both.
///
/// **What it is bounded by, said rather than hidden:** an operator authenticating
/// through `ANTHROPIC_API_KEY` in their shell has no `oauthAccount` and is reported
/// as having no credential. Reading that variable would be a process read inside
/// `placement`, and putting it on [`Host`](super::Host) to dodge that would put the
/// operator's own key on a value the whole spawn path holds. The refusal names the
/// bound so an operator in that state can read what happened rather than guess.
///
/// **Both readings, per C47.** `claude` is a name people alias, and this is a
/// measurement taken through a subprocess — the class where #31 found the two
/// readings disagreeing absolutely. So the version is read once through the bare name
/// and once through the absolute file the `PATH` resolved to, and whether they agreed
/// is [`HarnessReadiness::readings_agree`]. Zero tokens: `--version` asks for no
/// completion.
///
/// **No model list, and that is an answer rather than a gap.** Claude Code publishes
/// no catalog command, so [`HarnessReadiness::models`] is empty and the gate offers
/// the operator a name to type — exactly what that field's own documentation says an
/// empty list means.
pub fn claude_code_diagnose(home: Option<&Path>, path: &str) -> HarnessReadiness {
    let spec = &CLAUDE_CODE_SPEC;
    let invoked = spec.program.bin;
    let Some(resolved) = first_on_path(path, invoked) else {
        return HarnessReadiness::not_installed(spec);
    };

    let through_the_name = vendor_version(Path::new(invoked));
    let through_the_binary = vendor_version(&resolved);

    HarnessReadiness {
        harness: spec,
        invoked: invoked.to_string(),
        resolved: Some(resolved),
        readings_agree: through_the_name == through_the_binary,
        version: through_the_binary,
        login: claude_code_login(home),
        // C2 as amended by C9: the orchestrator inherits its provider and the gate
        // displays it. Claude Code reports none of its own here — an operator
        // pointing `ANTHROPIC_BASE_URL` somewhere is doing it in their shell, which
        // is the process this module may not read.
        provider: None,
        // **The vendor publishes no catalog, so the spec's is what the gate
        // offers** (C78). This is still not an invented reading: the aliases are
        // the vendor's own, quoted from its `--help`, and the field they land in
        // is the same one codex fills from a live `models.json`. What a caller
        // cannot do is tell the two apart — which is correct, because the gate's
        // job is to offer models and not to explain where the list came from.
        models: CLAUDE_CODE_SPEC
            .posture
            .declared_models
            .iter()
            .map(|(slug, display_name)| ModelChoice {
                slug: (*slug).to_string(),
                display_name: (*display_name).to_string(),
            })
            .collect(),
        // #37's, not this ticket's: comparing what the vendor resolved against what
        // FLEETOR wrote is a gate check of its own, and Claude Code publishes no
        // diagnostic that reports one.
        posture: ResolvedPosture::default(),
    }
}

/// Whether this machine has completed a `claude` login, by the one file M17 named.
fn claude_code_login(home: Option<&Path>) -> LoginState {
    let Some(home) = home else {
        return LoginState::Unreadable {
            why: "this machine reported no HOME, so `~/.claude.json` could not be located"
                .to_string(),
        };
    };
    let file = home.join(".claude.json");
    let text = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return LoginState::NoCredential {
                summary: no_login_recorded(&file),
                // Claude Code publishes no custom-provider reading at all, so this
                // shape cannot arise for it — an honest `None` rather than an
                // invented one (#51).
                provider_key: None,
            }
        }
        Err(e) => {
            let why = format!("{} could not be read: {e}", file.display());
            return LoginState::Unreadable { why };
        }
    };
    // **Not a refusal**, for the reason `LoginState::Unreadable` exists: a working
    // installation whose internal state file this arc cannot parse is a parse error,
    // and treating that as "logged out" would refuse a seat on the strength of one.
    let value: serde_json::Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(e) => {
            let why = format!("{} is not readable as JSON: {e}", file.display());
            return LoginState::Unreadable { why };
        }
    };
    match value.get("oauthAccount") {
        Some(account) if !account.is_null() => {
            LoginState::LoggedIn(AccountShape::SubscriptionPlan { plan: None })
        }
        _ => LoginState::NoCredential { summary: no_login_recorded(&file), provider_key: None },
    }
}

/// The one sentence both no-credential branches say, so they cannot drift apart.
fn no_login_recorded(file: &Path) -> String {
    format!(
        "{} records no completed `claude` login (an `ANTHROPIC_API_KEY` in your shell \
         is not written there and is not read here)",
        file.display(),
    )
}

/// The first `bin` on `path` that is a file. A plain search of a string the caller
/// handed over — nothing here asks the process what its `PATH` is.
fn first_on_path(path: &str, bin: &str) -> Option<PathBuf> {
    std::env::split_paths(path).map(|dir| dir.join(bin)).find(|found| found.is_file())
}

/// What a vendor says its version is, or `None` when it would not answer.
fn vendor_version(bin: &Path) -> Option<String> {
    let out = std::process::Command::new(bin).arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

// --- pinning the two forms together -------------------------------------------

/// **What the expand step's safety net became once there was no second form
/// left** (#23).
///
/// These began as pins holding [`CLAUDE_CODE_SPEC`] against the literals it was
/// written beside. The literals are gone, so what each one now pins is the *value*
/// — the paste framing, the argv shape, the allowlist coverage, the two live
/// constants the spec names rather than copies. That is still worth asserting and
/// it is cheap, so they stay rather than being retired: a value that changes here
/// changes what a pane runs.
///
/// They are not the conformance suite. That is `tests/harness_conformance_1_7.rs`
/// and its sibling — one pass per registered harness, every checkpoint observed
/// end to end through [`place`](crate::placement::place). These are narrower and
/// run in-crate.
#[cfg(test)]
mod tests {
    use super::*;

    /// **The registry pin, now holding two** (C20, C25, C53, C54, C57).
    ///
    /// The count held at one from #14 through #39 — phase 1's exit condition and
    /// phase 2's standing invariant — because a suite with one registered harness
    /// is the point rather than a limitation: a suite built against a second
    /// vendor while that vendor is the only thing describing it is a description
    /// of that vendor wearing an abstraction's clothes. #33 registered codex,
    /// found six checkpoints shaped around Claude Code, reshaped them, and
    /// reverted the flip because checkpoint 13 correctly refused a harness whose
    /// transcript is a live database. #39 landed that mechanism and observed all
    /// fourteen green over both harnesses; this is the flip it earned.
    ///
    /// **The count is no longer the assertion — the properties are.** Every entry
    /// is a distinct, nameable harness that [`by_name`] resolves back to the
    /// identical spec, and Claude Code is still among them. A registry that swapped
    /// one sole harness for another would be a description again, which is why
    /// both names are asserted rather than only the new one.
    /// **Every registered harness supplies its own mark, and no two share one**
    /// (#50, closing C56).
    ///
    /// The rail tells a mixed fleet apart by this glyph, so a harness that supplied
    /// none would be a pane with no mark and a harness that shared one would be two
    /// panes wearing the same. Neither is a compile error and neither shows up as
    /// anything but a rail that reads oddly.
    ///
    /// **A property over the registry, not a list of the two.** A third harness
    /// arrives with this rule already applied to it, which is what makes "no `ui/`
    /// change" true: the interface renders whatever comes off the wire, and this is
    /// what says something will.
    #[test]
    fn every_registered_harness_supplies_its_own_mark() {
        let all = registered();
        for entry in all {
            let spec = entry.spec();
            assert!(
                !spec.mark.is_empty(),
                "`{}` supplies no mark. The rail has nothing to render and no way to \
                 derive one — deriving it would be a branch on the name (#50).",
                spec.name,
            );
            let width = spec.mark.chars().count();
            assert!(
                width <= 2,
                "`{}`'s mark is {width} characters (`{}`). It sits in a tab beside a \
                 status word and a gauge; a mark that has to be read is the reading \
                 this ticket removed.",
                spec.name,
                spec.mark,
            );
        }

        let mut marks: Vec<&str> = all.iter().map(|h| h.spec().mark).collect();
        marks.sort_unstable();
        let unique = {
            let mut u = marks.clone();
            u.dedup();
            u
        };
        assert_eq!(
            marks, unique,
            "two harnesses share a mark, so two panes running different vendors are \
             indistinguishable in exactly the place the mark exists to distinguish them",
        );
    }

    /// **Every registered harness states how an operator logs it in** (#51).
    ///
    /// The property that makes "a third harness needs no `ui/` change" true on the
    /// backend side, exactly as the mark's does above: the card renders whatever
    /// arrives on `HarnessOffer::guidance`, and this is what says something will
    /// arrive. A harness registered without an answer here would render a refusal
    /// with an empty instruction under it, which is the dead end this ticket
    /// closed with different words in it.
    ///
    /// **`'static` and nothing else.** The command is a fact a harness knows about
    /// itself; one that could only be discovered by running the vendor would be a
    /// process read inside `placement` and would belong on `Host`.
    #[test]
    fn every_registered_harness_states_how_to_log_it_in() {
        for entry in registered() {
            let spec = entry.spec();
            let login = &spec.login;
            assert!(
                !login.command.trim().is_empty(),
                "`{}` declares no login command, so the gate can say what is wrong with it \
                 and not what to do about it — the state #51 exists to end.",
                spec.name,
            );
            assert!(
                login.then.is_none_or(|then| !then.trim().is_empty()),
                "`{}` declares an empty second step. Absent means one step; empty means a \
                 row telling an operator to type nothing.",
                spec.name,
            );
            assert!(
                !login.credential_home.trim().is_empty(),
                "`{}` does not say where its credential lands. That sentence is the measured \
                 reason FLEETOR asks for no token (#49), and without it the instruction is \
                 an order rather than an explanation.",
                spec.name,
            );
        }
    }

    /// **The three not-usable shapes get three different answers** (#51; C14, #36,
    /// #29).
    ///
    /// The one place the distinction is decided. Every one of these is a machine
    /// the gate has something to say about, and saying the same thing about all
    /// three would be worse than saying nothing: two of them are not fixed by
    /// logging in.
    #[test]
    fn the_three_not_usable_shapes_are_told_three_different_things() {
        let spec = &CLAUDE_CODE_SPEC;
        let readiness = |login: LoginState| HarnessReadiness {
            login,
            ..HarnessReadiness::not_installed(spec)
        };

        // 1 — no credential at all: the vendor's own login command, with whatever
        // second step the spec declared.
        let none = readiness(LoginState::NoCredential {
            summary: "no login recorded".into(),
            provider_key: None,
        })
        .login_guidance()
        .expect("a machine with no credential is told what to run");
        assert_eq!(none.command, Some(spec.login.command));
        assert_eq!(none.then, spec.login.then);
        assert_eq!(none.variable, None, "there is no variable to set in this shape");
        assert!(
            none.sentence.contains(spec.login.credential_home),
            "the operator is told to run something and not why only they can: {}",
            none.sentence,
        );

        // 2 — a custom provider whose key is missing: no command helps, and the
        // variable is the whole of the fix (#29 — no ambient fallback).
        let missing = readiness(LoginState::NoCredential {
            summary: "the configured provider's key is missing".into(),
            provider_key: Some(ProviderKey {
                provider: "the operator's own".into(),
                env_var: "OPERATORS_SHELL_KEY".into(),
            }),
        })
        .login_guidance()
        .expect("a provider missing its key is told about the variable");
        assert_eq!(
            missing.command, None,
            "a login command was offered for a state no login command fixes",
        );
        assert_eq!(missing.variable.as_deref(), Some("OPERATORS_SHELL_KEY"));
        assert!(
            missing.sentence.contains("No login command will help"),
            "the sentence does not say that this is the shape logging in does not fix: {}",
            missing.sentence,
        );

        // 3 — `Unreadable`: a working installation, and telling this operator to
        // log in would be telling them to fix something that is not broken.
        let unreadable = readiness(LoginState::Unreadable { why: "an unknown format".into() })
            .login_guidance()
            .expect("an unreadable report is told what it is");
        assert_eq!(unreadable.command, None);
        assert_eq!(unreadable.variable, None);
        assert!(
            unreadable.sentence.contains("Nothing to log in to"),
            "an operator with a working installation is being told to log in: {}",
            unreadable.sentence,
        );

        // The two states with nothing to say. `NotInstalled`'s reason already
        // carries the only actionable sentence there is, and a logged-in harness
        // has nothing to fix.
        assert_eq!(readiness(LoginState::NotInstalled).login_guidance(), None);
        assert_eq!(
            readiness(LoginState::LoggedIn(AccountShape::ApiKey)).login_guidance(),
            None,
        );

        // **And none of the three respells the caveat.** That sentence is about a
        // machine that *is* logged in — the opposite state to all three — and it is
        // said once, from `REACHABILITY_NOT_AUTHORIZATION`.
        for guidance in [&none, &missing, &unreadable] {
            assert!(
                !guidance.sentence.contains("revoked or expired key clears this check"),
                "a guidance sentence respells the reachability caveat: {}",
                guidance.sentence,
            );
        }
    }

    #[test]
    fn the_registry_holds_two_distinct_harnesses_and_names_resolve() {
        let all = registered();
        assert_eq!(all.len(), 2, "codex joined at #39's follow-up; a third is its own arc");
        assert!(
            all.iter().any(|h| h.spec().name == "claude-code"),
            "Claude Code stays — a registry that swapped its sole harness for another \
             would be a description again rather than a suite",
        );
        assert!(
            all.iter().any(|h| h.spec().name == "codex"),
            "codex is registered; a ticket that drops it back out fails here rather than \
             quietly shrinking the suite to one pass",
        );

        let mut names: Vec<&str> = all.iter().map(|h| h.spec().name).collect();
        names.sort_unstable();
        let unique = {
            let mut u = names.clone();
            u.dedup();
            u
        };
        assert_eq!(names, unique, "two entries share a name, so `by_name` answers one of them");

        for harness in all {
            let name = harness.spec().name;
            let found = by_name(name).unwrap_or_else(|| panic!("{name} is registered and unfindable"));
            assert!(
                std::ptr::eq(found.spec(), harness.spec()),
                "{name} resolves by name to a different spec than the registry holds",
            );
            assert!(!name.trim().is_empty(), "a harness with no name cannot be written down");
        }

        assert_eq!(claude_code().spec(), &CLAUDE_CODE_SPEC);
        assert!(by_name("nothing-registered-under-this-name").is_none());
    }

    #[test]
    fn the_program_and_the_argv_match_what_the_command_builders_already_produce() {
        // Checkpoints 1, 2 and 3 against `spawn`'s three shapes: `orch` gets the
        // brief alone, every unattended seat gets the permission flag first.
        let cc = claude_code();
        assert_eq!(cc.spec().program.bin, "claude");
        assert_eq!(
            cc.command_args(&Seat::new("BRIEF").for_the_operator()),
            vec!["--system-prompt", "BRIEF"]
        );
        assert_eq!(
            cc.command_args(&Seat::new("BRIEF").with_permission_mode("auto")),
            vec!["--permission-mode", "auto", "--system-prompt", "BRIEF"]
        );
        // Checkpoint 3's second channel, and the reason it now exists: this
        // harness names its model in the environment, so a seat that carries one
        // adds nothing to argv. A harness whose `model_flag` is set gets the flag
        // from the same call — see codex's own test.
        assert_eq!(
            cc.command_args(&Seat::new("BRIEF").with_model("a-model").with_permission_mode("auto")),
            vec!["--permission-mode", "auto", "--system-prompt", "BRIEF"],
        );
    }

    #[test]
    fn the_command_channel_covers_the_allowlist_and_nothing_beyond_it() {
        // Widening `ALLOWED_COMMANDS` is Tier 2; widening it without a spelling
        // here would send a pane a command the harness may not have.
        let spellings = claude_code().spec().commands.spellings;
        let canonical: Vec<&str> = spellings.iter().map(|(c, _)| *c).collect();
        assert_eq!(canonical, fleetor_core::command::ALLOWED_COMMANDS.to_vec());
    }

    #[test]
    fn the_gauge_window_and_the_guardrail_hook_are_the_live_constants() {
        // Not a copy of them — the same items. This test says so out loud so that
        // a later edit which replaces either with a literal is caught here.
        let spec = claude_code().spec();
        assert_eq!(spec.gauge.window_tokens, Some(crate::context_gauge::WORKER_WINDOW_TOKENS));
        assert_eq!(spec.guardrail.hook_file, guardrail::HOOK_FILE);
    }

    #[test]
    fn the_typing_profile_matches_the_bytes_the_pty_actually_writes() {
        // Checkpoint 9. The paste framing and the submit byte are pinned; the
        // 30 ms gap is the only delay on the message path (D-034, Tier 1.4) and
        // this is the value that must stay one number.
        let typing = &claude_code().spec().typing;
        assert!(typing.bracketed_paste);
        assert_eq!(typing.paste_start, b"\x1b[200~");
        assert_eq!(typing.paste_end, b"\x1b[201~");
        assert_eq!(typing.submit_bytes, b"\r");
        assert_eq!(typing.submit_gap_ms, 30);
    }

    #[test]
    fn the_project_key_is_the_canonicalization_the_trust_flag_and_the_gauge_share() {
        // Checkpoint 14's two halves agree: the method is `spawn::project_key`,
        // and the spec says that is what it is.
        let dir = std::env::temp_dir();
        assert_eq!(claude_code().project_key(&dir), super::super::spawn::project_key(&dir));
        assert!(claude_code().spec().project_identity.canonicalize);
    }

    #[test]
    fn seeding_through_the_harness_writes_the_keys_the_spec_names() {
        // Checkpoints 4 and 14 end to end, against a scratch directory — the same
        // property `placement`'s own rule buys: every filesystem effect is
        // confined to the directory it was handed.
        let scratch = std::env::temp_dir().join(format!(
            "fleetor-harness-seed-{}",
            fleetor_core::time::now_ms()
        ));
        let cwd = scratch.join("work");
        std::fs::create_dir_all(&cwd).expect("scratch cwd");
        let config = scratch.join("config");

        let cc = claude_code();
        cc.seed_config_dir(&Seed::new(&config, &cwd, None)).expect("seed the config dir");

        let spec = cc.spec();
        let text = std::fs::read_to_string(config.join(spec.config_dir.seed_file))
            .expect("the seed file the spec names");
        let root: serde_json::Value = serde_json::from_str(&text).expect("valid json");
        for key in spec.config_dir.seed_keys {
            assert_eq!(root[key], serde_json::json!(true), "config key {key}");
        }
        let project = &root["projects"][cc.project_key(&cwd)];
        for key in spec.project_identity.trust_keys {
            assert_eq!(project[key], serde_json::json!(true), "trust key {key}");
        }

        std::fs::remove_dir_all(&scratch).ok();
    }

    // --- what a harness reports about this machine (WP-25 #34) ------------------

    /// A machine that is logged in, described as a value rather than arranged for.
    ///
    /// **This is the answer to "how does a test supply this without touching the
    /// operator's installation".** Every field is public and owned, so the state
    /// the gate has to render is written down here instead of being produced by
    /// logging somebody in — the same property [`Host::bare`](super::Host::bare)
    /// gives the machine one level up.
    fn logged_in() -> HarnessReadiness {
        HarnessReadiness {
            harness: crate::placement::codex::codex().spec(),
            invoked: "codex".to_string(),
            resolved: Some(PathBuf::from("/opt/vendor/bin/codex")),
            readings_agree: true,
            version: Some("0.153.4".to_string()),
            login: LoginState::LoggedIn(AccountShape::ApiKey),
            provider: Some("loopback".to_string()),
            models: vec![ModelChoice {
                slug: "gpt-6-astra".to_string(),
                display_name: "GPT-6-Astra".to_string(),
            }],
            posture: ResolvedPosture {
                filesystem: Some("restricted".to_string()),
                network: Some("restricted".to_string()),
                approval: Some("never".to_string()),
                schema: None,
            },
        }
    }

    /// **The refusal and the greyed-out row are one rule read from two ends**
    /// (WP-25 #36, story 11).
    ///
    /// [`LoginState::can_take_a_seat`] decides whether a row is offered;
    /// [`HarnessReadiness::refusal`] decides whether a start is stopped, and
    /// supplies the sentence. If those two ever disagree the gate greys out a
    /// harness the fleet would happily start, or starts one the gate greyed out —
    /// and neither fails a compile. So they are asserted to be the same predicate,
    /// over all four states.
    #[test]
    fn what_may_not_take_a_seat_is_exactly_what_refuses_a_start() {
        let states = [
            LoginState::LoggedIn(AccountShape::ApiKey),
            LoginState::NoCredential { summary: "no Codex credentials".into(), provider_key: None },
            LoginState::NotInstalled,
            LoginState::Unreadable { why: "unknown report format".into() },
        ];
        for login in states {
            let reading = HarnessReadiness { login: login.clone(), ..logged_in() };
            assert_eq!(
                reading.login.can_take_a_seat(),
                reading.refusal().is_none(),
                "a seat that may be taken is a start that is not refused: {login:?}",
            );
        }

        // The two that refuse carry the vendor's own words, and the not-installed
        // sentence is the one `HarnessOffer::from` puts on the row (`not_on_the_path`).
        let refused = HarnessReadiness {
            login: LoginState::NoCredential { summary: "no Codex credentials".into(), provider_key: None },
            ..logged_in()
        };
        assert_eq!(refused.refusal().as_deref(), Some("no Codex credentials"));
        let absent = HarnessReadiness::not_installed(crate::placement::codex::codex().spec());
        assert_eq!(absent.refusal(), Some(not_on_the_path("codex")));
    }

    /// **The reachability-not-authorization caveat reaches the operator**, on the
    /// one machine state where it can mislead them (#34; C14, M17).
    ///
    /// This is the acceptance criterion that is easiest to satisfy dishonestly: a
    /// caveat recorded in `decisions.md` and nowhere else is a caveat the operator
    /// never sees, and the failure it describes — a revoked key clearing the gate
    /// and failing on turn one — looks like a FLEETOR bug from the outside. So the
    /// assertion is that the sentence is *on the feed*, that it says what the check
    /// did not prove, and that it does not appear on a machine with nothing to be
    /// wrong about.
    #[test]
    fn the_reachability_caveat_is_on_the_feed_whenever_the_gate_says_logged_in() {
        let lines = logged_in().notices();
        let caveat = lines
            .iter()
            .find(|(_, text)| text.contains(REACHABILITY_NOT_AUTHORIZATION))
            .unwrap_or_else(|| panic!("the caveat never reached the operator: {lines:#?}"));
        assert_eq!(caveat.0, NoticeLevel::Warn, "a green check that is narrower than it looks");
        assert!(caveat.1.starts_with("codex:"), "it names which harness: {}", caveat.1);
        assert!(
            REACHABILITY_NOT_AUTHORIZATION.contains("first turn"),
            "the operator needs when it bites, not only that it is imprecise",
        );
        assert!(
            REACHABILITY_NOT_AUTHORIZATION.contains("does not spend a token"),
            "and why the fleet did not simply check — the rejected live probe (C14)",
        );

        // Not on a machine with no credential: there is no green check to qualify,
        // and a caveat beside a refusal reads as a second problem.
        let refused = HarnessReadiness {
            login: LoginState::NoCredential { summary: "no Codex credentials".to_string(), provider_key: None },
            ..logged_in()
        };
        assert!(
            !refused.notices().iter().any(|(_, t)| t.contains(REACHABILITY_NOT_AUTHORIZATION)),
            "{:#?}",
            refused.notices(),
        );
    }

    /// **The gate names the shape, the provider and the posture it read back.**
    ///
    /// The posture line is checkpoint 3's read *back*: placement writes
    /// `sandbox_mode` and `approval_policy` into every codex pane, and nothing
    /// about having written them proves the vendor resolved them. C8's whole point
    /// is that the gate shows the second thing.
    #[test]
    fn the_gate_says_the_shape_the_provider_and_the_posture_the_vendor_resolved() {
        let lines = logged_in().notices();
        let text = lines.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join("\n");
        assert!(text.contains("API key"), "the shape is named individually (C14): {text}");
        assert!(text.contains("loopback"), "the provider is displayed beside it (C2): {text}");
        assert!(text.contains("0.153.4"), "which build answered: {text}");
        assert!(text.contains("1 model offered"), "what a picker will have: {text}");
        assert!(
            text.contains("filesystem restricted, network restricted, approval never"),
            "the resolved containment, all three rows: {text}",
        );
        assert!(
            text.contains("not from the keys FLEETOR wrote"),
            "and that it is a read-back rather than a restatement of the seed: {text}",
        );
    }

    /// **A wrapper on the PATH is the operator's problem to know about** (C47).
    ///
    /// The `codex` on the machine this was written on is a shim that injects flags,
    /// and #31 found the class where that changes a verdict absolutely. When the
    /// two readings disagree, the gate is describing a configuration no pane will
    /// run under — which is unactionable unless the operator is told.
    #[test]
    fn two_readings_that_disagree_are_a_warning_and_two_that_agree_are_silent() {
        assert!(
            !logged_in().notices().iter().any(|(_, t)| t.contains("is a wrapper")),
            "agreeing readings say nothing about the wrapper",
        );
        let disagreeing = HarnessReadiness { readings_agree: false, ..logged_in() };
        let warned = disagreeing
            .notices()
            .into_iter()
            .find(|(_, t)| t.contains("is a wrapper"))
            .expect("a disagreement the operator is not told about is C47 unenforced");
        assert_eq!(warned.0, NoticeLevel::Warn);
        assert!(warned.1.contains("/opt/vendor/bin/codex"), "it names both readings: {}", warned.1);
    }

    /// The three shapes' sentences, pinned — they are what an operator debugging a
    /// pane that will not authenticate reads to know which credential is at stake.
    #[test]
    fn each_auth_shape_has_its_own_sentence() {
        assert_eq!(AccountShape::ApiKey.display(), "API key");
        assert_eq!(
            AccountShape::SubscriptionPlan { plan: None }.display(),
            "subscription plan",
            "0.153.4's `doctor` reports the shape and not the tier — see the field",
        );
        assert_eq!(
            AccountShape::SubscriptionPlan { plan: Some("pro".into()) }.display(),
            "subscription plan (pro)",
            "and a release that starts naming one needs no code change here",
        );
        assert_eq!(
            AccountShape::CustomProvider { name: "mine".into(), env_var: None }.display(),
            "mine (custom provider)",
        );
    }
}
