//! Codex — checkpoints 2, 4 and 6, and the one reader the three key lists share
//! (WP-25 phase 2, issues #26 and #27; C3, C6, C9, C16, C17, C31, C32, C36, C37).
//!
//! Codex is the second harness and the first that is *configured* rather than
//! flagged: almost everything Claude Code takes as an environment variable or an
//! argv flag, codex takes as a key in a TOML document inside the directory
//! `CODEX_HOME` points at. That single fact is why this module exists as its own
//! file rather than as four more lines in [`harness`](super::harness) — the
//! behaviour here is a document transform, not a table of strings.
//!
//! ## What this ticket owns, and what it deliberately leaves empty
//!
//! [`CODEX_SPEC`] answers all fourteen checkpoints because [`HarnessSpec`] has no
//! [`Default`] — the compiler is the checklist, and there is no way to write "the
//! four this ticket owns" and stop. So every field below is filled from a
//! measurement in `docs/notes/codex-spike-notes.md`, and **the four lists that
//! were left empty on purpose are now all four filled**:
//!
//! | field | checkpoint | filled by |
//! |---|---|---|
//! | [`Posture::sandbox_keys`] | 3 | #28 — the sandbox trio's first two rows |
//! | [`Credentials::provider_keys`] | 5 | #29 — the FLEETOR provider entry |
//! | [`Credentials::scrubbed_env`] | 5 | #29 — the same ticket, the other half |
//! | [`Outbound::reachability_keys`] | 8 | #28 — the socket lever, the trio's third |
//!
//! None of the four needed **any new mechanism, only rows** — which is the shape
//! C36 predicted when it left the reader for the ticket that would first have
//! something to write. What those tickets added instead is a *seat*:
//! [`Seed::operators_own_seat`], because four of codex's 47 default-on features
//! reach past the fence and are turned off on every pane FLEETOR drives (C21), and
//! because a worker runs on the fleet's provider while the orchestrator keeps its
//! own (C9). The orchestrator inherits both, untouched, and that asymmetry is the
//! product rather than an omission.
//!
//! An empty list here would not have been a stub that passed quietly: the
//! conformance suite refuses an empty `scrubbed_env` and refuses a harness whose
//! spec answers nothing. **Codex is still not registered** — #33 is the single
//! moment it joins [`registered`](super::harness::registered) (C25).
//!
//! ## Checkpoint 5 is a table, a selection and a removal (#29, C9, D-062)
//!
//! **A codex worker holds the fleet's credential, never the operator's**, which is
//! not new policy — it is the policy a Claude Code worker already runs under, and
//! it lands here in three pieces that are deliberately in three different places:
//!
//!  - **The entry** — [`Credentials::provider_keys`], four rows, on *every* codex
//!    pane. A provider nothing selects is inert (measured), so this is a fact
//!    about how the fleet's endpoint is spelled rather than a decision about a
//!    seat.
//!  - **The selection** — [`WORKER_PROVIDER_SELECTION`], on every seat but the
//!    operator's. This is the seat decision, and it is C2 as C9 amended it.
//!  - **The removal** — [`Credentials::scrubbed_env`], applied by
//!    `spawn::worker_command_with` and returned on
//!    [`Placed::scrubbed`](super::Placed::scrubbed) so it is assertable as data
//!    rather than as an absence (C27, C30).
//!
//! The fleet's key itself is in none of those: it arrives on the pane's
//! environment through [`FLEET_KEY_ENV`], so **nothing under a fenced pane's
//! `pane-config/` ever holds a credential** — not the fleet's, and not the
//! operator's, whose `auth.json` and provider bearer tokens are kept out of a
//! worker's snapshot by [`OPERATORS_OWN_ENTRIES`] and
//! [`strike_provider_credentials`].
//!
//! ## The seat decides whose login travels (#44, C9, C43)
//!
//! The paragraph above is a **worker's** answer, and for one turn it was every
//! seat's. It should not have been: a codex login lives *inside the directory this
//! module replaces* (C6 — `CODEX_HOME` alone determines configuration and login),
//! so a snapshot that strikes the credential on every seat hands the orchestrator
//! a pane with no login at all. Measured on `codex-cli 0.153.4`, that pane's
//! `codex doctor --json` reports `auth.credentials` as **`fail` — "no Codex
//! credentials were found"**, which contradicts D-030/D-052 and C9: the
//! orchestrator is the operator's own pane, on their own login, spending their own
//! credential.
//!
//! So the credential strike is **seat-conditional, exactly as the selection above
//! already is**, and it branches on the same [`Seed::operators_own_seat`]:
//!
//! | | worker (`false`, the default) | orchestrator (`true`) |
//! |---|---|---|
//! | `auth.json` | never copied | copied — [`OPERATORS_OWN_ENTRIES`] |
//! | `[model_providers.*]` credentials | struck | inherited |
//! | `model_provider` | FLEETOR's | the operator's |
//!
//! **Copying the credential *in* is not writing *out*.** C6's snapshot-not-symlink
//! rule is untouched: the pane's `CODEX_HOME` is still never written back, and the
//! operator's real installation is still never written to — the copy is one-way,
//! and `the_operators_installation_is_never_written_to` seeds an orchestrator as
//! well as four workers to say so.
//!
//! ## Checkpoint 2 is a file in this directory, not a flag (#27, C3)
//!
//! The brief a codex pane runs on is the **same rendered text** a Claude Code pane
//! of the same seat runs on — one `orch.md`, one `worker.md`, every harness
//! (D-042) — and only the carrier varies. Codex's carrier is
//! `model_instructions_file`, a key naming a file, and it **replaces** the
//! vendor's built-in prompt rather than appending to it: measured off the wire,
//! `instructions` went from 17,730 characters of "You are Codex" to the sentinel
//! file's 50 (C3), and it stays that way across a `/clear` (C37).
//!
//! So the brief is written into the pane's own `CODEX_HOME` as [`BRIEF_FILE`],
//! beside the seed and in the same atomic pass, and **nothing is written into the
//! pane's checkout** — no `.git/info/exclude` line, no story about keeping
//! `git status` honest. Two other carriers were measured and are not reachable
//! from here by construction: `base_instructions` in a custom `model_catalog_json`
//! is *not honoured* (the built-in prompt is sent anyway; the field is a
//! descriptor), and an `AGENTS.md` in the pane's cwd arrives as a **`user`
//! message** — in-band, spending the pane's own context, and re-injected on every
//! clear.
//!
//! ## The three key lists get their one reader here (C36)
//!
//! `sandbox_keys`, `provider_keys` and `reachability_keys` had no production
//! reader through the whole of phase 1, left that way three times deliberately
//! (#18, #19, #21) because they are the same absent reader rather than three. That
//! reader is [`CodexCli::seed_config_dir`], and it reads all three in one pass:
//! each `(key, value)` is a **dotted path** into the seeded `config.toml`, and the
//! value is parsed as TOML with a bare string as the fallback.
//!
//! That rule is not invented here. It is codex's own, quoted verbatim from
//! `codex --help`:
//!
//! > Use a dotted path (`foo.bar.baz`) to override nested values. The `value`
//! > portion is parsed as TOML. If it fails to parse as TOML, the raw string is
//! > used as a literal.
//!
//! So `("sandbox_mode", "workspace-write")` becomes `sandbox_mode =
//! "workspace-write"` and `("sandbox_workspace_write.network_access", "true")`
//! becomes a `[sandbox_workspace_write]` table holding `network_access = true` —
//! the boolean, not the word. A ticket writing a key list here writes exactly what
//! it would have typed after `-c`, which is the form every one of those keys was
//! measured through.
//!
//! ## The snapshot, and why it is a copy (C6)
//!
//! Each codex pane's `CODEX_HOME` is a **bootstrap snapshot** of the operator's
//! own `~/.codex`, never written back. Not a symlink: codex writes to its own
//! `config.toml` — `/model` alone is enough — so a symlink is four workers
//! mutating the operator's preferences mid-run, which is what M9 refused. Not an
//! empty directory either: that silently drops the operator's MCP servers, skills
//! and model catalog, and with them the provider C2 says is inherited.
//!
//! The measurement that made this checkpoint 6 rather than "private HOME seeding"
//! is that `CODEX_HOME` carries **configuration and login together**: a fabricated
//! `HOME` with the real `CODEX_HOME` stayed logged in, and an empty `CODEX_HOME`
//! reported not-logged-in regardless of `HOME`. The Fence's private `HOME` is
//! still given to a codex worker and is still real — `~/.ssh`, shell profiles, the
//! `.gitconfig` a first commit needs — it is simply not this harness's credential
//! mechanism.
//!
//! ## The measured trap this module exists to defuse
//!
//! **A `~` inside a copied configuration value resolves against the pane's private
//! `HOME`, not the operator's.** The operator's own config carries
//! `model_catalog_json = "~/.codex/models.json"`, and a pane seeded with that
//! verbatim dies at spawn with `Error loading configuration: No such file or
//! directory (os error 2)`. [`refuse_home_relative`] is the mechanism: the seed
//! is refused rather than installed if any string value in it still begins with a
//! tilde, so the property is enforced where it is produced and not only where it
//! is asserted.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fleetor_core::event::NoticeLevel;
use toml_edit::{DocumentMut, Item, Table, Value};

use super::harness::{
    BriefCarrier, CommandChannel, ConfigAndCredentialIsolation, ConfigDir, Credentials, GaugeSource,
    GuardrailInstall, Harness, HarnessSpec, OrphanNames, Outbound, Posture,
    ProjectIdentityAndTrust, Program, Seed, Transcript, TypingProfile,
};

// --- the vendor's own shape ----------------------------------------------------

/// The operator's installation, relative to their `HOME`.
///
/// **Here rather than on [`HarnessSpec`], deliberately.** Where a vendor keeps the
/// operator's own copy is the same kind of fact as what document format it writes:
/// the vendor's, needed only by the one method that reads it, and general by
/// accident if it were a spec field a harness with no operator installation would
/// have to answer with `None` (C31, C32).
pub const OPERATOR_DIR: &str = ".codex";

/// What a snapshot carries in, by name.
///
/// **An allowlist, not a denylist**, because the failure modes are asymmetric: a
/// name missing from an allowlist costs a preference, and a name missing from a
/// denylist copies the operator's credential into four fenced panes. Each entry
/// below is one of the things the ticket's acceptance criteria name, and
/// `config.toml` is absent because it is not copied — it is transformed, by
/// [`seeded_document`].
///
/// What is **not** here, and why, measured against a real installation:
///
///  - `auth.json` — the operator's credential, which is **not on every seat's
///    snapshot and is on the orchestrator's**. It is [`OPERATORS_OWN_ENTRIES`],
///    below: a worker holds the fleet's credential and never the operator's (C9,
///    D-062), and the operator's own pane holds their own login because that is
///    what the seat is (#44, C43).
///  - `packages/` (**275 MB**) — the vendor's own downloaded release binaries.
///    Four panes' worth is a gigabyte of the same bytes, and `codex` is on the
///    pane's `PATH` already.
///  - `.tmp/plugins/` (**88 MB**) — the plugin *marketplace cache*, which is what
///    `codex plugin marketplace list` reports as its root. The plugin
///    *configuration* travels in `config.toml` with everything else; the cache is
///    a cache, under a `.tmp` the vendor named itself, and re-fetchable. This is
///    the one acceptance criterion that did not survive contact with the
///    installation — see `decisions.md`.
///  - `*.sqlite*` — `thread_history_1`, `state_5`, `logs_2`, `memories_1`,
///    `goals_1`, `queue_1`. These are per-pane state, and copying them is the
///    opposite of what C6 wants: the whole secondary reason for a private
///    `CODEX_HOME` is that a pane's thread history is its own, which makes
///    checkpoint 13's harvest a per-directory read with pane attribution for
///    free. They are also WAL-mode, so a file copy of one is a torn copy.
///  - `installation_id`, `version.json`, `.sandbox_migration`,
///    `shell_snapshots/`, `thread-writer-locks/`, `tmp/` — machine and run state,
///    not preferences.
///  - `memories/` — knowledge rather than preference, and Tier 1.8 says shared
///    knowledge merges only after review. Left out until a ticket asks for it.
const SNAPSHOT_ENTRIES: &[&str] = &[
    // Checkpoint 4's "the model catalog": the file `model_catalog_json` names.
    "models.json",
    // The operator's skills.
    "skills",
    // Custom prompts, when the operator has any.
    "prompts",
];

/// **What the snapshot carries in on the operator's own seat, and on no other**
/// (#44, C9, C43).
///
/// A second list rather than a flag on [`SNAPSHOT_ENTRIES`]'s rows, and rather
/// than a spec field, for [`WORKER_PROVIDER_SELECTION`]'s reason (C31, C41(b)):
/// the two lists answer different questions. `SNAPSHOT_ENTRIES` is *what a codex
/// pane needs to be the operator's pane* — preferences, catalog, skills, prompts —
/// and it is the same list on every seat. This is *what only the operator's own
/// seat may hold*, and it is the credential file. Folding them into one list of
/// `(name, seat)` pairs would make every future preference answer a question it
/// does not have — which is the shape a spec field would have forced too.
///
/// One name today, and a list rather than a constant because the vendor's
/// credential storage is a directory whose shape it owns: `auth storage mode` is
/// reported by `codex doctor --json` as `File` on this build, and a build that
/// splits the file is one name added here rather than a mechanism.
///
/// **The direction is the whole safety argument.** This copies *in*; nothing here
/// or anywhere in this module writes into the operator's installation, so C6's
/// snapshot-not-symlink rule holds exactly as it did — a pane that refreshes its
/// own token rewrites its own copy, and the operator's `~/.codex` never learns
/// about it. Cost, stated: the reverse is also true, so a plan token refreshed
/// inside the pane is not carried back to the operator's own installation, and a
/// re-seed overwrites the pane's copy with the operator's again.
const OPERATORS_OWN_ENTRIES: &[&str] = &["auth.json"];

/// Per-provider keys that carry, or reach, the operator's own credential — struck
/// from every `[model_providers.*]` table on a **fenced** seat's way in (C9, C43).
///
/// `experimental_bearer_token` is the operator's key in plaintext; the real
/// installation has one. `env_key` is subtler and is here for L2's reason: it
/// names an environment variable the vendor will read the key out of, so a value
/// that survives points a fenced pane straight at whatever the operator exported
/// in their shell profile. Naming a variable is not a secret, and following it is.
/// **FLEETOR writes its own `env_key` back, after the strike** (#29, C9). That is
/// not a contradiction: the strike removes a name *the operator* chose, which
/// points at whatever their shell profile exported; the row FLEETOR writes points
/// at [`FLEET_KEY_ENV`], a variable only [`spawn::worker_command_with`] sets and
/// only on a fenced seat. The order in [`install`] is what makes that true —
/// FLEETOR's keys are written last, so they win.
///
/// **Not struck on the operator's own seat** (#44, C43). The orchestrator keeps
/// the provider it inherited (C2 as C9 amended it), and a provider entry with its
/// credential removed is an inherited provider that cannot authenticate — which is
/// the third auth shape C14 requires, since the operator's own installation is a
/// third-party endpoint with its own bearer token. That seat is also the one seat
/// `scrubbed_env` is not applied to (`spawn::worker_command_with` is the only
/// caller of `scrub`), so an inherited `env_key` still finds the variable the
/// operator's own shell exported — which is what makes keeping the name correct
/// there and dangerous on a fenced pane.
const PROVIDER_CREDENTIAL_KEYS: &[&str] = &["experimental_bearer_token", "env_key"];

