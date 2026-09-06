//! Codex — checkpoints 4 and 6, and the one reader the three key lists share
//! (WP-25 phase 2, issue #26; C6, C9, C16, C17, C31, C32, C36).
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
//! measurement in `docs/notes/codex-spike-notes.md`, **except four lists that are
//! empty on purpose and name the ticket that fills them**:
//!
//! | field | checkpoint | filled by |
//! |---|---|---|
//! | [`Posture::sandbox_keys`] | 3 | #28 — the sandbox trio and the feature overrides |
//! | [`Credentials::provider_keys`] | 5 | #29 — the FLEETOR provider |
//! | [`Credentials::scrubbed_env`] | 5 | #29 — the same ticket, the other half |
//! | [`Outbound::reachability_keys`] | 8 | #28 — the socket lever is one of the trio |
//!
//! An empty list here is not a stub that will pass quietly: the conformance suite
//! refuses an empty `scrubbed_env` and refuses a harness whose spec answers
//! nothing, which is exactly why **codex is not registered yet** and why #33 is
//! the single moment it joins [`registered`](super::harness::registered).
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
///  - `auth.json` — the operator's credential. #29's job, and a worker holds the
///    fleet's credential rather than the operator's regardless (C9, D-062).
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

/// Per-provider keys that carry, or reach, the operator's own credential — struck
/// from every `[model_providers.*]` table on the way in (C9).
///
/// `experimental_bearer_token` is the operator's key in plaintext; the real
/// installation has one. `env_key` is subtler and is here for L2's reason: it
/// names an environment variable the vendor will read the key out of, so a value
/// that survives points a fenced pane straight at whatever the operator exported
/// in their shell profile. Naming a variable is not a secret, and following it is.
const PROVIDER_CREDENTIAL_KEYS: &[&str] = &["experimental_bearer_token", "env_key"];

