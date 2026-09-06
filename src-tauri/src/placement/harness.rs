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

use std::path::Path;

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
/// [`credentials_follow_home`](Self::credentials_follow_home) is the field that
/// says so.
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
    /// Whether this harness's login is reachable only through `HOME`. `false`
    /// here, and it is the fact C17 renamed this checkpoint over: a fenced Claude
    /// Code pane keeps a private `HOME` and still authenticates, because its
    /// credential arrives through [`Credentials::token_env`] and its keychain
    /// entry through [`credential_env`](Self::credential_env).
    pub credentials_follow_home: bool,
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
/// The installer itself is [`crate::guardrail::install`] and does not move: the
/// script, the interpreter and the journal are the fleet's, identical for every
/// harness. What varies is the settings file, the event name and the tool
/// matcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardrailInstall {
    /// The settings file inside the config dir the hook is registered in.
    /// Claude Code: `settings.json`.
    pub settings_file: &'static str,
    /// The hook event fired before a tool call, which is the only place a refusal
    /// can be a refusal rather than a report. Claude Code: `PreToolUse`.
    pub hook_event: &'static str,
    /// The tools the hook is matched against.
    pub tool_matcher: &'static str,
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
    /// Whether usage is read out of the pane's own transcript (checkpoint 13)
    /// rather than from a separate store.
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
    /// The directory under the pane's config dir that holds them, per project.
    pub subdir: &'static str,
    /// The extension the harvest looks for.
    pub file_ext: &'static str,
    /// What `manifest.json` records as this pane's `transcript_format`, so a
    /// Critic reading a mixed run cold knows what it is holding (M24).
    pub format: &'static str,
    /// Whether a plain filesystem move is a safe way to take it. `true` for
    /// append-only files; a harness whose transcript is a live database needs a
    /// mechanism of its own, and a file copy of one is a torn copy.
    pub file_move_is_safe: bool,
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
#[derive(Debug, Clone, Copy)]
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
    /// The pane's own configuration directory. **Every byte this call writes lands
    /// under here**, and a harness that writes anywhere else is the bug this field
    /// exists to make obvious.
    pub config_dir: &'a Path,
    /// The pane's working directory, which checkpoint 14's trust record is keyed
    /// by through [`Harness::project_key`].
    pub cwd: &'a Path,
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
        Self { operators_own_seat: false, config_dir, cwd, operator_home, brief: "" }
    }

    /// The same seed, for the one pane the operator sits at (#28, C21).
    ///
    /// Said affirmatively and in exactly one place, so the default stays the
    /// narrow answer: a harness that fences what FLEETOR drives fences everything
    /// it was not explicitly told to leave alone.
    pub fn for_the_operator(self) -> Self {
        Self { operators_own_seat: true, ..self }
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

    /// **Checkpoints 1, 2 and 3's behavioural half:** the argv one pane is
    /// launched with, given its brief and — for the seats that get one — its
    /// permission posture.
    ///
    /// `permission_mode` is `None` for the operator's own seat, which is watched
    /// by a human who approves its calls, and `Some` for the unattended ones. That
    /// asymmetry is the product (D-030, D-052), so it is the caller's to state.
    fn command_args(&self, brief: &str, permission_mode: Option<&str>) -> Vec<String>;
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
        permission_flag: Some("--permission-mode"),
        sandbox_keys: &[],
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
        ],
    },

    // 6 — config and credential isolation (C17). Configuration relocates by
    // variable; the credential is a keychain entry selected by another. Neither
    // follows HOME, which is the measurement this checkpoint was renamed over.
    isolation: ConfigAndCredentialIsolation {
        config_env: "CLAUDE_CONFIG_DIR",
        credential_env: Some(super::spawn::ENV_CC_SECURESTORAGE_DIR),
        credentials_follow_home: false,
        private_home: true,
        seeds_from_operator: false,
    },

    // 7 — write-guardrail install. `guardrail::install`.
    guardrail: GuardrailInstall {
        settings_file: "settings.json",
        hook_event: "PreToolUse",
        tool_matcher: "Bash|Write|Edit|MultiEdit|NotebookEdit",
        hook_file: guardrail::HOOK_FILE,
    },

    // 8 — outbound reachability. No seatbelt, so the socket is simply reachable.
    outbound: Outbound { sandboxed: false, socket_reachable: true, reachability_keys: &[] },

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
        file_move_is_safe: true,
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
        super::spawn::seed_config_dir(self, seed).map(|()| Vec::new())
    }

    fn command_args(&self, brief: &str, permission_mode: Option<&str>) -> Vec<String> {
        let spec = self.spec();
        let mut args: Vec<String> =
            spec.program.base_args.iter().map(|a| (*a).to_string()).collect();
        if let (Some(flag), Some(mode)) = (spec.posture.permission_flag, permission_mode) {
            args.push(flag.to_string());
            args.push(mode.to_string());
        }
        if let Some(flag) = spec.brief.argv_flag {
            args.push(flag.to_string());
            args.push(brief.to_string());
        }
        args
    }
}

// --- the registry -------------------------------------------------------------

static CLAUDE_CODE: ClaudeCode = ClaudeCode;

/// Every registered harness. **Exactly one, and that is the point of this
/// ticket:** the seam has to be green with one harness before a second exists, or
/// it is not a seam, it is a description of the second one (C20).
static REGISTERED: [&dyn Harness; 1] = [&CLAUDE_CODE];

/// Every harness a pane may run.
pub fn registered() -> &'static [&'static dyn Harness] {
    &REGISTERED
}

/// Claude Code, by name — the sole registered harness, and what
/// [`PaneSpec::harness`](crate::placement::PaneSpec::harness) answers for every
/// pane kind until the gate offers a choice.
pub fn claude_code() -> &'static dyn Harness {
    &CLAUDE_CODE
}

/// A registered harness by name, or `None`. The lookup a manifest read needs when
/// it has a `harness` string and wants the spec behind it (M24).
pub fn by_name(name: &str) -> Option<&'static dyn Harness> {
    registered().iter().copied().find(|h| h.spec().name == name)
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

    #[test]
    fn exactly_one_harness_is_registered_and_it_is_claude_code() {
        // The seam is green with one harness before a second exists (C20). A
        // second entry here without a conformance pass behind it is the
        // "half-implemented harness compiles quietly" failure.
        assert_eq!(registered().len(), 1);
        assert_eq!(registered()[0].spec().name, "claude-code");
        assert_eq!(claude_code().spec(), &CLAUDE_CODE_SPEC);
        assert!(by_name("claude-code").is_some());
        assert!(by_name("nothing-registered-under-this-name").is_none());
    }

    #[test]
    fn the_program_and_the_argv_match_what_the_command_builders_already_produce() {
        // Checkpoints 1, 2 and 3 against `spawn`'s three shapes: `orch` gets the
        // brief alone, every unattended seat gets the permission flag first.
        let cc = claude_code();
        assert_eq!(cc.spec().program.bin, "claude");
        assert_eq!(cc.command_args("BRIEF", None), vec!["--system-prompt", "BRIEF"]);
        assert_eq!(
            cc.command_args("BRIEF", Some("auto")),
            vec!["--permission-mode", "auto", "--system-prompt", "BRIEF"]
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
}