/// **The variable a codex worker's fleet credential arrives in** (#29, C9, D-062).
///
/// Named once and read twice — as [`Credentials::token_env`], which is the line in
/// [`spawn::worker_command_with`] that sets it from the `.env` walk, and as the
/// `env_key` of the FLEETOR provider entry below, which is what tells the vendor
/// to read it. Two spellings of one variable is the failure this constant exists
/// to make impossible, and `the_two_halves_of_the_credential_channel_name_one_variable`
/// is the test that says so.
///
/// **The key is never written into the pane's configuration file.** Codex's
/// provider table has two credential mechanisms and this is the one that keeps the
/// secret out of a file on disk: `experimental_bearer_token` would put the fleet's
/// key in plaintext inside `pane-config/worker-N/config.toml`, and `env_key` names
/// a variable instead. It is also the exact mirror of what a Claude Code worker
/// gets — `ANTHROPIC_AUTH_TOKEN`, set on the command by the same function on the
/// same line — which is C9's whole argument made mechanical.
///
/// Measured on `codex-cli 0.153.4`: with this row present, `codex doctor --json`
/// reports `auth.credentials` as `auth is provided by the active model provider`
/// and names the variable; with the variable unset it **fails** rather than
/// falling back, *even when the operator's own `CODEX_API_KEY` is present in the
/// environment*. The fleet's provider entry cannot be satisfied by the operator's
/// key, by construction rather than by the scrub.
const FLEET_KEY_ENV: &str = "FLEETOR_CODEX_KEY";

/// What FLEETOR's own `[model_providers.*]` table is called.
///
/// The table's name is load-bearing twice — in the dotted paths of
/// [`Credentials::provider_keys`] and in the value of
/// [`WORKER_PROVIDER_SELECTION`] — and a pane whose `model_provider` names a table
/// that does not exist dies at configuration load. `fleetor` rather than the
/// vendor's or the provider's name, because the point of the entry is *whose it
/// is*: an operator reading their pane's `config.toml` should be able to tell at a
/// glance which provider row FLEETOR wrote.
const FLEET_PROVIDER: &str = "fleetor";

/// **The one row that says a pane runs on the fleet's provider rather than the
/// operator's** (#29, C2 as amended by C9).
///
/// The *table* is [`Credentials::provider_keys`] and lands on every codex pane,
/// because that is what a spec key list means (C41(a)) and because a defined
/// provider nothing selects is inert — measured: an unselected
/// `[model_providers.fleetor]` whose `env_key` variable is absent resolves
/// perfectly. The *selection* is this, and it is the seat's rather than the
/// harness's, so it lives here for [`WORKER_FEATURE_OVERRIDES`]'s reason (C31,
/// C41(b)): a spec field for "the key set on every seat but the operator's" is a
/// field every other harness answers with an empty list.
///
/// **The orchestrator keeps the provider it inherited, and that is the whole of
/// C9's amendment to C2.** The provider is displayed as a fact on the operator's
/// own seat, where their own login is the entire point (D-030, D-052); on a seat
/// FLEETOR drives, FLEETOR owns it. There is no picker anywhere and C24 keeps it
/// that way.
///
/// Cost, stated rather than hidden: a codex worker cannot spend an operator's
/// subscription plan even when that is what they wanted.
const WORKER_PROVIDER_SELECTION: &[(&str, &str)] = &[("model_provider", FLEET_PROVIDER)];

/// **The four default-on features FLEETOR turns off on every seat but the
/// operator's own** (#28, C21) — five rows, because `browser_use` carries a
/// companion.
///
/// `codex doctor --all` reports **47 feature flags enabled by default**. Four of
/// them reach past the fence [`CODEX_SPEC`]'s sandbox trio actually enforces: the
/// seatbelt bounds the filesystem and the network, and it does not bound a pane
/// driving a browser, a pane driving a desktop, or a pane fanning out into threads
/// the run manifest never sees.
///
/// **That last one is the sharpest.** WP-11's archive claims to be the evidence of
/// what a run did, and D-029/D-030 deleted headless supervision on purpose, so a
/// worker quietly re-growing it is a regression wearing a feature flag.
///
/// `browser_use_full_cdp_access` is the fifth row for four features: C21 names it
/// in the same breath as `browser_use` because it is separately on by default and
/// separately grants full remote-debugging access. Disabling the parent should be
/// enough; a row that is redundant narrows nothing further and costs a line,
/// where a missing one is a browser a fenced pane can still drive.
///
/// **Here rather than on [`HarnessSpec`]** for C31's reason and the same one
/// [`PROVIDER_CREDENTIAL_KEYS`] is: these are vendor feature names, read by the
/// one method that writes them, and a spec field for "keys set on every seat but
/// the operator's" would be a field every other harness answers with an empty
/// list. What *is* general — which seat this is — is on
/// [`Seed::operators_own_seat`], because only the caller knows it.
///
/// **The key spelling is the vendor's own**, quoted from `codex features --help`:
/// `--disable <FEATURE>` is documented as "Equivalent to `-c features.<name>=false`".
const WORKER_FEATURE_OVERRIDES: &[(&str, &str)] = &[
    // A worker spawning its own subagent threads — the run manifest never sees them.
    ("features.multi_agent", "false"),
    // A pane driving a browser, and the full remote-debugging access that comes with it.
    ("features.browser_use", "false"),
    ("features.browser_use_full_cdp_access", "false"),
    // A pane driving the desktop.
    ("features.computer_use", "false"),
    // And the same reach under the vendor's in-app name for it.
    ("features.in_app_local_automation", "false"),
];

/// The file `CODEX_HOME` is read from — checkpoint 4's seed file, and checkpoint
/// 14's trust file, which for this harness are the same document.
const CONFIG_FILE: &str = "config.toml";

/// Checkpoint 2's file: the pane's brief, beside the seed, named by
/// [`BriefCarrier::config_key`] (#27, C3).
///
/// **Here rather than on the spec, for [`OPERATOR_DIR`]'s reason** (C31, C32).
/// The spec says *which key names the brief*, which is the vendor's contract and
/// the thing a conformance suite can assert; what FLEETOR calls the file it writes
/// into a directory it owns is not a checkpoint, and a `brief_file` field would be
/// a spec entry every harness that carries its brief in argv answers with a name
/// nothing reads.
///
/// **`fleetor-` prefixed on purpose.** `CODEX_HOME` is the vendor's directory —
/// `config.toml`, `models.json`, `prompts/`, its own sqlite stores — and a bare
/// `brief.md` there is a file whose author nobody can tell from the outside.
const BRIEF_FILE: &str = "fleetor-brief.md";

/// Checkpoint 14's affirmative answer, which for this harness is a **string**
/// rather than Claude Code's `true`.
///
/// It lives here rather than on the spec for C31's reason: `trust_keys` names
/// keys, and what counts as saying yes is the vendor's. The conformance suite's
/// `is_affirmative` already accepts either shape, which is what makes that
/// possible without a `seed_values` field nobody wants.
const TRUST_AFFIRMATIVE: &str = "trusted";

// --- the fourteen --------------------------------------------------------------

/// Codex — the second harness. **Not registered**; #33 does that (C25).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CodexCli;

/// Codex's fourteen answers, every one of them from a measurement in
/// `docs/notes/codex-spike-notes.md` against `codex-cli 0.153.4`.
pub const CODEX_SPEC: HarnessSpec = HarnessSpec {
    name: "codex",

    // 1 — program and base arguments. A native binary, no interpreter, no flags
    // every seat shares.
    program: Program { bin: "codex", base_args: &[] },

    // 2 — brief carrier (C3). `model_instructions_file` *replaces* the built-in
    // prompt: read off the wire, the request's `instructions` went from the
    // built-in 17,730 characters to the sentinel file's 50 and "You are Codex" was
    // absent from the whole body. It is a config key rather than an argv flag, so
    // unlike cursor's `.mdc` nothing is written into the pane's checkout and there
    // is no `.git/info/exclude` line to keep `git status` honest. C37 measured
    // that it survives `/clear`. The key is written and the file it names is
    // installed by `install_brief`, out of `Seed::brief` (#27).
    brief: BriefCarrier {
        argv_flag: None,
        config_key: Some("model_instructions_file"),
        replaces_system_prompt: true,
        writes_into_worktree: false,
    },

    // 3 — model and posture. Codex takes its model as an argv flag rather than an
    // environment variable, and has no permission *flag* at all: its containment
    // is the seatbelt, which is configured. `sandbox_keys` is #28's.
    posture: Posture {
        model_env: None,
        model_flag: Some("--model"),
        permission_flag: None,
        // **The sandbox trio, two of its three rows** (#28, C7). The third is
        // checkpoint 8's and lives on `outbound` because it is the socket lever
        // rather than the fence — but all three were measured together on
        // `0.153.4` and only make sense together: `workspace-write` without
        // `never` is a pane that parks on an approval prompt no human will answer,
        // and `never` without `workspace-write` is a pane auto-running under
        // whatever posture the operator's own config happened to carry.
        //
        // `never` rather than `on-request` for the reason the whole fence exists:
        // `on-request` lets the model decide when to ask a human, and **a worker
        // has no human**, so it parks looking perfectly healthy.
        //
        // **Both rows are FLEETOR's on every codex pane, and that is deliberate.**
        // `sandbox_mode` lives in the operator's own configuration, so an operator
        // who set it months ago for unrelated reasons would otherwise get a fleet
        // whose panes cannot write — or one that is not fenced at all. Every key
        // FLEETOR writes over an operator's answer gets an Activity feed line;
        // see `install`.
        sandbox_keys: &[("sandbox_mode", "workspace-write"), ("approval_policy", "never")],
    },

    // 4 — config dir and its seeding. The two seed keys are this ticket's own and
    // they are both isolation: they pin the pane's state store and its logs inside
    // its own `CODEX_HOME`, so an operator who has pointed either somewhere shared
    // does not silently un-isolate four panes into one store. See
    // `isolation_keys`.
    config_dir: ConfigDir {
        env_var: "CODEX_HOME",
        seed_file: CONFIG_FILE,
        seed_keys: &["sqlite_home", "log_dir"],
        seed_merges: true,
    },

    // 5 — credential wiring and the scrub (#29, C9, D-062). **A codex worker holds
    // the fleet's credential, never the operator's**, and both halves below are
    // what makes that true.
    //
    // The channel is *both* config and environment, because codex's provider table
    // splits them: the four `provider_keys` rows are the FLEETOR entry — a table
    // the vendor reads out of the seeded `config.toml` — and its `env_key` names
    // `FLEET_KEY_ENV`, which `spawn::worker_command_with` sets from the `.env` walk
    // on the same line it sets a Claude Code worker's `ANTHROPIC_AUTH_TOKEN`. So
    // the endpoint is written down and **the secret never is**.
    //
    // `base_url_env` stays `None` because codex has no endpoint variable for a
    // provider it did not define; the endpoint is a row in the table. That is the
    // one place the fleet's endpoint is spelled twice — see the `base_url` row.
    credentials: Credentials {
        base_url_env: None,
        // Read by `spawn::worker_command_with` and by nothing else, so a fenced
        // seat gets it and the three attended seats do not — which is why the
        // provider entry below is inert on the operator's own pane even though the
        // table is written there too.
        token_env: Some(FLEET_KEY_ENV),
        // **The FLEETOR provider entry**, written into every codex pane's seeded
        // `config.toml` as four dotted paths, each exactly what would have been
        // typed after `-c` (C39(b)). Only `WORKER_PROVIDER_SELECTION` selects it.
        //
        // `base_url` and `wire_api` are C9's measured pair. The base URL is the
        // fleet's own endpoint **in its `responses`-wire spelling**, which is not
        // the `.../anthropic` suffix `LaunchConfig::worker_base_url` carries for
        // Claude Code — same vendor, two wire protocols, two paths. It is a
        // literal here rather than a value off `LaunchConfig` because there is no
        // launch key that holds it; an operator who repoints `worker.base_url`
        // repoints their Claude Code workers and not their codex ones, and the
        // thing that would reverse that is a second launch key, not a change here.
        provider_keys: &[
            ("model_providers.fleetor.name", "FLEETOR"),
            ("model_providers.fleetor.base_url", "https://api.deepseek.com/"),
            ("model_providers.fleetor.wire_api", "responses"),
            ("model_providers.fleetor.env_key", FLEET_KEY_ENV),
        ],
        // **Removed from the inherited environment, not merely left unset** — the
        // half that is easier to get wrong, and the reason `Placed::scrubbed`
        // exists (C27, C30). Both names are codex's own: `doctor` reports them
        // under `auth env vars present`, so an operator with either in a shell
        // profile is an operator whose personal credential authenticates a fenced
        // pane. That the FLEETOR entry's `env_key` refuses to fall back to them
        // was measured and is the belt; this is the braces, and it is what makes
        // the removal assertable as data on every seat that gets one.
        scrubbed_env: &["OPENAI_API_KEY", "CODEX_API_KEY"],
    },

    // 6 — config and credential isolation (C17). The measurement that renamed this
    // checkpoint: `CODEX_HOME` carries configuration *and* login, so there is no
    // separate credential variable and the credential does not follow `HOME`. The
    // private `HOME` is still given — the Fence wants it for `~/.ssh` and shell
    // profiles — it is simply not load-bearing for authentication.
    isolation: ConfigAndCredentialIsolation {
        config_env: "CODEX_HOME",
        credential_env: None,
        credentials_follow_home: false,
        private_home: true,
        seeds_from_operator: true,
    },

    // 7 — write-guardrail install (C4). Codex has a real pre-edit event, so D-065
    // ports rather than needing M12's "the sandbox stands in" argument. The
    // settings file is the same `config.toml` as everything else, which is why
    // C32 named the installer as the thing that becomes a `Harness` method: the
    // document is TOML and `guardrail::install` writes JSON. **#31's ticket.**
    guardrail: GuardrailInstall {
        settings_file: CONFIG_FILE,
        hook_event: "pre_tool_use",
        tool_matcher: "*",
        hook_file: crate::guardrail::HOOK_FILE,
    },

    // 8 — outbound reachability (C5). Codex confines its pane in a real seatbelt —
    // `echo x > $HOME/probe` under it was refused with no file created — and a
    // unix-socket connect is refused by default. One key lifts it, and the narrow
    // alternatives (`network.unix_sockets`,
    // `network.dangerously_allow_all_unix_sockets`, `--enable network_proxy`) were
    // tried and do not: they configure the network-proxy layer, not the seatbelt.
    // `socket_reachable` is `true` because it is reachable *as FLEETOR configures
    // it*; the key that makes it so is #28's, alongside the rest of the trio.
    outbound: Outbound {
        sandboxed: true,
        socket_reachable: true,
        // **One key, and it is all-or-nothing** (#28, C5). `true` is the boolean
        // rather than the word, by codex's own `-c key=value` rule (C39) — the
        // dotted path becomes a `[sandbox_workspace_write]` table on the way in.
        //
        // This is the second key that is FLEETOR's rather than inherited, and it
        // is load-bearing for messaging rather than for comfort: an operator who
        // narrowed their sandbox's network access would get a fleet of panes that
        // cannot reach the fleet socket, which is a pane that looks alive and
        // cannot talk.
        reachability_keys: &[("sandbox_workspace_write.network_access", "true")],
    },

    // Bring-up (#42). A fresh pane opens on a splash that ends on a keypress and
    // discards what it is sent until then, so it is woken before it is announced.
    bring_up: super::harness::BringUp::AfterWaking,

    // 9 — typing profile (C22). A bracketed paste followed by CR into a real pty
    // submits the turn: the request reached the capture server carrying a sentinel
    // absent from the pasted bytes. **The framing is measured; the gap is not, and
    // is #32's.** C22's 6 s/0.6 s failure and 12 s/1.5 s success used fixed sleeps
    // that conflate a startup wait with the post-paste gap, so which of the two is
    // load-bearing is not yet known — and `submit_gap_ms` is the one delay on the
    // message path (Tier 1.4, D-034), which is not a number to guess at. It stays
    // at Claude Code's measured 30 until #32 isolates codex's own, and C37 already
    // showed the startup half is a readiness question rather than a constant.
    typing: TypingProfile {
        bracketed_paste: true,
        paste_start: b"\x1b[200~",
        paste_end: b"\x1b[201~",
        submit_bytes: b"\r",
        submit_gap_ms: 30,
    },

    // 10 — command-channel spellings (C10). `codex` carries `/clear` and
    // `/compact` under the fleet's own names, so M7's open question closes with
    // identical rows.
    commands: CommandChannel { spellings: &[("/clear", "/clear"), ("/compact", "/compact")] },

    // 11 — gauge source and window. Codex publishes `context_window` per model in
    // its own catalog, which is strictly better than the fleet asserting one, so
    // there is no window to export and no variable to export it through (D-054).
    // Whether per-turn usage is persisted at all is the open question C12 names
    // and #40's to answer.
    gauge: GaugeSource { reads_transcript: true, window_tokens: None, window_env: None },

    // 12 — orphan-sweep names (C11). A native binary, so one word — no interpreter
    // to match around, which is the whole of cursor's difficulty here.
    orphans: OrphanNames { comm_suffixes: &["codex"] },

    // 13 — transcript location and format (C12). `thread_history_1.sqlite` inside
    // the pane's own `CODEX_HOME`, with `thread_items` and `thread_turns` tables
    // under an `_sqlx_migrations`-managed schema. The generation number in the
    // filename is codex's own compatibility signal and is what the manifest
    // records. **`file_move_is_safe` is `false` and that is the load-bearing
    // field**: it is WAL-mode, so archiving it is `VACUUM INTO` or the backup API
    // and never `cp` — `run-rotation-notes.md` measured the torn-copy failure for
    // the fleet's own store and the same physics applies here.
    transcript: Transcript {
        subdir: "",
        file_ext: "sqlite",
        format: "codex-thread-history-1-sqlite",
        file_move_is_safe: false,
    },

    // 14 — project identity and trust seeding (C16, C17). The key canonicalizes
    // exactly as Claude Code's does and for the same macOS reason. **`trust_file`
    // is the same document as the seed file**, so one read-modify-write covers
    // both checkpoints.
    //
    // `exact_path_match` is `true` because that is what this harness's
    // `project_key` produces and what the seeder writes. It is also the field with
    // the live spike behind it: the operator's own config carries
    // `[projects."/Users/…/harness"]`, a *parent* of the repository, which
    // suggests codex may resolve trust by repository root. Observed, not verified.
    // **#30 verifies it**, and a `false` there means the seeder writes one row per
    // repository rather than one per worktree — a change to this module's
    // `trust_record`, not to its shape.
    project_identity: ProjectIdentityAndTrust {
        canonicalize: true,
        exact_path_match: true,
        trust_file: CONFIG_FILE,
        trust_keys: &["trust_level"],
    },
};