/// The file `CODEX_HOME` is read from — checkpoint 4's seed file, and checkpoint
/// 14's trust file, which for this harness are the same document.
const CONFIG_FILE: &str = "config.toml";

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
    // that it survives `/clear`.
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
        sandbox_keys: &[],
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

    // 5 — credential wiring and the scrub. **Both halves are #29's**, and empty
    // here rather than guessed: codex's worker credential is a FLEETOR-written
    // `[model_providers.fleetor]` in the seeded config (C9), which is a
    // `provider_keys` answer, not an environment one.
    credentials: Credentials {
        base_url_env: None,
        token_env: None,
        provider_keys: &[],
        scrubbed_env: &[],
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
    outbound: Outbound { sandboxed: true, socket_reachable: true, reachability_keys: &[] },

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
    /// 3. **FLEETOR's keys last, so they win.** The isolation keys, then the three
    ///    checkpoint key lists in checkpoint order, then the trust record. M10's
    ///    "overlays are additive and deny wins" gains its inverse here (C21): the
    ///    keys FLEETOR owns overwrite the operator's, because a snapshot that
    ///    could turn the sandbox off would be a snapshot that widens auto-approve.
    /// 4. **The refusal.** [`refuse_home_relative`] before anything is installed.
    /// 5. **Temp file and rename**, because a half-written `config.toml` is a pane
    ///    that dies at spawn.
    fn seed_config_dir(&self, seed: &Seed<'_>) -> Result<(), String> {
        install(self.spec(), &self.project_key(seed.cwd), seed)
    }

    /// Checkpoints 1, 2 and 3's behavioural half.
    ///
    /// **The brief does not travel in argv for this harness** — it is
    /// `model_instructions_file`, a config key naming a file — so what this
    /// returns carries no brief, and the file it names is written beside the
    /// seed. That is #27's ticket, and until it lands a codex pane would get the
    /// vendor's own prompt, which is one more reason codex is not registered.
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
fn install(spec: &'static HarnessSpec, project_key: &str, seed: &Seed<'_>) -> Result<(), String> {
    let dir = seed.config_dir;
    std::fs::create_dir_all(dir).map_err(|e| format!("create config dir {}: {e}", dir.display()))?;

    let source = seed.operator_home.map(|home| home.join(OPERATOR_DIR)).filter(|d| d.is_dir());

    // 1. The tree.
    if let Some(source) = &source {
        for entry in SNAPSHOT_ENTRIES {
            copy_into(&source.join(entry), &dir.join(entry))?;
        }
    }

    // 2. The base document.
    let installed = dir.join(spec.config_dir.seed_file);
    let mut doc = match (spec.config_dir.seed_merges, std::fs::read_to_string(&installed)) {
        (true, Ok(text)) => {
            text.parse::<DocumentMut>().map_err(|e| format!("parse {}: {e}", installed.display()))?
        }
        _ => seeded_document(source.as_deref(), dir)?,
    };

    // 3. FLEETOR's keys, last so they win.
    for (key, value) in isolation_keys(spec, dir) {
        set_path(&mut doc, &[key], Value::from(value));
    }
    for (key, value) in checkpoint_keys(spec) {
        set_path(&mut doc, &key.split('.').collect::<Vec<_>>(), toml_value(value));
    }
    for key in spec.project_identity.trust_keys {
        set_path(&mut doc, &["projects", project_key, key], Value::from(TRUST_AFFIRMATIVE));
    }

    // 4. The measured trap, refused rather than installed.
    refuse_home_relative(&doc)?;

    // 5. Temp file, then rename.
    let tmp = installed.with_extension("toml.tmp");
    std::fs::write(&tmp, doc.to_string()).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &installed).map_err(|e| format!("install {}: {e}", installed.display()))
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
/// 1. **The credential comes out** (C9). Every `[model_providers.*]` table loses
///    [`PROVIDER_CREDENTIAL_KEYS`], and `auth.json` was never in
///    [`SNAPSHOT_ENTRIES`] to begin with.
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
fn seeded_document(source: Option<&Path>, pane_dir: &Path) -> Result<DocumentMut, String> {
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

    strike_provider_credentials(&mut doc);

    let operator_home = source.parent().map(Path::to_path_buf);
    rewrite_item(doc.as_item_mut(), &|raw| {
        relocate(raw, operator_home.as_deref(), source, pane_dir)
    });

    Ok(doc)
}

/// Every `[model_providers.*]` table loses the keys that carry or reach the
/// operator's credential. The provider itself stays: C2 says it is inherited and
/// displayed, and #29 is what gives a worker seat the fleet's own instead.
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

        fn seed_at(&self, pane: &str, cwd: &Path) -> Result<PathBuf, String> {
            let dir = self.pane_dir(pane);
            let home = self.operator_home();
            codex().seed_config_dir(&Seed::new(&dir, cwd, Some(&home)))?;
            Ok(dir)
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
        codex().seed_config_dir(&Seed::new(&dir, &cwd, None)).expect("seed against nothing");

        let seeded: DocumentMut = std::fs::read_to_string(dir.join(CONFIG_FILE))
            .expect("a seed file all the same")
            .parse()
            .expect("valid TOML");
        assert_eq!(
            seeded["projects"][codex().project_key(&cwd)]["trust_level"].as_str(),
            Some(TRUST_AFFIRMATIVE),
        );
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
        install(&ALL_THREE_LISTS, "/a/project", &Seed::new(&dir, &cwd, Some(&machine.operator_home())))
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
    fn the_spec_leaves_exactly_the_four_lists_its_own_tickets_fill() {
        // A reminder in test form, so #33 cannot register codex while a checkpoint
        // is still empty: the conformance suite refuses an empty `scrubbed_env` and
        // an empty `seed_keys`, and these four are the ones phase 2 still owes.
        assert!(CODEX_SPEC.posture.sandbox_keys.is_empty(), "#28 fills the sandbox trio");
        assert!(CODEX_SPEC.outbound.reachability_keys.is_empty(), "#28 fills the socket lever");
        assert!(CODEX_SPEC.credentials.provider_keys.is_empty(), "#29 fills the provider");
        assert!(CODEX_SPEC.credentials.scrubbed_env.is_empty(), "#29 fills the scrub");
        // Everything else is answered from a measurement.
        assert_eq!(CODEX_SPEC.config_dir.env_var, "CODEX_HOME");
        assert_eq!(CODEX_SPEC.isolation.config_env, CODEX_SPEC.config_dir.env_var);
        const { assert!(CODEX_SPEC.isolation.seeds_from_operator, "checkpoint 6 is a snapshot") };
        const { assert!(!CODEX_SPEC.isolation.credentials_follow_home, "and login is not HOME's") };
        const { assert!(CODEX_SPEC.isolation.private_home, "the Fence still wants one") };
        const { assert!(!CODEX_SPEC.transcript.file_move_is_safe, "WAL-mode: never `cp`") };
        assert!(!CODEX_SPEC.config_dir.seed_keys.is_empty());
    }
}
