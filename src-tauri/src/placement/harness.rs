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
}