impl Harness for CodexCli {
    fn spec(&self) -> &'static HarnessSpec {
        &CODEX_SPEC
    }

    /// The same canonicalization Claude Code uses, and for the same reason: on
    /// macOS `/tmp` and `/var` are symlinks and a child's own cwd comes back
    /// resolved, so an unresolved key would never match.
    ///
    /// **Written out here rather than delegating to `spawn::project_key`** (C31).
    /// That function is Claude Code's own answer, and two harnesses agreeing
    /// because both call it is precisely the coupling the seam exists to remove —
    /// the day #30 measures that codex resolves trust by repository root, this
    /// body changes and Claude Code's does not.
    fn project_key(&self, cwd: &Path) -> String {
        std::fs::canonicalize(cwd)
            .unwrap_or_else(|_| cwd.to_path_buf())
            .to_string_lossy()
            .into_owned()
    }

    /// Checkpoints 4, 6 and 14 in one call, and the one reader of the three key
    /// lists (C36).
    ///
    /// The order is the order it has to be in:
    ///
    /// 1. **The tree first.** [`SNAPSHOT_ENTRIES`] are copied in before the
    ///    document is transformed, because step 3 re-points a value at the pane's
    ///    own copy only when that copy is already there.
    /// 2. **The base document.** An existing seeded `config.toml` when there is
    ///    one — that is what [`ConfigDir::seed_merges`] means here, and it is what
    ///    keeps the previous target's trust row alive across a target switch — and
    ///    otherwise the operator's own, transformed by [`seeded_document`].
    /// 3. **FLEETOR's keys last, so they win.** The isolation keys, then
    ///    checkpoint 2's brief file and the key naming it, then the three
    ///    checkpoint key lists in checkpoint order, then the trust record. M10's
    ///    "overlays are additive and deny wins" gains its inverse here (C21): the
    ///    keys FLEETOR owns overwrite the operator's, because a snapshot that
    ///    could turn the sandbox off would be a snapshot that widens auto-approve.
    /// 4. **The refusal.** [`refuse_home_relative`] before anything is installed.
    /// 5. **Temp file and rename**, because a half-written `config.toml` is a pane
    ///    that dies at spawn.
    fn seed_config_dir(&self, seed: &Seed<'_>) -> Result<Vec<(NoticeLevel, String)>, String> {
        install(self.spec(), &self.project_key(seed.cwd), seed)
    }

    /// Checkpoints 1, 2 and 3's behavioural half.
    ///
    /// **The brief does not travel in argv for this harness** — it is
    /// `model_instructions_file`, a config key naming a file — so what this
    /// returns carries no brief, and the file it names is written beside the seed
    /// by [`install_brief`] out of [`Seed::brief`] (#27, C3). The argument is
    /// ignored here rather than at the call site, because which of the two
    /// carriers a harness uses is [`BriefCarrier`]'s answer and not the caller's.
    fn command_args(&self, _brief: &str, _permission_mode: Option<&str>) -> Vec<String> {
        self.spec().program.base_args.iter().map(|a| (*a).to_string()).collect()
    }
}

/// The body of [`CodexCli::seed_config_dir`], taking its spec and its project key
/// as arguments.
///
/// **Split out so the three key lists can be exercised against a spec that has
/// some** (C36). `codex`'s own three lists are empty until #28 and #29 fill them,
/// and a reader whose only test is against three empty lists is a reader nobody
/// has run — which is the shape C26 named and C36 authorized this ticket to
/// resolve rather than repeat. The tests below hand this function a stand-in spec
/// whose lists are full, so the mechanism the next three tickets write into is
/// tested now and not when they get there.
fn install(
    spec: &'static HarnessSpec,
    project_key: &str,
    seed: &Seed<'_>,
) -> Result<Vec<(NoticeLevel, String)>, String> {
    let dir = seed.config_dir;
    let seat = seat_label(dir);
    let mut notices = Vec::new();
    std::fs::create_dir_all(dir).map_err(|e| format!("create config dir {}: {e}", dir.display()))?;

    let source = seed.operator_home.map(|home| home.join(OPERATOR_DIR)).filter(|d| d.is_dir());

    // 1. The tree. The same entries on every seat — and the operator's login on
    // theirs alone, because a codex login lives inside the directory this module
    // replaces (C6), so a seat that does not carry it has none at all (#44, C43).
    if let Some(source) = &source {
        for entry in SNAPSHOT_ENTRIES {
            copy_into(&source.join(entry), &dir.join(entry))?;
        }
        if seed.operators_own_seat {
            for entry in OPERATORS_OWN_ENTRIES {
                copy_into(&source.join(entry), &dir.join(entry))?;
            }
        }
    }

    // 2. The base document.
    let installed = dir.join(spec.config_dir.seed_file);
    let mut doc = match (spec.config_dir.seed_merges, std::fs::read_to_string(&installed)) {
        (true, Ok(text)) => {
            text.parse::<DocumentMut>().map_err(|e| format!("parse {}: {e}", installed.display()))?
        }
        _ => seeded_document(source.as_deref(), dir, seed.operators_own_seat)?,
    };

    // 3. FLEETOR's keys, last so they win.
    for (key, value) in isolation_keys(spec, dir) {
        set_owned(&mut doc, &[key], Value::from(value), &seat, ISOLATION_WHY, &mut notices);
    }
    if let Some(key) = spec.brief.config_key {
        set_path(&mut doc, &[key], Value::from(install_brief(dir, seed.brief)?));
    }
    for (key, value) in checkpoint_keys(spec) {
        let path: Vec<&str> = key.split('.').collect();
        set_owned(&mut doc, &path, toml_value(value), &seat, CONTAINMENT_WHY, &mut notices);
    }
    // The rows that are the *seat's* rather than the harness's (C9, C21). The
    // orchestrator is the operator's own pane and inherits their provider and
    // their flags untouched; every other seat is one FLEETOR drives, and
    // `operators_own_seat` defaults to `false` so a seat nobody thought about is
    // fenced rather than trusted.
    if !seed.operators_own_seat {
        // Checkpoint 5's seat half: the entry went into every pane above, and this
        // is the line that makes a pane run on it. Written here rather than in
        // `checkpoint_keys` precisely because it is not a harness answer — it is
        // the answer for one seat, and putting it in the spec would make the
        // orchestrator run on the fleet's credential too (C2, as amended by C9).
        for (key, value) in WORKER_PROVIDER_SELECTION {
            let path: Vec<&str> = key.split('.').collect();
            set_owned(&mut doc, &path, toml_value(value), &seat, CREDENTIAL_WHY, &mut notices);
        }
        for (key, value) in WORKER_FEATURE_OVERRIDES {
            let path: Vec<&str> = key.split('.').collect();
            set_owned(&mut doc, &path, toml_value(value), &seat, FEATURES_WHY, &mut notices);
        }
    }
    notices.push(narrowing_notice(spec, &seat, seed.operators_own_seat));
    for key in spec.project_identity.trust_keys {
        set_path(&mut doc, &["projects", project_key, key], Value::from(TRUST_AFFIRMATIVE));
    }

    // 4. The measured trap, refused rather than installed.
    refuse_home_relative(&doc)?;

    // 5. Temp file, then rename.
    let tmp = installed.with_extension("toml.tmp");
    std::fs::write(&tmp, doc.to_string()).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &installed)
        .map_err(|e| format!("install {}: {e}", installed.display()))?;
    Ok(notices)
}

/// Codex, by name. Not in the registry — #33 puts it there.
pub fn codex() -> &'static dyn Harness {
    &CODEX
}

static CODEX: CodexCli = CodexCli;

// --- the snapshot --------------------------------------------------------------

/// The operator's `config.toml`, made safe to be a pane's own.
///
/// Three transforms, in this order, and each one is a measurement:
///
/// 1. **The credential comes out on a fenced seat** (C9, C43). Every
///    `[model_providers.*]` table loses [`PROVIDER_CREDENTIAL_KEYS`], and
///    `auth.json` is not among the entries a fenced seat's snapshot carries. On
///    the operator's own seat neither happens: that pane is their own, on their
///    own login, and `operators_own_seat` is the one question deciding both halves
///    (#44).
/// 2. **Tildes are expanded against the operator's `HOME`.** This is the trap: a
///    `~` resolves against the *pane's* private home at spawn, so
///    `model_catalog_json = "~/.codex/models.json"` names a file that is not
///    there and the pane dies with a configuration-load error.
/// 3. **Paths into the operator's installation are re-pointed at the pane's own
///    copy**, when the snapshot carried that file in. This is what makes "never
///    written back" true by mechanism rather than by promise: after seeding, a
///    value the vendor might write through names a file inside the pane's
///    `CODEX_HOME` and not one inside `~/.codex`. A path into the operator's
///    installation that was *not* copied — a plugin cache, say — keeps its
///    absolute form, which is a read the vendor was going to do anyway.
///
/// `None` for the source is a real machine, not a test convenience: a host with no
/// operator `HOME`, or an operator who has never run codex. The pane gets a clean
/// document, which is a working pane without the operator's preferences rather
/// than a fleet that refuses to start.
fn seeded_document(
    source: Option<&Path>,
    pane_dir: &Path,
    operators_own_seat: bool,
) -> Result<DocumentMut, String> {
    let Some(source) = source else {
        return Ok(DocumentMut::new());
    };
    let file = source.join(CONFIG_FILE);
    let Ok(text) = std::fs::read_to_string(&file) else {
        return Ok(DocumentMut::new());
    };
    let mut doc = text
        .parse::<DocumentMut>()
        .map_err(|e| format!("parse the operator's {}: {e}", file.display()))?;

    if !operators_own_seat {
        strike_provider_credentials(&mut doc);
    }

    let operator_home = source.parent().map(Path::to_path_buf);
    rewrite_item(doc.as_item_mut(), &|raw| {
        relocate(raw, operator_home.as_deref(), source, pane_dir)
    });

    Ok(doc)
}

/// Every `[model_providers.*]` table loses the keys that carry or reach the
/// operator's credential. The provider itself stays: C2 says it is inherited and
/// displayed, and #29 is what gives a worker seat the fleet's own instead.
///
/// **Called on a fenced seat only** (#44, C43) — the caller branches, rather than
/// this function taking the seat, so the one place a seat is read stays
/// [`install`] and `seeded_document`'s three transforms and this function's one
/// job each keep a single subject.
fn strike_provider_credentials(doc: &mut DocumentMut) {
    let Some(providers) = doc.get_mut("model_providers").and_then(Item::as_table_like_mut) else {
        return;
    };
    let names: Vec<String> = providers.iter().map(|(name, _)| name.to_string()).collect();
    for name in names {
        let Some(provider) = providers.get_mut(&name).and_then(Item::as_table_like_mut) else {
            continue;
        };
        for key in PROVIDER_CREDENTIAL_KEYS {
            provider.remove(key);
        }
    }
}

/// One value's new spelling, or `None` to leave it exactly as it was.
///
/// The two rules, in order: expand a leading `~` against the operator's real
/// `HOME`, then — whether or not that fired — re-point anything inside the
/// operator's installation at the pane's own copy of it, if the snapshot brought
/// that copy in.
fn relocate(
    raw: &str,
    operator_home: Option<&Path>,
    operator_dir: &Path,
    pane_dir: &Path,
) -> Option<String> {
    let expanded = match (raw.strip_prefix("~/"), raw == "~", operator_home) {
        (Some(rest), _, Some(home)) => Some(home.join(rest)),
        (None, true, Some(home)) => Some(home.to_path_buf()),
        _ if Path::new(raw).is_absolute() => Some(PathBuf::from(raw)),
        _ => None,
    }?;

    let relocated = expanded
        .strip_prefix(operator_dir)
        .ok()
        .map(|rest| pane_dir.join(rest))
        .filter(|inside| inside.exists())
        .unwrap_or(expanded);

    let text = relocated.to_string_lossy().into_owned();
    (text != raw).then_some(text)
}

/// Copy one snapshot entry, file or directory, and say nothing when it is absent —
/// an operator with no `prompts/` is an ordinary operator.
fn copy_into(from: &Path, to: &Path) -> Result<(), String> {
    let Ok(meta) = std::fs::symlink_metadata(from) else {
        return Ok(());
    };
    if meta.is_file() {
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        std::fs::copy(from, to).map_err(|e| format!("copy {}: {e}", from.display()))?;
        return Ok(());
    }
    if !meta.is_dir() {
        // A symlink to somewhere outside the operator's installation is a reach
        // this snapshot does not make on a pane's behalf.
        return Ok(());
    }
    std::fs::create_dir_all(to).map_err(|e| format!("create {}: {e}", to.display()))?;
    let entries =
        std::fs::read_dir(from).map_err(|e| format!("read {}: {e}", from.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("read {}: {e}", from.display()))?;
        copy_into(&entry.path(), &to.join(entry.file_name()))?;
    }
    Ok(())
}

// --- the keys FLEETOR writes ---------------------------------------------------

/// **Checkpoint 4's `seed_keys`, resolved against this pane's own directory** —
/// the second reader this ticket gives the spec, after the three key lists.
///
/// Every one of them is set to the pane's own `CODEX_HOME`, which is what makes it
/// one function rather than a table: for this harness the seed keys are *paths
/// that must be the pane's*, and a later key that is not one is a sign it belongs
/// on a checkpoint key list instead.
///
/// **Both are checkpoint 6 wearing checkpoint 4's clothes**, which is why they are
/// this ticket's and not another's: `sqlite_home` is where the pane's thread
/// history and state stores land, and `log_dir` is where its logs do. Codex
/// defaults both to `CODEX_HOME` — but an operator who has pointed either
/// somewhere shared has, without knowing it, put four panes' thread history in one
/// file, which is exactly the arrangement C6 rejected and the one that would make
/// checkpoint 13's harvest a query instead of a read.
///
/// **Stated at the level it was measured at.** Both are real `ConfigToml` fields,
/// read off the vendor binary's own schema, and a seed carrying them is accepted
/// by `codex-cli 0.153.4` (`tests/vendor_binary_tier.rs`). That the vendor
/// *relocates* its stores when they are set is the vendor's documented meaning
/// and is **not** separately measured here — no zero-token instrument was found
/// that opens the thread store. #40 reads that store for the gauge and is the
/// ticket that will observe it either way.
///
/// They are absolute, and that is not decoration: an absolute path is the only
/// spelling that cannot resolve against the pane's private `HOME`.
fn isolation_keys(spec: &'static HarnessSpec, pane_dir: &Path) -> Vec<(&'static str, String)> {
    let here = pane_dir.to_string_lossy().into_owned();
    spec.config_dir.seed_keys.iter().map(|key| (*key, here.clone())).collect()
}

/// **Checkpoint 2, written** (#27, C3): the pane's brief beside its seed, and the
/// absolute path the carrier key is set to.
///
/// The brief itself is not this module's — it is `fleetor-core::brief`'s rendered
/// `orch.md` / `worker.md`, byte-identical to what a Claude Code pane of the same
/// seat is handed, because D-042 holds across every harness and only the carrier
/// varies. What is this module's is *where it lands and how it is named*, and the
/// answer is the pane's own `CODEX_HOME`: nothing is written into the pane's
/// checkout, so there is no `.git/info/exclude` line and no story about keeping
/// `git status` honest. That whole class of cost is cursor's alone (M5).
///
/// **An empty brief is refused rather than installed**, for the reason
/// [`refuse_home_relative`] exists: a codex pane whose carrier key is absent runs
/// on the vendor's built-in 17,730-character prompt — it renders a prompt, accepts
/// a paste and answers, having never been told it is part of a fleet. That is this
/// arc's signature failure and it is invisible from the outside, so the last place
/// that can see it says so.
///
/// Temp file and rename, for `install`'s reason one line down: the vendor reads
/// this file at startup and again after a `/clear` (C37), and a torn brief is a
/// pane briefed with half a document.
fn install_brief(dir: &Path, brief: &str) -> Result<String, String> {
    if brief.trim().is_empty() {
        return Err(format!(
            "the codex seed for {} carries no brief. The carrier is a config key naming a \
             file, so an absent brief is not a pane with a shorter prompt — it is a pane \
             running the vendor's own, which looks perfectly healthy and has never been \
             told it is part of a fleet",
            dir.display()
        ));
    }
    let at = dir.join(BRIEF_FILE);
    let tmp = at.with_extension("md.tmp");
    std::fs::write(&tmp, brief).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &at).map_err(|e| format!("install {}: {e}", at.display()))?;
    Ok(at.to_string_lossy().into_owned())
}

/// **The one reader of the three key lists** (C36), in checkpoint order.
///
/// Checkpoint 3's sandbox posture, checkpoint 5's provider wiring and checkpoint
/// 8's reachability lever are three lists of the same shape and one question —
/// *where does this harness's configuration get written* — and they are answered
/// together here so they cannot disagree. All three are empty on `codex` today
/// and each names its ticket on [`CODEX_SPEC`]; the mechanism is what this ticket
/// owes them, not the values.
///
/// A later key list is written by adding rows to the spec. Nothing here changes.
fn checkpoint_keys(spec: &'static HarnessSpec) -> Vec<(&'static str, &'static str)> {
    spec.posture
        .sandbox_keys
        .iter()
        .chain(spec.credentials.provider_keys)
        .chain(spec.outbound.reachability_keys)
        .copied()
        .collect()
}

/// The two of the three that are *containment* — checkpoint 3's fence and
/// checkpoint 8's socket lever — for the sentence that names them (#29).
///
/// Split out from [`checkpoint_keys`] when checkpoint 5 filled its list, because
/// the two lists answer different questions and the unconditional notice quotes
/// only one of them: `sandbox_mode` is what a pane may do, and
/// `model_providers.fleetor.base_url` is who it talks to. Reciting the provider
/// entry under the word "containment" would be a sentence that is wrong about the
/// most consequential row in it.
///
/// The writer still reads all three in one pass, which is C36's whole point. This
/// is a reader for the notice and nothing else.
fn containment_keys(spec: &'static HarnessSpec) -> Vec<(&'static str, &'static str)> {
    spec.posture.sandbox_keys.iter().chain(spec.outbound.reachability_keys).copied().collect()
}

/// A key list's value, by **codex's own documented rule** for `-c key=value`:
/// parsed as TOML, and the raw string used as a literal when that fails.
///
/// So `"true"` is the boolean, `"4096"` is the integer, `'["a","b"]'` is the
/// array, and `workspace-write` — which is not valid TOML on its own — is the
/// string. A ticket filling one of the three lists writes exactly what it would
/// have typed after `-c`, which is the form every one of those keys was measured
/// through in `codex-spike-notes.md`.
fn toml_value(raw: &str) -> Value {
    raw.parse::<Value>().map(|v| v.decorated(" ", "")).unwrap_or_else(|_| Value::from(raw))
}

// --- what the operator is told (#28, C21, story 23) ----------------------------

/// Why the two isolation keys are FLEETOR's, in the operator's own terms.
const ISOLATION_WHY: &str =
    "a pane's thread history and logs are its own — a shared store puts four panes' \
     history in one file, which is the arrangement the fenced config directory exists to \
     prevent";

/// Why the sandbox trio is FLEETOR's on **every** codex pane.
const CONTAINMENT_WHY: &str =
    "a codex pane's containment is the fleet's on every seat: a pane FLEETOR did not fence \
     is not a worker, and one whose sandbox cannot reach the fleet socket is a pane that \
     looks alive and cannot talk";

/// Why a worker's provider is FLEETOR's, and the one seat it is not (#29, C9).
///
/// The sentence an operator most needs is the *cost*, so it is stated rather than
/// implied: this pane cannot spend their plan. C21's rule is that every override
/// gets a feed line, and this is the override most likely to be read as a bug —
/// an operator whose codex is pointed at their own provider will otherwise find a
/// worker talking to a different endpoint with no explanation anywhere.
const CREDENTIAL_WHY: &str =
    "a worker holds the fleet's credential and never yours (D-062). This pane runs on \
     FLEETOR's own provider entry, authenticated with the fleet's key from `.env`, so your \
     codex login stays out of a fenced pane and a worker cannot spend your plan. Your \
     orchestrator keeps the provider and the login you configured — it is your own pane";

/// Why the four features are off, and the one seat they are not off on.
const FEATURES_WHY: &str =
    "the sandbox bounds the filesystem and the network, and it does not bound a pane \
     driving a browser or a desktop, or one fanning out into threads the run manifest \
     never sees. Your orchestrator keeps this flag — it is your own pane";

/// The pane this seed is for, by name.
///
/// **The configuration directory's own last segment**, which is the pane's name by
/// construction: [`Layout::pane_config`](crate::placement::Layout::pane_config) is
/// `pane-config/<pane>`. The seeder is deliberately not handed a `PaneId` as well
/// as [`Seed::operators_own_seat`] — the seat is *one* question, and two fields
/// answering it is two chances for them to disagree. The full path is the fallback
/// because a label that could be empty is worse than a long one.
fn seat_label(dir: &Path) -> String {
    dir.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.display().to_string())
}

/// Set one key FLEETOR owns, and say so **only if it replaced an answer the
/// operator actually gave**.
///
/// This is [`crate::prompts`]'s rule in the seeder: an override that loads is
/// announced, and an ordinary machine with nothing to say stays quiet. The
/// condition is what makes the line worth reading — "my setting did not apply" is
/// a mystery only for an operator who had a setting, and a feed that says the same
/// nine things every run is a feed nobody reads the tenth line of.
///
/// A **`Warn`** rather than an `Info`, and that is the same judgement `prompts.rs`
/// makes when it refuses a broken override: the operator wrote something down and
/// it is not in effect. Nothing is wrong, but they are owed the sentence.
fn set_owned(
    doc: &mut DocumentMut,
    segments: &[&str],
    value: Value,
    seat: &str,
    why: &str,
    notices: &mut Vec<(NoticeLevel, String)>,
) {
    let before = value_at(doc, segments);
    let after = rendered(&value);
    set_path(doc, segments, value);
    let Some(before) = before.filter(|before| *before != after) else {
        return;
    };
    notices.push((
        NoticeLevel::Warn,
        format!(
            "{seat}: your codex `{}` = {before} did not apply — FLEETOR sets it to {after}, \
             because {why}.",
            segments.join("."),
        ),
    ));
}

/// The one line every codex pane gets, whether or not it overrode anything.
///
/// **Unconditional, and the four features are why.** Codex's 47 feature flags are
/// on by *default* rather than by a line in anyone's `config.toml`, so
/// [`set_owned`] has nothing to compare against and would say nothing at all —
/// yet turning four of them off is the most consequential thing this seeder does
/// to a pane. An operator debugging why a codex worker will not drive a browser
/// deserves to find the answer in the feed rather than in this file.
///
/// The containment half is read back off the spec rather than spelled here, so a
/// row added to [`Posture::sandbox_keys`] or [`Outbound::reachability_keys`]
/// appears in the sentence without anyone remembering to edit it.
fn narrowing_notice(
    spec: &'static HarnessSpec,
    seat: &str,
    operators_own_seat: bool,
) -> (NoticeLevel, String) {
    let containment = containment_keys(spec)
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(", ");
    let tail = if operators_own_seat {
        "Your codex provider, login and feature flags travel with this seat — it is your own \
         pane, and it runs on your own credential. The snapshot is still never written back, \
         so nothing here reaches your `~/.codex`."
            .to_string()
    } else {
        format!(
            "This pane runs on FLEETOR's own provider and the fleet's key, never yours, so it \
             cannot spend your plan. Four default-on features are off here — {} — because the \
             sandbox does not bound a pane driving a browser or a desktop, or one fanning out \
             into threads the run manifest never sees.",
            WORKER_FEATURE_OVERRIDES
                .iter()
                .map(|(key, _)| key.trim_start_matches("features."))
                .collect::<Vec<_>>()
                .join(", "),
        )
    };
    (NoticeLevel::Info, format!("{seat}: codex containment is FLEETOR's — {containment}. {tail}"))
}

/// One value as it will read in the seeded document, for a notice that quotes it.
fn rendered(value: &Value) -> String {
    value.to_string().trim().to_string()
}

/// What is at a dotted path today, or `None` when nothing is.
fn value_at(doc: &DocumentMut, segments: &[&str]) -> Option<String> {
    let mut item: &Item = doc.as_item();
    for segment in segments {
        item = item.as_table_like()?.get(segment)?;
    }
    item.as_value().map(rendered)
}

/// Set one dotted path, creating the tables on the way down.
///
/// The segments arrive **already split**, so a key that is itself a path — the
/// project key in `projects."/Users/me/work"` is a real one — is one segment and
/// stays one segment. Splitting on `.` is the caller's decision because only the
/// caller knows whether it is holding a dotted *path* or a literal *name*.
///
/// Intermediate tables are implicit, so a document that sets only
/// `sandbox_workspace_write.network_access` renders the header rather than an
/// empty `[sandbox_workspace_write]` above it.
fn set_path(doc: &mut DocumentMut, segments: &[&str], value: Value) {
    let Some((last, parents)) = segments.split_last() else {
        return;
    };
    let mut table: &mut Table = doc.as_table_mut();
    for segment in parents {
        let entry = table.entry(segment).or_insert_with(|| {
            let mut fresh = Table::new();
            fresh.set_implicit(true);
            Item::Table(fresh)
        });
        if !entry.is_table() {
            *entry = Item::Table(Table::new());
        }
        table = entry.as_table_mut().expect("just made it a table");
    }
    table.insert(last, Item::Value(value));
}

// --- the refusal ---------------------------------------------------------------

/// **The measured trap, refused at the point of production** (C6).
///
/// A `~` inside a configuration value resolves against the pane's private `HOME`
/// rather than the operator's, and the pane dies at spawn with `Error loading
/// configuration: No such file or directory`. Loud — but only if something is
/// looking, and the seeder is the last place that can look before the pane exists.
///
/// This is a guard rather than a test because the two are not the same promise: a
/// test says *the seed we happened to produce today was clean*, and this says *no
/// seed that is not clean is ever installed*. A tilde reaching here means an
/// operator value the rewrite did not know how to resolve — no operator `HOME` on
/// this machine, say — and a refused placement naming the key is a better morning
/// than a pane that boots into an error the operator has to read the vendor's
/// source to understand.
fn refuse_home_relative(doc: &DocumentMut) -> Result<(), String> {
    let mut offenders = BTreeMap::new();
    collect_home_relative(doc.as_item(), &mut String::new(), &mut offenders);
    match offenders.into_iter().next() {
        None => Ok(()),
        Some((key, value)) => Err(format!(
            "the seeded codex configuration would carry {key} = {value:?}, whose leading `~` \
             resolves against the pane's private HOME rather than the operator's — the pane \
             would die at spawn with a configuration-load error"
        )),
    }
}

fn collect_home_relative(item: &Item, at: &mut String, found: &mut BTreeMap<String, String>) {
    match item {
        Item::Value(value) => collect_home_relative_value(value, at, found),
        Item::Table(table) => {
            for (key, child) in table.iter() {
                descend(at, key, |at| collect_home_relative(child, at, found));
            }
        }
        Item::ArrayOfTables(tables) => {
            for (index, table) in tables.iter().enumerate() {
                for (key, child) in table.iter() {
                    descend(at, &format!("{index}.{key}"), |at| {
                        collect_home_relative(child, at, found)
                    });
                }
            }
        }
        Item::None => {}
    }
}

fn collect_home_relative_value(
    value: &Value,
    at: &mut String,
    found: &mut BTreeMap<String, String>,
) {
    match value {
        Value::String(text) => {
            let raw = text.value();
            if raw == "~" || raw.starts_with("~/") {
                found.insert(at.clone(), raw.to_string());
            }
        }
        Value::Array(array) => {
            for (index, element) in array.iter().enumerate() {
                descend(at, &index.to_string(), |at| {
                    collect_home_relative_value(element, at, found)
                });
            }
        }
        Value::InlineTable(table) => {
            for (key, element) in table.iter() {
                descend(at, key, |at| collect_home_relative_value(element, at, found));
            }
        }
        _ => {}
    }
}

fn descend(at: &mut String, key: &str, body: impl FnOnce(&mut String)) {
    let was = at.len();
    if !at.is_empty() {
        at.push('.');
    }
    at.push_str(key);
    body(at);
    at.truncate(was);
}

// --- walking the document ------------------------------------------------------

fn rewrite_item(item: &mut Item, relocate: &impl Fn(&str) -> Option<String>) {
    match item {
        Item::Value(value) => rewrite_value(value, relocate),
        Item::Table(table) => {
            for (_, child) in table.iter_mut() {
                rewrite_item(child, relocate);
            }
        }
        Item::ArrayOfTables(tables) => {
            for table in tables.iter_mut() {
                for (_, child) in table.iter_mut() {
                    rewrite_item(child, relocate);
                }
            }
        }
        Item::None => {}
    }
}

fn rewrite_value(value: &mut Value, relocate: &impl Fn(&str) -> Option<String>) {
    match value {
        Value::String(text) => {
            if let Some(new) = relocate(text.value()) {
                let decor = text.decor().clone();
                let mut replacement = Value::from(new);
                *replacement.decor_mut() = decor;
                *value = replacement;
            }
        }
        Value::Array(array) => {
            for element in array.iter_mut() {
                rewrite_value(element, relocate);
            }
        }
        Value::InlineTable(table) => {
            for (_, element) in table.iter_mut() {
                rewrite_value(element, relocate);
            }
        }
        _ => {}
    }
}

// --- checkpoints 4, 6 and 14, directly ------------------------------------------

/// **Codex's own tests, not a conformance pass** (C25).
///
/// The conformance suite is one pass per *registered* harness and codex is not
/// registered — #33 is the single moment it joins the registry, because a
/// half-implemented harness that compiles quietly is the failure story 55 names
/// and registering early turns the suite red between phase-2 tickets. So this
/// checkpoint lands with a direct test of its own, which is the pattern every
/// phase-2 ticket follows.
///
/// **Nothing here touches the operator's real `~/.codex`.** Every test fabricates
/// an operator installation under a scratch root and hands its path down on
/// [`Seed::operator_home`] — which is possible at all because that value arrives
/// from [`Host`](crate::placement::Host) rather than being read from the process,
/// and is the mechanism half of "the operator's installation is never written
/// to". [`the_operators_installation_is_never_written_to`] is the assertion half.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::placement::harness::registered;

    /// What a pane is briefed with when a test is not about the brief. Short and
    /// recognisable rather than a rendered `worker.md` — the tests that care that
    /// it is the *real* brief render one and say so.
    const A_BRIEF: &str = "SENTINEL-BRIEF\nYou are a FLEETOR worker pane.";

    /// One fabricated machine: an operator installation, a pane's config dir, and
    /// a pane cwd, all under one scratch root that is removed on drop.
    struct Machine {
        root: PathBuf,
    }

    impl Machine {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "fleetor-codex-{name}-{}-{:?}",
                fleetor_core::time::now_ms(),
                std::thread::current().id()
            ));
            std::fs::create_dir_all(root.join("operator").join(OPERATOR_DIR)).expect("operator");
            std::fs::create_dir_all(root.join("work")).expect("cwd");
            Self { root }
        }

        fn operator_home(&self) -> PathBuf {
            self.root.join("operator")
        }

        fn operator_dir(&self) -> PathBuf {
            self.operator_home().join(OPERATOR_DIR)
        }

        /// Write one file into the fabricated operator installation.
        fn operator_file(&self, rel: &str, body: &str) -> PathBuf {
            let at = self.operator_dir().join(rel);
            std::fs::create_dir_all(at.parent().expect("a parent")).expect("operator subdir");
            std::fs::write(&at, body).expect("operator file");
            at
        }

        fn pane_dir(&self, name: &str) -> PathBuf {
            self.root.join("panes").join(name)
        }

        fn cwd(&self) -> PathBuf {
            self.root.join("work")
        }

        /// Seed one pane the way `place` does.
        fn seed(&self, pane: &str) -> Result<PathBuf, String> {
            self.seed_at(pane, &self.cwd())
        }

        /// The Activity feed lines one ordinary (fenced) seat's seeding produced.
        fn notices(&self, pane: &str) -> Vec<(NoticeLevel, String)> {
            let dir = self.pane_dir(pane);
            let home = self.operator_home();
            codex()
                .seed_config_dir(&Seed::new(&dir, &self.cwd(), Some(&home)).with_brief(A_BRIEF))
                .expect("seed")
        }

        /// The same, for the operator's own seat.
        fn notices_for_the_operator(&self, pane: &str) -> Vec<(NoticeLevel, String)> {
            let dir = self.pane_dir(pane);
            let home = self.operator_home();
            codex()
                .seed_config_dir(
                    &Seed::new(&dir, &self.cwd(), Some(&home))
                        .with_brief(A_BRIEF)
                        .for_the_operator(),
                )
                .expect("seed")
        }

        /// Seed one pane as the operator's own — the orchestrator's arm.
        fn seed_for_the_operator(&self, pane: &str) -> Result<PathBuf, String> {
            self.notices_for_the_operator(pane);
            Ok(self.pane_dir(pane))
        }

        fn seed_at(&self, pane: &str, cwd: &Path) -> Result<PathBuf, String> {
            self.seed_briefed(pane, cwd, A_BRIEF)
        }

        /// The same seeding, with this pane's brief spelled out — the seam #27
        /// added, and the reason every other test here can stay about the
        /// snapshot: a codex seed with no brief is refused, so the brief is
        /// stated once here rather than at fourteen call sites.
        fn seed_briefed(&self, pane: &str, cwd: &Path, brief: &str) -> Result<PathBuf, String> {
            let dir = self.pane_dir(pane);
            let home = self.operator_home();
            codex().seed_config_dir(&Seed::new(&dir, cwd, Some(&home)).with_brief(brief))?;
            Ok(dir)
        }

        /// The pane's installed brief.
        fn brief(&self, pane: &str) -> String {
            std::fs::read_to_string(self.pane_dir(pane).join(BRIEF_FILE))
                .expect("the pane's brief file")
        }

        /// The seeded `config.toml`, parsed.
        fn seeded(&self, pane: &str) -> DocumentMut {
            std::fs::read_to_string(self.pane_dir(pane).join(CONFIG_FILE))
                .expect("the seed file")
                .parse()
                .expect("the seeded config is valid TOML")
        }
    }

    impl Drop for Machine {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.root).ok();
        }
    }

    /// Every file under `at`, by relative path, with its bytes — the handle a test
    /// needs to say "this directory did not change".
    fn contents(at: &Path) -> BTreeMap<String, Vec<u8>> {
        let mut found = BTreeMap::new();
        fn walk(at: &Path, prefix: &Path, into: &mut BTreeMap<String, Vec<u8>>) {
            let Ok(entries) = std::fs::read_dir(at) else { return };
            for entry in entries.flatten() {
                let path = entry.path();
                let rel = path.strip_prefix(prefix).expect("under the prefix");
                if path.is_dir() {
                    into.insert(format!("{}/", rel.display()), Vec::new());
                    walk(&path, prefix, into);
                } else {
                    into.insert(
                        rel.display().to_string(),
                        std::fs::read(&path).unwrap_or_default(),
                    );
                }
            }
        }
        walk(at, at, &mut found);
        found
    }

    /// Every string value in a document, keyed by its dotted path.
    fn strings(doc: &DocumentMut) -> BTreeMap<String, String> {
        fn walk(item: &Item, at: &mut String, into: &mut BTreeMap<String, String>) {
            match item {
                Item::Value(Value::String(s)) => {
                    into.insert(at.clone(), s.value().to_string());
                }
                Item::Value(Value::InlineTable(t)) => {
                    for (key, value) in t.iter() {
                        descend(at, key, |at| {
                            walk(&Item::Value(value.clone()), at, into);
                        });
                    }
                }
                Item::Table(t) => {
                    for (key, child) in t.iter() {
                        descend(at, key, |at| walk(child, at, into));
                    }
                }
                _ => {}
            }
        }
        let mut found = BTreeMap::new();
        walk(doc.as_item(), &mut String::new(), &mut found);
        found
    }

    /// The operator's real installation, spelled the way it actually is on the
    /// machine this was written on — every arm below seeds from a fabricated copy
    /// of this, never from `~/.codex` itself.
    const OPERATOR_CONFIG: &str = r#"
model = "deepseek-v4-flash"
model_provider = "deepseek"
model_catalog_json = "~/.codex/models.json"
notify = "~/bin/tell-me.sh"

[projects."/Users/operator/harness"]
trust_level = "trusted"

[model_providers.deepseek]
name = "deepseek"
base_url = "https://api.deepseek.com/"
wire_api = "responses"
experimental_bearer_token = "sk-OPERATORS-OWN-KEY"
env_key = "OPERATORS_SHELL_KEY"

[mcp_servers.notes]
command = "note-server"
args = ["--root", "~/notes"]
"#;

    /// A fabricated operator installation carrying everything the snapshot has an
    /// opinion about: preferences, the catalog, skills, and the three things that
    /// must not travel.
    fn a_full_installation(machine: &Machine) {
        machine.operator_file(CONFIG_FILE, OPERATOR_CONFIG);
        machine.operator_file("models.json", r#"{"models":[{"id":"deepseek-v4-flash"}]}"#);
        machine.operator_file("skills/reviewing/SKILL.md", "how this operator reviews");
        machine.operator_file("prompts/handoff.md", "the operator's own prompt");
        // The three that must not.
        machine.operator_file("auth.json", r#"{"OPENAI_API_KEY":"sk-OPERATORS-OWN-KEY"}"#);
        machine.operator_file("thread_history_1.sqlite", "SQLite format 3\u{0}");
        machine.operator_file("packages/standalone/current/bin/codex", "a 275 MB binary");
    }

    #[test]
    fn codex_is_implemented_and_still_not_registered() {
        // Phase 1's exit condition holds through every phase-2 ticket: the
        // conformance suite is green with exactly one registered harness, and #33
        // is the single moment that changes (C25). `grep -rn "codex" src/` now
        // finds this module; the registry still does not.
        assert_eq!(registered().len(), 1);
        assert_eq!(registered()[0].spec().name, "claude-code");
        assert!(
            !registered().iter().any(|h| h.spec().name == CODEX_SPEC.name),
            "codex must not join the registry before #33",
        );
        assert_eq!(codex().spec(), &CODEX_SPEC);
    }

    #[test]
    fn the_snapshot_carries_the_preferences_and_leaves_the_credential_behind() {
        // Checkpoint 6: a layered snapshot of the operator's own, minus the two
        // things C9 says a worker never holds — the credential file and the
        // provider's bearer token — and minus the state stores whose per-pane
        // freshness is the whole secondary reason for a private CODEX_HOME.
        let machine = Machine::new("snapshot");
        a_full_installation(&machine);
        let dir = machine.seed("worker-1").expect("seed");

        assert!(dir.join("models.json").is_file(), "the model catalog travels");
        assert!(dir.join("skills/reviewing/SKILL.md").is_file(), "skills travel");
        assert!(dir.join("prompts/handoff.md").is_file(), "custom prompts travel");

        let seeded = machine.seeded("worker-1");
        assert_eq!(
            seeded["mcp_servers"]["notes"]["command"].as_str(),
            Some("note-server"),
            "MCP settings travel — they live in the same document",
        );
        assert_eq!(
            seeded["model"].as_str(),
            Some("deepseek-v4-flash"),
            "preference-bearing config travels",
        );

        assert!(!dir.join("auth.json").exists(), "the operator's credential file does not");
        assert!(!dir.join("thread_history_1.sqlite").exists(), "nor a WAL-mode state store");
        assert!(!dir.join("packages").exists(), "nor 275 MB of vendor binaries");

        let provider = &seeded["model_providers"]["deepseek"];
        assert_eq!(provider["base_url"].as_str(), Some("https://api.deepseek.com/"));
        assert!(provider.get("experimental_bearer_token").is_none(), "the bearer token is struck");
        assert!(provider.get("env_key").is_none(), "and so is the pointer at the operator's shell");

        for (rel, bytes) in contents(&dir) {
            let text = String::from_utf8_lossy(&bytes);
            assert!(
                !text.contains("sk-OPERATORS-OWN-KEY"),
                "the operator's key reached the pane through {rel}",
            );
        }
    }

    #[test]
    fn the_operators_own_seat_carries_the_login_a_fenced_seat_is_denied() {
        // **The asymmetry the arc applies at every layer, at the snapshot** (#44,
        // C9, C43). One installation, two seats, and the difference between them
        // is `Seed::operators_own_seat` and nothing else:
        //
        //  - the orchestrator is the operator's own pane on their own login, which
        //    is the entire point of the seat (D-030, D-052, C9, C15) — and a codex
        //    login lives *inside* the directory this module replaces (C6), so a
        //    seat that does not carry it in has none at all;
        //  - the worker is unchanged, and the test above is what says so: it scans
        //    every byte under the pane directory for the operator's key.
        let machine = Machine::new("seat-credential");
        a_full_installation(&machine);
        let worker = machine.seed("worker-1").expect("seed");
        let orch = machine.seed_for_the_operator("orch").expect("seed");

        assert_eq!(
            std::fs::read_to_string(orch.join("auth.json")).ok().as_deref(),
            Some(r#"{"OPENAI_API_KEY":"sk-OPERATORS-OWN-KEY"}"#),
            "the operator's own pane arrived with no login, which is what #44 is",
        );
        assert!(
            !worker.join("auth.json").exists(),
            "a fenced pane holds the fleet's credential and never the operator's (D-062)",
        );

        // The inherited provider keeps the credential that makes it usable — the
        // third auth shape C14 names, and the operator's own installation is that
        // shape (a third-party endpoint with its own bearer token).
        let inherited = machine.seeded("orch");
        let provider = &inherited["model_providers"]["deepseek"];
        assert_eq!(
            provider["experimental_bearer_token"].as_str(),
            Some("sk-OPERATORS-OWN-KEY"),
            "the orchestrator's inherited provider was struck of the key it authenticates with",
        );
        assert_eq!(
            provider["env_key"].as_str(),
            Some("OPERATORS_SHELL_KEY"),
            "and of the variable the operator's own shell exports it in — that seat is not \
             scrubbed, so the name still resolves there",
        );
        assert_eq!(
            inherited["model_provider"].as_str(),
            Some("deepseek"),
            "the orchestrator inherits and displays its provider; there is no picker (C2, C9)",
        );

        // The FLEETOR entry still lands on every codex pane, orchestrator included:
        // a provider nothing selects is inert (C41(a), C42(b)), and this ticket
        // changes which credential a seat holds, not which entries it carries.
        assert_eq!(
            inherited["model_providers"][FLEET_PROVIDER]["env_key"].as_str(),
            Some(FLEET_KEY_ENV),
        );
    }

    #[test]
    fn no_seeded_value_resolves_against_the_panes_private_home() {
        // **The measured trap** (C6). `model_catalog_json = "~/.codex/models.json"`
        // is the operator's real value, and a `~` resolves against the *pane's*
        // private HOME at spawn: the pane dies with a configuration-load error
        // naming a file inside a home that has no `.codex` in it. Reproduced
        // against the real binary in `tests/vendor_binary_tier.rs`.
        let machine = Machine::new("tilde");
        a_full_installation(&machine);
        let dir = machine.seed("worker-1").expect("seed");
        let seeded = machine.seeded("worker-1");

        for (key, value) in strings(&seeded) {
            assert!(
                value != "~" && !value.starts_with("~/"),
                "the seeded configuration carries {key} = {value:?}, which resolves against \
                 the pane's private HOME",
            );
        }

        // The catalog was carried in, so the value names the pane's own copy —
        // which is how "never written back" survives a vendor that writes through
        // one of its own configured paths.
        assert_eq!(
            seeded["model_catalog_json"].as_str(),
            Some(dir.join("models.json").to_string_lossy().as_ref()),
        );
        // A tilde pointing outside the installation is expanded against the
        // operator's real HOME instead. It is a read the vendor was always going
        // to do, and an absolute path is the only spelling that cannot resolve
        // against the pane's home.
        assert_eq!(
            seeded["notify"].as_str(),
            Some(machine.operator_home().join("bin/tell-me.sh").to_string_lossy().as_ref()),
        );
        // Nested values too — an argument vector inside an MCP server entry is
        // exactly the place a rewrite that only walks the top level would miss.
        assert_eq!(
            seeded["mcp_servers"]["notes"]["args"][1].as_str(),
            Some(machine.operator_home().join("notes").to_string_lossy().as_ref()),
        );
    }

    #[test]
    fn a_home_relative_path_is_refused_rather_than_installed() {
        // The guard is not the same promise as the test above it. That one says
        // the seed produced today was clean; this says an unclean one is never
        // installed — and the file that was already there survives the refusal,
        // because the rename is the last thing that happens.
        let machine = Machine::new("refusal");
        a_full_installation(&machine);
        let dir = machine.seed("worker-1").expect("the first seed");
        let installed = dir.join(CONFIG_FILE);
        let good = std::fs::read_to_string(&installed).expect("the installed seed");

        // A tilde arrives in the merge base — which is where one can still arrive,
        // since the pane's own vendor writes to this file.
        std::fs::write(&installed, format!("{good}\nmodel_instructions_file = \"~/brief.md\"\n"))
            .expect("the pane's vendor wrote to its own config");
        let refused = machine.seed("worker-1").expect_err("a tilde must refuse");
        assert!(refused.contains("model_instructions_file"), "the refusal names the key: {refused}");
        assert!(refused.contains("private HOME"), "and says why: {refused}");
        assert!(
            std::fs::read_to_string(&installed).expect("still there").contains("~/brief.md"),
            "a refused seed installs nothing, so what was there is what is there",
        );
    }

    #[test]
    fn the_operators_installation_is_never_written_to() {
        // Checkpoint 6's promise, asserted by mechanism and by observation. The
        // mechanism is that seeding is handed `Seed::operator_home` as a value, so
        // a test can point it at a fabricated installation and the code has no
        // other way to find one — there is no `std::env::var("HOME")` in this
        // module. The observation is that four panes' worth of seeding leaves that
        // installation byte for byte identical.
        let machine = Machine::new("read-only");
        a_full_installation(&machine);
        let before = contents(&machine.operator_dir());
        assert!(!before.is_empty(), "the fabricated installation has something in it");

        for slot in 1..=4 {
            machine.seed(&format!("worker-{slot}")).expect("seed");
        }
        // **And the seat that carries the operator's login** (#44, C43). That seat
        // copies `auth.json` *in*, which is the one arm of this module that touches
        // a credential file at all — so it is the arm most able to turn C6's
        // one-way snapshot into a two-way one without anyone noticing.
        machine.seed_for_the_operator("orch").expect("seed");
        assert_eq!(
            before,
            contents(&machine.operator_dir()),
            "seeding wrote into the operator's own installation",
        );
    }

    #[test]
    fn every_pane_gets_its_own_directory_and_its_own_state_store() {
        // The two seed keys are checkpoint 6 wearing checkpoint 4's clothes: they
        // pin the pane's state store and logs inside its own CODEX_HOME, so an
        // operator who has pointed `sqlite_home` somewhere shared does not put
        // four panes' thread history in one file — the arrangement C6 rejected,
        // and the one that would make checkpoint 13's harvest a query.
        let machine = Machine::new("isolation");
        machine.operator_file(CONFIG_FILE, "sqlite_home = \"/tmp/one-store-for-everyone\"\n");
        let one = machine.seed("worker-1").expect("seed");
        let two = machine.seed("worker-2").expect("seed");

        for (pane, dir) in [("worker-1", &one), ("worker-2", &two)] {
            let seeded = machine.seeded(pane);
            for key in CODEX_SPEC.config_dir.seed_keys {
                let value = seeded[*key].as_str().unwrap_or_default().to_string();
                assert_eq!(value, dir.to_string_lossy(), "{pane}: {key} is the pane's own");
                assert!(Path::new(&value).is_absolute(), "{pane}: {key} is absolute");
            }
        }
        assert_ne!(one, two, "two panes, two directories");
    }

    #[test]
    fn the_trust_record_is_keyed_by_the_project_key_and_says_yes_as_a_string() {
        // Checkpoint 14, both halves. The affirmative answer is `"trusted"` and not
        // `true` — which is why `trust_keys` names keys and there is no
        // `seed_values` field for a value that is one vendor's (C31).
        let machine = Machine::new("trust");
        a_full_installation(&machine);
        machine.seed("orch").expect("seed");

        let key = codex().project_key(&machine.cwd());
        assert_eq!(key, std::fs::canonicalize(machine.cwd()).expect("resolved").to_string_lossy());

        let seeded = machine.seeded("orch");
        for name in CODEX_SPEC.project_identity.trust_keys {
            assert_eq!(seeded["projects"][&key][*name].as_str(), Some(TRUST_AFFIRMATIVE));
        }
        // A project key is a path, and a path is one key rather than a dotted one.
        // Round-tripping the document is what proves the quoting is right.
        assert!(
            seeded.to_string().contains(&format!("[projects.{key:?}]")),
            "the project key is quoted as one key:\n{seeded}",
        );
    }

    #[test]
    fn switching_targets_keeps_the_first_targets_trust_row() {
        // `seed_merges` means the same thing here it does for Claude Code: a fleet
        // pointed at a new target re-seeds every pane, and a seed that rebuilds
        // from the operator's snapshot each time works exactly once.
        let machine = Machine::new("merge");
        a_full_installation(&machine);
        let first = machine.cwd();
        machine.seed_at("worker-1", &first).expect("the first target");

        let second = machine.root.join("other-repo");
        std::fs::create_dir_all(&second).expect("a second target");
        machine.seed_at("worker-1", &second).expect("the second target");

        let seeded = machine.seeded("worker-1");
        for cwd in [&first, &second] {
            let key = codex().project_key(cwd);
            assert_eq!(
                seeded["projects"][&key]["trust_level"].as_str(),
                Some(TRUST_AFFIRMATIVE),
                "switching targets clobbered {}",
                cwd.display(),
            );
        }
        // And the operator's own row is still there, inherited with the rest.
        assert_eq!(
            seeded["projects"]["/Users/operator/harness"]["trust_level"].as_str(),
            Some(TRUST_AFFIRMATIVE),
        );
    }

    #[test]
    fn a_machine_with_no_operator_installation_still_gets_a_working_seed() {
        // `Host::operator_home` is `Option` because a machine nobody looked at is a
        // real machine. A pane with the fleet's keys and none of the operator's
        // preferences is a working pane; refusing to place would be a fleet that
        // cannot start because a preference file is missing.
        let machine = Machine::new("bare");
        let dir = machine.pane_dir("worker-1");
        let cwd = machine.cwd();
        codex()
            .seed_config_dir(&Seed::new(&dir, &cwd, None).with_brief(A_BRIEF))
            .expect("seed against nothing");

        let seeded: DocumentMut = std::fs::read_to_string(dir.join(CONFIG_FILE))
            .expect("a seed file all the same")
            .parse()
            .expect("valid TOML");
        assert_eq!(
            seeded["projects"][codex().project_key(&cwd)]["trust_level"].as_str(),
            Some(TRUST_AFFIRMATIVE),
        );
    }

    // --- checkpoints 3 and 8, directly (#28; C5, C7, C21) -----------------------

    /// Every `(key, value)` FLEETOR writes into a seat's own document, whichever
    /// list it came from — the handle a containment test needs, so a row that
    /// stopped being written cannot pass by being asserted somewhere else.
    fn fleetor_owned(seat_is_the_operators: bool) -> Vec<(String, String)> {
        let mut rows: Vec<(String, String)> = checkpoint_keys(&CODEX_SPEC)
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        if !seat_is_the_operators {
            rows.extend(
                WORKER_PROVIDER_SELECTION
                    .iter()
                    .chain(WORKER_FEATURE_OVERRIDES)
                    .map(|(k, v)| (k.to_string(), v.to_string())),
            );
        }
        rows
    }

    #[test]
    fn the_sandbox_trio_lands_on_every_codex_pane() {
        // **Checkpoint 3 and checkpoint 8, which are one measurement** (C5, C7).
        // The three rows were measured together on `0.153.4` and only make sense
        // together, so they are asserted together even though two of them live on
        // `posture` and the third on `outbound`.
        //
        // What this test can and cannot prove is the whole reason
        // `tests/vendor_binary_tier.rs` has an arm of its own: this says FLEETOR
        // *wrote* the keys. Only the real seatbelt can say the vendor honoured
        // them, and a checkpoint that fails by looking healthy has to be measured
        // rather than inferred.
        let machine = Machine::new("trio");
        a_full_installation(&machine);
        machine.seed("worker-1").expect("seed");
        machine.seed("orch-seat").expect("seed");

        for pane in ["worker-1", "orch-seat"] {
            let seeded = machine.seeded(pane);
            assert_eq!(
                seeded["sandbox_mode"].as_str(),
                Some("workspace-write"),
                "{pane}: the fence itself",
            );
            assert_eq!(
                seeded["approval_policy"].as_str(),
                Some("never"),
                "{pane}: `on-request` lets the model decide when to ask a human, and a \
                 worker has no human — it would park looking perfectly healthy",
            );
            assert_eq!(
                seeded["sandbox_workspace_write"]["network_access"].as_bool(),
                Some(true),
                "{pane}: the socket lever, and `true` is the boolean rather than the word",
            );
        }
    }

    #[test]
    fn the_operators_own_sandbox_answers_are_replaced_and_the_operator_is_told() {
        // Two keys are FLEETOR's rather than inherited, and both are load-bearing
        // for messaging: an operator who enabled the sandbox months ago for
        // unrelated reasons, or narrowed its network access, would otherwise get a
        // fleet whose panes cannot talk. So the snapshot's own answers lose — and
        // the operator gets the sentence, the way `prompts.rs` announces a loaded
        // override (C21, story 23).
        let machine = Machine::new("replaced");
        machine.operator_file(
            CONFIG_FILE,
            "sandbox_mode = \"read-only\"\n\
             approval_policy = \"on-request\"\n\
             [sandbox_workspace_write]\nnetwork_access = false\n",
        );
        let notices = machine.notices("worker-1");

        let seeded = machine.seeded("worker-1");
        assert_eq!(seeded["sandbox_mode"].as_str(), Some("workspace-write"));
        assert_eq!(seeded["approval_policy"].as_str(), Some("never"));
        assert_eq!(seeded["sandbox_workspace_write"]["network_access"].as_bool(), Some(true));

        for key in ["sandbox_mode", "approval_policy", "sandbox_workspace_write.network_access"] {
            let told = notices
                .iter()
                .find(|(level, text)| *level == NoticeLevel::Warn && text.contains(key))
                .unwrap_or_else(|| panic!("nothing on the Activity feed named {key}: {notices:?}"));
            assert!(
                told.1.contains("did not apply"),
                "the line has to say the operator's answer lost, not merely mention the key: {}",
                told.1,
            );
            assert!(told.1.contains("worker-1"), "and which pane it was: {}", told.1);
        }
    }

    #[test]
    fn an_operator_who_set_none_of_it_is_not_warned_about_any_of_it() {
        // The other half of `prompts.rs`'s rule, and the one that keeps the feed
        // worth reading: "my setting did not apply" is a mystery only for an
        // operator who had a setting. A default installation gets the one Info
        // line saying what FLEETOR narrowed, and no warnings at all.
        let machine = Machine::new("quiet");
        machine.operator_file(CONFIG_FILE, "model = \"deepseek-v4-flash\"\n");
        let notices = machine.notices("worker-1");

        assert!(
            !notices.iter().any(|(level, _)| *level == NoticeLevel::Warn),
            "an operator who overrode nothing was warned anyway: {notices:?}",
        );
        assert_eq!(notices.len(), 1, "one line per pane, not one per key: {notices:?}");
    }

    #[test]
    fn the_four_features_are_off_on_a_worker_and_untouched_on_the_orchestrator() {
        // **C21, both halves.** The sandbox bounds the filesystem and the network;
        // it does not bound a pane driving a browser or a desktop, and it does not
        // bound a pane fanning out into threads the run manifest never sees — the
        // sharpest of the four, because D-029/D-030 deleted headless supervision
        // deliberately and a worker re-growing it is a regression wearing a
        // feature flag.
        //
        // The orchestrator is the operator's own pane, so it inherits their flags
        // untouched. That asymmetry is the product, and it is the only thing
        // `Seed::operators_own_seat` exists to say.
        let machine = Machine::new("features");
        a_full_installation(&machine);
        machine.seed("worker-1").expect("seed");
        machine.seed_for_the_operator("orch").expect("seed");

        let worker = machine.seeded("worker-1");
        let orch = machine.seeded("orch");
        for (key, _) in WORKER_FEATURE_OVERRIDES {
            let name = key.trim_start_matches("features.");
            assert_eq!(
                worker["features"][name].as_bool(),
                Some(false),
                "a worker can still reach past the fence through `{key}`",
            );
            assert!(
                orch.get("features").and_then(|f| f.get(name)).is_none(),
                "the orchestrator is the operator's own pane and `{key}` is theirs to set",
            );
        }
        // Four features, five rows: `browser_use` carries its full-CDP companion.
        assert_eq!(WORKER_FEATURE_OVERRIDES.len(), 5);
    }

    #[test]
    fn a_feature_the_operator_turned_on_by_hand_is_turned_off_and_announced() {
        // The features are on by *default* rather than by a line in anyone's
        // config, so `set_owned` usually has nothing to compare against and the
        // unconditional line is what carries them. An operator who wrote one down
        // is the case where both fire, and they should.
        let machine = Machine::new("explicit-feature");
        machine.operator_file(CONFIG_FILE, "[features]\nmulti_agent = true\n");
        let notices = machine.notices("worker-1");

        assert_eq!(machine.seeded("worker-1")["features"]["multi_agent"].as_bool(), Some(false));
        assert!(
            notices.iter().any(|(level, text)| *level == NoticeLevel::Warn
                && text.contains("features.multi_agent")
                && text.contains("did not apply")),
            "the operator wrote it down and it is not in effect: {notices:?}",
        );
    }

    #[test]
    fn every_codex_pane_is_told_what_fleetor_narrowed_even_with_nothing_to_override() {
        // The unconditional line, and why it is unconditional: turning four
        // default-on features off is the most consequential thing this seeder does
        // to a pane, and no operator file records a default. An operator debugging
        // why a codex worker will not drive a browser should find the answer in the
        // feed rather than in this file.
        let machine = Machine::new("narrowing");
        a_full_installation(&machine);

        let worker = machine.notices("worker-1");
        let line = worker
            .iter()
            .find(|(level, _)| *level == NoticeLevel::Info)
            .expect("every codex pane gets one");
        // The containment rows are quoted as `key=value`, read off the spec so a row
        // added to either list appears without anyone editing the sentence.
        for (key, value) in containment_keys(&CODEX_SPEC) {
            let named = format!("{key}={value}");
            assert!(line.1.contains(&named), "the line does not name {named}: {}", line.1);
        }
        // The four features are named by their bare vendor name.
        for (key, _) in WORKER_FEATURE_OVERRIDES {
            let named = key.trim_start_matches("features.");
            assert!(line.1.contains(named), "the line does not name {named}: {}", line.1);
        }
        // Checkpoint 5 is named in prose rather than as a row: `provider_keys` is
        // four keys of plumbing and one *fact*, which is whose credential this pane
        // spends — and reciting the entry under the word "containment" would be a
        // sentence that is wrong about the most consequential thing in it (#29, C9).
        assert!(
            line.1.contains("fleet's key") && line.1.contains("cannot spend your plan"),
            "the line has to state the cost of a worker running on FLEETOR's provider: {}",
            line.1,
        );
        assert!(
            !line.1.contains(FLEET_KEY_ENV),
            "the Activity feed names the variable a credential travels in: {}",
            line.1,
        );

        let orch = machine.notices_for_the_operator("orch");
        let line = orch
            .iter()
            .find(|(level, _)| *level == NoticeLevel::Info)
            .expect("the orchestrator gets one too — the trio is still FLEETOR's");
        assert!(
            line.1.contains("your own pane") && line.1.contains("your own credential"),
            "the orchestrator's line has to say the provider, the flags and the login on this \
             seat are the operator's (#44, C43): {}",
            line.1,
        );
        assert!(
            line.1.contains("never written back"),
            "and that carrying the login in did not turn C6's one-way snapshot into a \
             two-way one: {}",
            line.1,
        );
        for (key, _) in WORKER_FEATURE_OVERRIDES {
            assert!(
                !line.1.contains(key.trim_start_matches("features.")),
                "the orchestrator's line names a feature it does not disable: {}",
                line.1,
            );
        }
    }

    #[test]
    fn the_operators_other_answers_survive_including_the_ones_that_deny() {
        // FLEETOR overwrites the keys it owns and nothing else, which is how an
        // operator's explicit deny rules stay honoured: they are not on any of
        // FLEETOR's lists, so the snapshot carries them through untouched. M10's
        // "overlays are additive and deny wins" holds, with C21's inverse applying
        // only to the rows above.
        let machine = Machine::new("denies");
        machine.operator_file(
            CONFIG_FILE,
            "[sandbox_workspace_write]\n\
             writable_roots = []\n\
             exclude_tmpdir_env_var = true\n\
             [mcp_servers.notes]\ncommand = \"note-server\"\nenabled = false\n",
        );
        machine.seed("worker-1").expect("seed");
        let seeded = machine.seeded("worker-1");

        assert!(
            seeded["sandbox_workspace_write"]["writable_roots"].as_array().is_some_and(|a| a
                .is_empty()),
            "an operator's narrowing of the sandbox's writable roots was dropped",
        );
        assert_eq!(seeded["sandbox_workspace_write"]["exclude_tmpdir_env_var"].as_bool(), Some(true));
        assert_eq!(seeded["mcp_servers"]["notes"]["enabled"].as_bool(), Some(false));
        // And FLEETOR's own row is in the same table, beside them rather than
        // instead of them.
        assert_eq!(seeded["sandbox_workspace_write"]["network_access"].as_bool(), Some(true));
    }

    #[test]
    fn the_newer_permission_profile_generation_is_not_written() {
        // Deliberately not used (C7). It is where the vendor is visibly heading —
        // a `.sandbox_migration` marker exists in a default install — but nothing
        // about it was measured, and the binary carries an explicit "derived
        // permission profile cannot be represented as a legacy sandbox policy;
        // falling back to read-only" path. That is a worker that spawns clean and
        // silently cannot write, which is the exact failure class C7 and C21 exist
        // to prevent. The legacy keys ship; #37's gate tripwire is what makes that
        // survivable.
        let machine = Machine::new("legacy");
        a_full_installation(&machine);
        machine.seed("worker-1").expect("seed");
        let seeded = machine.seeded("worker-1");

        assert!(seeded.get("permissions").is_none(), "the newer generation was written");
        assert!(seeded.get("default_permissions").is_none(), "and so was its selector");
        for (key, _) in fleetor_owned(false) {
            assert!(
                !key.starts_with("permissions"),
                "a key list reached for the unmeasured generation: {key}",
            );
        }
    }

    #[test]
    fn the_narrow_socket_lever_is_not_reached_for_again() {
        // Found, tried in three variations including `--enable network_proxy`, and
        // it does not work: `network.unix_sockets` and
        // `network.dangerously_allow_all_unix_sockets` configure the network-*proxy*
        // layer, not the seatbelt profile, and all three left the refusal in place
        // (C5). Recorded as *tried* rather than *unconsidered*, in the one form
        // that survives someone skimming this file — a red test.
        for (key, _) in fleetor_owned(false) {
            assert!(
                !key.starts_with("network."),
                "`{key}` reads like a targeted unix-socket allowance. It was measured and it \
                 does not lift the seatbelt's refusal — see C5 before spending an afternoon \
                 on it. The all-or-nothing `sandbox_workspace_write.network_access` is the \
                 only lever there is.",
            );
        }
    }

    // --- the reader the next three tickets write into ---------------------------

    /// A stand-in harness whose three key lists are **full**, so the one reader
    /// C36 puts here is exercised now rather than when #28, #29 and #30 arrive.
    ///
    /// Its rows are the measured ones: C7's sandbox trio, C9's FLEETOR provider
    /// and C5's socket lever — spelled exactly as they would be typed after
    /// `codex -c`, which is the form every one of them was measured through.
    static ALL_THREE_LISTS: HarnessSpec = HarnessSpec {
        posture: Posture {
            sandbox_keys: &[("sandbox_mode", "workspace-write"), ("approval_policy", "never")],
            ..CODEX_SPEC.posture
        },
        credentials: Credentials {
            provider_keys: &[
                ("model_provider", "fleetor"),
                ("model_providers.fleetor.base_url", "https://api.deepseek.com/"),
                ("model_providers.fleetor.wire_api", "responses"),
            ],
            ..CODEX_SPEC.credentials
        },
        outbound: Outbound {
            reachability_keys: &[("sandbox_workspace_write.network_access", "true")],
            ..CODEX_SPEC.outbound
        },
        ..CODEX_SPEC
    };

    #[test]
    fn the_three_key_lists_have_one_reader_and_it_writes_all_of_them() {
        // **The load-bearing test for #28, #29 and #30** (C36). Checkpoint 3's
        // sandbox posture, checkpoint 5's provider wiring and checkpoint 8's
        // reachability lever are three lists of the same shape answering one
        // question, and they are read together here so they cannot disagree about
        // where a harness's config keys get written.
        let machine = Machine::new("key-lists");
        a_full_installation(&machine);
        let dir = machine.pane_dir("worker-1");
        let cwd = machine.cwd();
        install(
            &ALL_THREE_LISTS,
            "/a/project",
            &Seed::new(&dir, &cwd, Some(&machine.operator_home())).with_brief(A_BRIEF),
        )
            .expect("seed with all three lists full");

        let seeded: DocumentMut =
            std::fs::read_to_string(dir.join(CONFIG_FILE)).expect("seed").parse().expect("TOML");

        // Checkpoint 3 — and Tier 1.7 reads this: what a harness puts here narrows
        // what a pane may do, so it has to have landed where the pane will read it.
        assert_eq!(seeded["sandbox_mode"].as_str(), Some("workspace-write"));
        assert_eq!(seeded["approval_policy"].as_str(), Some("never"));
        // Checkpoint 5 — a dotted path becomes a nested table, the way `-c` does.
        assert_eq!(seeded["model_provider"].as_str(), Some("fleetor"));
        assert_eq!(
            seeded["model_providers"]["fleetor"]["base_url"].as_str(),
            Some("https://api.deepseek.com/"),
        );
        // Checkpoint 8 — and `true` is the boolean, not the word.
        assert_eq!(
            seeded["sandbox_workspace_write"]["network_access"].as_bool(),
            Some(true),
            "a TOML-parseable value is written as that value, which is codex's own \
             documented rule for `-c key=value`",
        );

        // FLEETOR's keys are written last, so a snapshot cannot turn the sandbox
        // off by carrying the operator's own answer (C21's inverse of M10).
        assert_eq!(seeded["model_provider"].as_str(), Some("fleetor"));
    }

    #[test]
    fn a_key_lists_value_follows_codexs_own_parse_rule() {
        // Quoted from `codex --help`: "The `value` portion is parsed as TOML. If it
        // fails to parse as TOML, the raw string is used as a literal."
        assert_eq!(toml_value("true").as_bool(), Some(true));
        assert_eq!(toml_value("4096").as_integer(), Some(4096));
        assert_eq!(toml_value("\"quoted\"").as_str(), Some("quoted"));
        assert_eq!(toml_value("workspace-write").as_str(), Some("workspace-write"));
        assert!(toml_value("[\"a\", \"b\"]").as_array().is_some());
    }

    #[test]
    fn the_spec_leaves_none_of_the_four_lists_phase_two_owed() {
        // A reminder in test form, so #33 cannot register codex while a checkpoint
        // is still empty: the conformance suite refuses an empty `scrubbed_env` and
        // an empty `seed_keys`. #26 wrote this pinning **four** empty lists, #28
        // filled two and #29 filled the last two — so the pin inverts rather than
        // being deleted, and now says the thing worth saying next: none of the four
        // may go back to empty. An empty `sandbox_keys` is an unfenced pane, an
        // empty `reachability_keys` is a mute one, an empty `provider_keys` is a
        // pane with no way to authenticate, and an empty `scrubbed_env` is a fenced
        // pane holding the operator's own credential.
        assert!(!CODEX_SPEC.credentials.provider_keys.is_empty(), "#29 filled the provider");
        assert!(!CODEX_SPEC.credentials.scrubbed_env.is_empty(), "#29 filled the scrub");
        assert!(!CODEX_SPEC.posture.sandbox_keys.is_empty(), "#28 filled the sandbox trio");
        assert!(!CODEX_SPEC.outbound.reachability_keys.is_empty(), "#28 filled the socket lever");
        // Everything else is answered from a measurement.
        assert_eq!(CODEX_SPEC.config_dir.env_var, "CODEX_HOME");
        assert_eq!(CODEX_SPEC.isolation.config_env, CODEX_SPEC.config_dir.env_var);
        const { assert!(CODEX_SPEC.isolation.seeds_from_operator, "checkpoint 6 is a snapshot") };
        const { assert!(!CODEX_SPEC.isolation.credentials_follow_home, "and login is not HOME's") };
        const { assert!(CODEX_SPEC.isolation.private_home, "the Fence still wants one") };
        const { assert!(!CODEX_SPEC.transcript.file_move_is_safe, "WAL-mode: never `cp`") };
        assert!(!CODEX_SPEC.config_dir.seed_keys.is_empty());
    }

    // --- checkpoint 5 (#29, C2, C9, C27, C30, D-062) ---------------------------

    /// One fabricated operator installation with a **real credential in it**, for
    /// the tests that have to prove none of it reaches a fenced pane.
    ///
    /// The sentinels are distinct strings that appear nowhere else in this file, so
    /// a test asserting their absence is asserting about the operator's credential
    /// and not about a substring some unrelated value happens to contain. The shape
    /// is the operator's real one, read off the spike: a third-party provider with
    /// its own bearer token, selected by `model_provider`, plus an `env_key`
    /// naming a variable out of their shell profile.
    const OPERATORS_TOKEN: &str = "sk-operator-CREDENTIAL-SENTINEL";
    const OPERATORS_KEY_VAR: &str = "OPERATORS_OWN_SHELL_KEY_SENTINEL";
    const OPERATORS_PROVIDER: &str = "operators-deepseek";

    fn with_the_operators_credential(machine: &Machine) {
        machine.operator_file(
            CONFIG_FILE,
            &format!(
                "model_provider = \"{OPERATORS_PROVIDER}\"\n\
                 [model_providers.{OPERATORS_PROVIDER}]\n\
                 name = \"the operator's own\"\n\
                 base_url = \"https://api.deepseek.com/\"\n\
                 wire_api = \"responses\"\n\
                 experimental_bearer_token = \"{OPERATORS_TOKEN}\"\n\
                 env_key = \"{OPERATORS_KEY_VAR}\"\n",
            ),
        );
    }

    /// **The entry is written on every codex pane; only a seat FLEETOR drives runs
    /// on it** (C2 as amended by C9).
    ///
    /// The split is the whole of checkpoint 5's seat half. The *table* is a spec key
    /// list, so it lands everywhere for C41(a)'s reason and is inert where nothing
    /// selects it. The *selection* is [`WORKER_PROVIDER_SELECTION`], and it is the
    /// one row that decides whose credential a pane spends — which is why the
    /// orchestrator's own `model_provider` surviving is asserted here rather than
    /// left to be true by omission.
    #[test]
    fn a_worker_runs_on_the_fleets_provider_and_the_operator_keeps_their_own() {
        let machine = Machine::new("provider-seat");
        with_the_operators_credential(&machine);

        machine.seed("worker-1").expect("seed");
        machine.seed_for_the_operator("orch").expect("seed");

        let worker = machine.seeded("worker-1");
        let orch = machine.seeded("orch");

        // The entry itself, on both seats and spelled off the spec so a row that
        // changed goes red here rather than in review.
        for seat in [&worker, &orch] {
            for (key, value) in CODEX_SPEC.credentials.provider_keys {
                let path: Vec<&str> = key.split('.').collect();
                assert_eq!(
                    value_at(seat, &path).as_deref(),
                    Some(format!("\"{value}\"").as_str()),
                    "the FLEETOR provider entry is a spec key list, so it lands on every \
                     codex pane: {key}",
                );
            }
        }

        // The selection, which is the seat's.
        assert_eq!(
            worker["model_provider"].as_str(),
            Some(FLEET_PROVIDER),
            "a worker holds the fleet's credential, never the operator's (D-062)",
        );
        assert_eq!(
            orch["model_provider"].as_str(),
            Some(OPERATORS_PROVIDER),
            "the orchestrator runs on the operator's own login and inherited provider — \
             that seat being their own pane is the entire point (D-030, D-052)",
        );

        // And the override is announced, because an operator who finds a worker
        // talking to an endpoint they did not configure is owed the sentence (C21).
        let said = machine.notices("worker-2");
        let warned: Vec<&String> = said
            .iter()
            .filter(|(level, _)| *level == NoticeLevel::Warn)
            .map(|(_, text)| text)
            .filter(|text| text.contains("model_provider"))
            .collect();
        assert_eq!(
            warned.len(),
            1,
            "the provider a worker did not keep gets exactly one Warn: {said:#?}",
        );
        assert!(
            warned[0].contains(OPERATORS_PROVIDER) && warned[0].contains(FLEET_PROVIDER),
            "the line names what lost and what won: {}",
            warned[0],
        );
        assert!(
            !machine
                .notices_for_the_operator("orch-2")
                .iter()
                .any(|(_, text)| text.contains("model_provider")),
            "nothing was overridden on the operator's own seat, so nothing is announced",
        );
    }

    /// **The criterion that fails by looking healthy** (#29's acceptance criterion
    /// 2): nothing anywhere under a worker's configuration directory carries the
    /// operator's credential.
    ///
    /// A seeded pane with the operator's bearer token in it boots, reaches its
    /// prompt, answers, and is spending the operator's plan from inside the Fence —
    /// there is no symptom to notice. So the assertion is over the **whole
    /// directory as bytes**, not over the keys the seeder happens to know about: a
    /// snapshot entry added later that carried a credential in would go red here
    /// without anyone remembering that this test exists.
    ///
    /// Both sentinels matter and for different reasons. The token is the credential
    /// itself. The variable *name* is not a secret — following it is (L2), and a
    /// surviving `env_key` points a fenced pane straight at whatever the operator
    /// exported in their shell profile.
    #[test]
    fn a_workers_seeded_directory_carries_no_credential_of_the_operators() {
        let machine = Machine::new("no-operator-credential");
        with_the_operators_credential(&machine);
        // Something the snapshot *does* carry, so a green result cannot come from a
        // seeding that copied nothing at all.
        machine.operator_file("models.json", "{\"models\":[]}");

        let dir = machine.seed("worker-1").expect("seed");
        assert!(dir.join("models.json").is_file(), "the snapshot ran");

        for (name, bytes) in contents(&dir) {
            let text = String::from_utf8_lossy(&bytes);
            assert!(
                !text.contains(OPERATORS_TOKEN),
                "{name} in a fenced pane's config dir carries the operator's own key. This is \
                 the failure with no symptom: the pane boots, answers, and spends their plan.",
            );
            assert!(
                !text.contains(OPERATORS_KEY_VAR),
                "{name} names the variable the operator's key is exported in, so the vendor \
                 will read the key out of the pane's inherited environment (L2).",
            );
        }

        // The strike is not a deletion of the provider *table* — the pane keeps the
        // operator's preferences, minus the credential — so assert the thing that
        // would make the two indistinguishable is not what happened.
        let seeded = machine.seeded("worker-1");
        assert!(
            value_at(&seeded, &["model_providers", OPERATORS_PROVIDER, "base_url"]).is_some(),
            "the operator's provider row survived; only its credential fields are struck",
        );
    }

    /// **The fleet's own key is not written down either**, and the two halves of
    /// the channel that carries it name one variable.
    ///
    /// Codex's provider table offers two credential mechanisms and this is the
    /// decision between them: `experimental_bearer_token` would put the fleet's key
    /// in plaintext inside `pane-config/worker-N/config.toml`, and
    /// [`FLEET_KEY_ENV`] names a variable `spawn::worker_command_with` sets on the
    /// command instead — the same line, in the same function, that sets a Claude
    /// Code worker's `ANTHROPIC_AUTH_TOKEN`.
    ///
    /// The second half is the disagreement guard: `env_key` and
    /// [`Credentials::token_env`] are two independent spellings of one variable, and
    /// a pane whose provider reads `A` while placement sets `B` fails at its first
    /// turn with an authentication error that names neither.
    #[test]
    fn the_two_halves_of_the_credential_channel_name_one_variable() {
        assert_eq!(
            CODEX_SPEC.credentials.token_env,
            Some(FLEET_KEY_ENV),
            "the variable placement sets is the variable the provider entry reads",
        );
        let declared: Vec<&str> = CODEX_SPEC
            .credentials
            .provider_keys
            .iter()
            .filter(|(key, _)| key.ends_with(".env_key"))
            .map(|(_, value)| *value)
            .collect();
        assert_eq!(declared, [FLEET_KEY_ENV], "the entry reads exactly one variable, and it is ours");

        // The selection names the table that was actually written.
        for (_, provider) in WORKER_PROVIDER_SELECTION {
            assert!(
                CODEX_SPEC
                    .credentials
                    .provider_keys
                    .iter()
                    .any(|(key, _)| key.starts_with(&format!("model_providers.{provider}."))),
                "`model_provider = {provider}` names a table nothing defines, which is a pane \
                 that dies at configuration load",
            );
        }

        // And no row in the entry is a secret: every value is either a name, an
        // endpoint or a wire protocol. `experimental_bearer_token` is the row that
        // would put the fleet's key on disk, and it is deliberately not here.
        assert!(
            !CODEX_SPEC
                .credentials
                .provider_keys
                .iter()
                .any(|(key, _)| key.ends_with(".experimental_bearer_token")),
            "the fleet's key would be written into every pane's config.toml in plaintext",
        );
    }

    /// **The scrub, asserted as removal rather than as absence** (C27, C30).
    ///
    /// `env_remove` deletes a `CommandBuilder` entry rather than marking it, so on
    /// the finished command a name that was scrubbed and a name this machine never
    /// exported are the same observation — which is exactly why `Placed::scrubbed`
    /// exists. The list comes back from the call that performed the removal, so it
    /// cannot claim a scrub the command did not get, and asserting it needs nothing
    /// in the process environment.
    ///
    /// **Both names are codex's own**: `doctor` reports them under `auth env vars
    /// present`, so an operator with either in a shell profile is an operator whose
    /// personal credential would authenticate a fenced pane.
    ///
    /// The attended half is the asymmetry made observable (D-030, D-052, D-062):
    /// `spawn` builds a worker's command through the one function that scrubs and
    /// returns a `Worker`, and the three attended seats through functions that
    /// return a bare command and carry `Placed::scrubbed = &[]` — so an attended
    /// codex seat has nothing removed and nothing to report, and is also never
    /// handed the fleet's key.
    #[test]
    fn a_fenced_codex_worker_reports_the_scrub_and_an_attended_seat_has_none_to_report() {
        use crate::prompts::PaneContext;

        let context = PaneContext::baked();
        let worker = crate::placement::spawn::worker_command_with(
            codex(),
            1,
            Path::new("/tmp"),
            Path::new("/tmp/home"),
            Path::new("/tmp/cfg"),
            Path::new("/tmp/s.sock"),
            "sk-the-fleets-own-key",
            None,
            &context,
            None,
            "/usr/bin",
        );

        assert_eq!(
            worker.scrubbed, CODEX_SPEC.credentials.scrubbed_env,
            "the names reported are the names checkpoint 5 declares, from the one call that \
             removed them",
        );
        assert!(!worker.scrubbed.is_empty(), "a scrub that removes nothing is checkpoint 5 stubbed");
        assert_eq!(
            worker.command.get_env(FLEET_KEY_ENV).map(|v| v.to_string_lossy().into_owned()),
            Some("sk-the-fleets-own-key".to_string()),
            "the fenced pane authenticates with the fleet's key, through the variable the \
             provider entry reads",
        );

        let orch = crate::placement::spawn::orch_command_with(
            codex(),
            Path::new("/tmp"),
            Path::new("/tmp/s.sock"),
            Path::new("/tmp/cfg-orch"),
            &context,
            None,
            "/usr/bin",
        );
        assert!(
            orch.get_env(FLEET_KEY_ENV).is_none(),
            "the operator's own seat runs their login, not the fleet's credential",
        );
        for name in CODEX_SPEC.credentials.scrubbed_env {
            assert!(
                orch.get_env(name).is_none(),
                "{name} was set on an attended seat's command; the attended seats set nothing \
                 and remove nothing",
            );
        }
    }

    // --- checkpoint 2 (#27, C3, C37) ------------------------------------------

    /// The brief lands **beside the seed**, and the carrier names it by absolute
    /// path.
    ///
    /// The absolute path is not decoration: `CODEX_HOME` is not the pane's cwd, and
    /// a relative `model_instructions_file` would resolve against whichever
    /// directory the pane happens to be started in — a brief that is found in
    /// testing and missing in a worktree.
    #[test]
    fn the_brief_lands_beside_the_seed_and_the_carrier_names_it() {
        let machine = Machine::new("brief-file");
        let dir = machine.seed("worker-1").expect("seed");

        assert_eq!(machine.brief("worker-1"), A_BRIEF, "the brief is written verbatim");

        let key = CODEX_SPEC.brief.config_key.expect("codex carries its brief in a config key");
        let named = machine.seeded("worker-1")[key].as_str().expect("the carrier is set").to_string();
        assert_eq!(
            named,
            dir.join(BRIEF_FILE).to_string_lossy(),
            "the carrier names the file that was just written, absolutely",
        );
        assert!(Path::new(&named).is_file(), "and that file is there before the pane is");
        assert!(!dir.join(BRIEF_FILE.to_string() + ".tmp").exists(), "no temp file survives");
    }

    /// **D-042, asserted rather than assumed:** a codex pane's brief is the *same
    /// rendered text* a Claude Code pane of the same seat is handed. One
    /// `worker.md`, one `orch.md`, every harness — only the carrier varies.
    #[test]
    fn a_codex_pane_gets_the_same_rendered_brief_a_claude_code_pane_gets() {
        use fleetor_core::pane::{PaneId, WORKER_SLOTS};

        let machine = Machine::new("same-brief");
        let cwd = machine.cwd();
        let me = PaneId::Worker(2);
        // What `place_worker` renders and hands to `Seed::with_brief` — and, for a
        // Claude Code pane, what it hands to `--system-prompt` instead.
        let rendered =
            fleetor_core::brief::worker_brief(me, &PaneId::roster(&WORKER_SLOTS), &cwd.to_string_lossy());
        machine.seed_briefed("worker-2", &cwd, &rendered).expect("seed");

        assert_eq!(machine.brief("worker-2"), rendered, "byte-identical, not a codex dialect");

        // And the same text is what Claude Code's carrier would have carried, so
        // the two harnesses differ in transport and in nothing else.
        let cc = registered()[0];
        let flag = cc.spec().brief.argv_flag.expect("Claude Code briefs through argv");
        let argv = cc.command_args(&rendered, Some("acceptEdits"));
        let at = argv.iter().position(|a| a == flag).expect("the flag is there");
        assert_eq!(argv[at + 1], rendered);
        assert!(
            codex().command_args(&rendered, Some("acceptEdits")).iter().all(|a| a != &rendered),
            "and codex's argv carries no brief at all — its carrier is the config key",
        );
    }

    /// **The two fragments survive into the rendered codex brief** (`building.md`
    /// §4). The delivery contract is the only reason a model can tell a failed send
    /// from a good one; the broadcast rule is the only mitigation left for
    /// amplification after the rate limiter was removed (D-031).
    ///
    /// Asserted against **literals from the fragment files**, not against the
    /// placeholder or the fragment constants: a test that checked `{broadcast_rule}`
    /// was gone would pass on a template that dropped the placeholder entirely.
    #[test]
    fn the_two_fragments_survive_into_the_rendered_codex_brief() {
        use fleetor_core::pane::{PaneId, WORKER_SLOTS};

        let machine = Machine::new("fragments");
        let cwd = machine.cwd();
        let rendered = fleetor_core::brief::worker_brief(
            PaneId::Worker(1),
            &PaneId::roster(&WORKER_SLOTS),
            &cwd.to_string_lossy(),
        );
        machine.seed_briefed("worker-1", &cwd, &rendered).expect("seed");
        let installed = machine.brief("worker-1");

        for (fragment, clause) in [
            ("delivery-contract.md", "did **not** deliver"),
            ("broadcast-rule.md", "Never reply to a broadcast unless it names you"),
        ] {
            assert!(
                installed.contains(clause),
                "{fragment}'s clause is missing from the brief a codex pane will run on",
            );
        }
        assert!(
            !installed.contains("{delivery_contract}") && !installed.contains("{broadcast_rule}"),
            "the placeholders were composed into the brief, not carried into it",
        );
        assert!(installed.contains("fleet send"), "and the verbs reached the pane");
    }

    /// **Nothing is written into the pane's checkout** — the whole of M5's cost
    /// that C3 says does not transfer. No brief file in the worktree, so no
    /// `.git/info/exclude` line and no story about keeping `git status` honest.
    #[test]
    fn the_brief_is_not_written_into_the_panes_checkout() {
        let machine = Machine::new("no-worktree-file");
        let cwd = machine.cwd();
        std::fs::write(cwd.join("README.md"), "the operator's own\n").expect("a checkout");
        let before = contents(&cwd);

        machine.seed("worker-1").expect("seed");

        assert_eq!(contents(&cwd), before, "the pane's checkout is untouched by seeding");
        const { assert!(!CODEX_SPEC.brief.writes_into_worktree) };
        const { assert!(CODEX_SPEC.brief.replaces_system_prompt, "D-043 — replace, not append") };
        const { assert!(CODEX_SPEC.brief.argv_flag.is_none(), "and it never travels in argv") };
    }

    /// **Neither rejected carrier is reached for** (C3), asserted against the
    /// seeded document rather than against this module's source.
    ///
    /// `base_instructions` in a custom `model_catalog_json` is a *descriptor* — the
    /// built-in prompt is sent anyway — and an `AGENTS.md` in the pane's cwd
    /// arrives as a `user` message, in-band, re-injected on every clear (C37).
    #[test]
    fn neither_rejected_carrier_is_reached_for() {
        let machine = Machine::new("rejected-carriers");
        let cwd = machine.cwd();
        let dir = machine.seed("worker-1").expect("seed");

        let seeded = machine.seeded("worker-1").to_string();
        assert!(
            !seeded.contains("base_instructions"),
            "the model catalog's instruction field is not honoured — it is not a carrier",
        );
        assert!(!cwd.join("AGENTS.md").exists(), "an AGENTS.md would be in-band, as a user message");
        assert!(!dir.join("AGENTS.md").exists());
        assert!(
            !seeded.contains("experimental_instructions_file"),
            "that key does not exist in the recorded build",
        );
    }

    /// **A seed with no brief is refused rather than installed.**
    ///
    /// This is the signature failure of the whole arc: a codex pane whose carrier
    /// key is absent runs the vendor's built-in prompt, renders a prompt, accepts a
    /// paste and answers — having never been told it is part of a fleet. Nothing
    /// downstream can see it, so the seeder is the last place that can.
    #[test]
    fn a_seed_carrying_no_brief_is_refused() {
        let machine = Machine::new("no-brief");
        let cwd = machine.cwd();

        let why = machine.seed_briefed("worker-1", &cwd, "").expect_err("an empty brief is refused");
        assert!(why.contains("no brief"), "the refusal says what is missing: {why}");
        assert!(
            !machine.pane_dir("worker-1").join(CONFIG_FILE).exists(),
            "and nothing is installed — a half-seeded pane is the thing being prevented",
        );
        assert!(machine.seed_briefed("worker-2", &cwd, "   \n").is_err(), "whitespace is not a brief");
    }

    /// A re-seed rewrites the brief and keeps the carrier pointing at it — the
    /// merge path (`seed_merges`), which is what a target switch goes through.
    #[test]
    fn a_reseed_rewrites_the_brief_and_keeps_the_carrier() {
        let machine = Machine::new("reseed");
        let cwd = machine.cwd();
        machine.seed_briefed("worker-1", &cwd, "the first brief").expect("seed");
        machine.seed_briefed("worker-1", &cwd, "the second brief").expect("re-seed");

        assert_eq!(machine.brief("worker-1"), "the second brief");
        let key = CODEX_SPEC.brief.config_key.expect("a config key");
        assert_eq!(
            machine.seeded("worker-1")[key].as_str(),
            Some(machine.pane_dir("worker-1").join(BRIEF_FILE).to_string_lossy().as_ref()),
            "the carrier survives the merge that keeps the previous target's trust row",
        );
    }
}
