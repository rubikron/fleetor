//! Claude Code: its spec, its `Harness` impl, its diagnostic and every answer that knows its formats.

use std::path::{Path, PathBuf};

use fleetor_core::event::NoticeLevel;

use super::harness::*;
use crate::guardrail;

// --- Claude Code --------------------------------------------------------------

/// Claude Code — the harness every pane has run since before there was a seam,
/// and for now the only registered one.
///
/// A unit struct: everything it knows is in [`CLAUDE_CODE_SPEC`], and two of its
/// three methods delegate to `project_key` and `json_seed::seed_json_document`.
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
        // full name". An alias is deliberately preferred
        // to a dated id here — it tracks the latest of each family, so this list
        // does not go stale every release.
        //
        // **Offered on a seat running the operator's own login and nowhere else.**
        // A worker on the fleet's key talks to FLEETOR's provider, where none of
        // these names resolves; the gate picks the list per credential.
        declared_models: &[
            ("fable", "Claude Fable 5.1"),
            ("opus", "Claude Opus 5.5"),
            ("sonnet", "Claude Sonnet 5.5"),
            ("haiku", "Claude Haiku 5.5"),
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

    // 4 — config dir and its seeding. `json_seed::seed_json_document`.
    config_dir: ConfigDir {
        env_var: "CLAUDE_CONFIG_DIR",
        seed_file: ".claude.json",
        seed_keys: &["hasCompletedOnboarding"],
        seed_merges: true,
    },

    // 5 — credential wiring and the scrub. `spawn::worker_command_with`.
    credentials: Credentials {
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
        // **Measured, and it is macOS's fact rather than this vendor's** (C73,
        // `plan-worker-notes.md` §2): `security list-keychains` under a private
        // `HOME` returns the System keychain alone, so the entry `credential_env`
        // names is not on the search list at all. Setting that variable correctly
        // buys nothing once `HOME` has been replaced.
        operator_store_follows_home: true,
        private_home: true,
        // **Measured** (D-082): the operator's 2.1.266 binary under a private
        // `HOME` wrote 2.1.267 (200 MB) into it within 90 seconds and printed
        // "Update installed · Restart to update"; with this set, nothing. The
        // vendor's own sandbox runner sets the same variable for the same reason.
        fenced_env: &[("DISABLE_AUTOUPDATER", "1")],
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

    // 11 — gauge source and window. Claude Code does not publish its window,
    // so the fleet states it (D-125).
    gauge: GaugeSource { reads_transcript: true, window_tokens: Some(1_000_000) },

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

    // 14 — project identity and trust seeding (C17). `project_key` and the
    // per-project half of `json_seed::seed_json_document`.
    project_identity: ProjectIdentityAndTrust {
        canonicalize: true,
        exact_path_match: true,
        trust_file: ".claude.json",
        trust_keys: &["hasTrustDialogAccepted", "hasCompletedProjectOnboarding"],
    },

    // 15 — reopening a recorded session (WP-27, R6). `--resume <id>` takes an id
    // directly, appends to the same transcript under the same `sessionId`, and
    // does not fork. Driven under a pty in `docs/notes/reopen-spike-notes.md`.
    resume: Resume::Supported,
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
        project_key(cwd)
    }

    fn seed_config_dir(&self, seed: &Seed<'_>) -> Result<Vec<(NoticeLevel, String)>, String> {
        // Nothing to announce: this directory is the fleet's own, so no key
        // written into it is standing on top of an answer the operator gave.
        let spec = self.spec();
        // This harness keeps the first-run gate in the document it seeds, so one
        // read-modify-write covers both checkpoints. A harness that split them would
        // need two writes, and would not be sharing this implementation.
        debug_assert_eq!(
            spec.config_dir.seed_file, spec.project_identity.trust_file,
            "this seeder writes one document; a harness whose trust file differs needs its own",
        );
        super::json_seed::seed_json_document(self, seed)?;
        // **Checkpoint 6's plan-seat half** (C73, C75). The credential goes here
        // and only here: a fenced pane has no login keychain on its search list,
        // and its own configuration directory is the one store it can still read.
        // Measured, not assumed — `plan-worker-notes.md` §12.
        if let Some(login) = seed.operator_login {
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
    fn diagnose(&self, facts: &MachineFacts<'_>) -> HarnessReadiness {
        claude_code_diagnose(facts.home, facts.path)
    }

    fn read_usage(
        &self,
        source: &crate::context_gauge::TranscriptSource,
    ) -> Option<crate::context_gauge::UsageReading> {
        jsonl_usage(source)
    }

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

        let text =
            serde_json::to_string_pretty(&root).map_err(|e| format!("encode settings: {e}"))?;
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

    /// **Claude Code reopens with `--resume <id>`**, which the vendor accepts as a
    /// session id directly with no picker (verified from `--help` and driven under
    /// a pty, `docs/notes/reopen-spike-notes.md` §1).
    ///
    /// **This launch's brief is passed again, with prompt recording off.** Claude
    /// Code records the system prompt on a conversation's first request and
    /// reuses that record on every resume. A pane with no turns has no record,
    /// so without the flag it comes up on the vendor's own prompt; a pane with
    /// history would keep its old brief. `--system-prompt-snapshot` needs
    /// Claude Code 2.1.257 or later and is passed unconditionally.
    ///
    /// **No `--fork-session`.** Measured: a plain resume appends to the same
    /// `.jsonl` under the same `sessionId` rather than forking, which is exactly
    /// what R3's continue-in-place wants.
    fn resume_args(&self, seat: &Seat<'_>, session_id: &str) -> Option<Vec<String>> {
        let spec = self.spec();
        let mut args = vec!["--resume".to_string(), session_id.to_string()];
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
        args.push("--system-prompt-snapshot".to_string());
        args.push("off".to_string());
        Some(args)
    }

    /// **Claude Code's resumable id is its transcript's own filename** — it keeps
    /// `projects/<cwd-slug>/<session-id>.jsonl`, and every row inside carries the
    /// same `sessionId` as the stem (measured; spike notes §2).
    ///
    /// Reads the newest transcript under the seat, because R3 leaves a seat's
    /// sessions in place and a lineage's directory accumulates one per reopen —
    /// the id worth recording for *this* run is the one it last wrote. Same rule
    /// the context gauge already uses (`context_gauge::latest_transcript`).
    fn session_id(&self, seat_dir: &Path) -> Option<String> {
        let spec = self.spec();
        let root = seat_dir.join(spec.transcript.subdir);
        let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
        for file in crate::runs::transcript_files_for(&root, spec.transcript.file_ext) {
            let Ok(at) = std::fs::metadata(&file).and_then(|m| m.modified()) else { continue };
            if newest.as_ref().is_none_or(|(best, _)| at > *best) {
                newest = Some((at, file));
            }
        }
        newest?.1.file_stem().and_then(|s| s.to_str()).map(str::to_string)
    }

    /// Present when a transcript named `<session_id>.jsonl` is under the seat —
    /// the filename rule [`Self::session_id`] reads the id from, asked the other
    /// way round.
    fn has_session(&self, seat_dir: &Path, session_id: &str) -> bool {
        let spec = self.spec();
        crate::runs::transcript_files_for(
            &seat_dir.join(spec.transcript.subdir),
            spec.transcript.file_ext,
        )
        .iter()
        .any(|file| file.file_stem().and_then(|s| s.to_str()) == Some(session_id))
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

    let through_the_name = vendor_version(Path::new(invoked), path);
    let through_the_binary = vendor_version(&resolved, path);

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
            };
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
fn vendor_version(bin: &Path, path: &str) -> Option<String> {
    let out = std::process::Command::new(bin).arg("--version").env("PATH", path).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// Whether Claude Code serves this slug: a `claude-` id or one of its aliases,
/// either with a `[1m]` suffix (D-126).
pub fn claude_code_recognises(slug: &str) -> bool {
    const ALIASES: [&str; 7] = ["sonnet", "opus", "default", "fable", "best", "opusplan", "haiku"];
    let slug = slug.strip_suffix("[1m]").unwrap_or(slug);
    slug.starts_with("claude-") || ALIASES.contains(&slug)
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
    std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf()).to_string_lossy().into_owned()
}

/// `<config_dir>/projects/<slug>/`, where `<slug>` is the pane's project key with
/// every character that is not an ASCII letter or digit replaced by `-`.
///
/// **The key comes from the pane's own harness** (WP-25, checkpoint 14), which is
/// what makes "the gauge and the trust flag agree on one spelling of a cwd" a
/// property of the seam rather than of two call sites happening to name the same
/// function. For Claude Code that answer is still `std::fs::canonicalize` — the
/// same resolved path it itself sees as its cwd, since macOS resolves `/tmp` and
/// `/var` symlinks on `getcwd` — and it is the key
/// [`Harness::seed_config_dir`](crate::placement::harness::Harness::seed_config_dir)
/// wrote the pane's trust record under.
///
/// **The subdirectory is checkpoint 13's** (WP-25,
/// [`Transcript::subdir`](crate::placement::harness::Transcript::subdir)), read
/// off the same harness as the key.
///
/// **The slug rule is not, and this is now the only place that shows it.** How a
/// harness names the *per-project directory* underneath that subdirectory is not
/// one of the fourteen answers, so the `/`-and-`.`-to-`-` mapping below is still
/// Claude Code's own literal — the last one in this function, where there were
/// two. Nothing else in the fleet re-derives it: the harvest walks whatever
/// directories it finds rather than computing their names, so this is the single
/// consumer and the single point of drift. A second harness whose transcripts are
/// per-project cannot be read by this gauge until that becomes a checkpoint
/// answer or a method on [`Harness`](crate::placement::harness::Harness); a
/// harness whose store is not per-project at all is unaffected. Left for #39,
/// which is the ticket that has a second layout to generalize against — inventing
/// the shape from one example is how a seam ends up describing one vendor.
pub(crate) fn project_dir(source: &crate::context_gauge::TranscriptSource) -> PathBuf {
    let resolved = source.harness.project_key(&source.cwd);
    // Claude Code's own rule, measured on disk: `_shell` becomes `-shell`.
    let slug: String =
        resolved.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    source.config_dir.join(source.harness.spec().transcript.subdir).join(slug)
}

/// The pane's current transcript: the most-recently-modified file with this
/// harness's transcript extension (WP-25, checkpoint 13's
/// [`Transcript::file_ext`](crate::placement::harness::Transcript::file_ext))
/// under its project directory.
///
/// Read fresh on every call, never cached from spawn. A `/clear` resets a
/// pane's session (`docs/notes/command-channel-notes.md`); whether Claude Code
/// opens a new `<uuid>.jsonl` for that or keeps writing the same one was not
/// worth a fifth spike run to settle — picking "most recently modified" is
/// correct under either answer, at the cost of one directory read per sample.
fn latest_transcript(dir: &Path, file_ext: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some(file_ext))
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
}

/// **The default reader** behind
/// [`Harness::read_usage`](crate::placement::harness::Harness::read_usage): the
/// `usage` object on the last completed turn of the pane's own transcript.
///
/// It reports no window of its own — a harness whose transcript carries usage
/// but not the window it was measured against is exactly the case D-054's
/// constant was written for, and stating `None` here is what routes the pane
/// back to the window its spec asserts rather than to one this function
/// invented.
pub fn jsonl_usage(
    source: &crate::context_gauge::TranscriptSource,
) -> Option<crate::context_gauge::UsageReading> {
    let path = latest_transcript(&project_dir(source), source.harness.spec().transcript.file_ext)?;
    let text = std::fs::read_to_string(&path).ok()?;
    Some(crate::context_gauge::UsageReading {
        used_tokens: last_turn_usage(&text)?,
        window_tokens: None,
    })
}

/// The last `assistant` line's total prompt usage, or `None` if the
/// transcript has no completed turn yet.
///
/// **`input_tokens + cache_creation_input_tokens + cache_read_input_tokens`,
/// never `input_tokens` alone.** Prompt caching moves most of a turn's tokens
/// out of `input_tokens` and into `cache_read_input_tokens` the moment the
/// endpoint recognizes repeated context — reading `input_tokens` alone would
/// report a pane's usage *dropping* the instant caching kicked in, which is
/// the opposite of what happened (`docs/notes/context-gauge-notes.md` §3, measured
/// live: 27,723 → 96 on `input_tokens` alone across two turns of the same
/// growing conversation; 27,723 → 27,744 on the sum).
///
/// Scans from the end: two identical `usage` lines are written per real turn
/// (same `message.id`, byte-identical usage), so "last" and "last real turn"
/// agree without de-duplicating.
///
/// **A compaction after the last call wins over it**: the boundary line carries
/// the size the context was cut to, and the next call may be hours away.
///
/// **A line with no usage is skipped**, not read as zero: Claude Code writes a
/// synthetic `assistant` line for an API error, with every usage field 0.
fn last_turn_usage(transcript: &str) -> Option<u32> {
    transcript.lines().rev().find_map(|line| {
        let entry: serde_json::Value = serde_json::from_str(line).ok()?;
        match entry.get("type")?.as_str()? {
            "system" if entry.get("subtype")?.as_str()? == "compact_boundary" => {
                Some(entry.get("compactMetadata")?.get("postTokens")?.as_u64()? as u32)
            }
            "assistant" => {
                let usage = entry.get("message")?.get("usage")?;
                let field = |name: &str| usage.get(name).and_then(|v| v.as_u64()).unwrap_or(0);
                let total = field("input_tokens")
                    + field("cache_creation_input_tokens")
                    + field("cache_read_input_tokens");
                (total > 0).then_some(total as u32)
            }
            _ => None,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_gauge::TranscriptSource;
    use crate::placement::harness::Seat;

    fn assistant_line(input: u64, cache_creation: u64, cache_read: u64) -> String {
        serde_json::json!({
            "type": "assistant",
            "message": {
                "role": "assistant", "model": "deepseek-v4-flash",
                "usage": {
                    "input_tokens": input,
                    "cache_creation_input_tokens": cache_creation,
                    "cache_read_input_tokens": cache_read,
                    "output_tokens": 2
                }
            }
        })
        .to_string()
    }

    fn user_line(text: &str) -> String {
        serde_json::json!({"type": "user", "message": {"role": "user", "content": text}})
            .to_string()
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
        let readiness =
            |login: LoginState| HarnessReadiness { login, ..HarnessReadiness::not_installed(spec) };

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
        assert_eq!(readiness(LoginState::LoggedIn(AccountShape::ApiKey)).login_guidance(), None,);

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
        assert_eq!(spec.gauge.window_tokens, Some(1_000_000));
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
        // Checkpoint 14's two halves agree: the method is `project_key`,
        // and the spec says that is what it is.
        let dir = std::env::temp_dir();
        assert_eq!(claude_code().project_key(&dir), project_key(&dir));
        assert!(claude_code().spec().project_identity.canonicalize);
    }

    /// Checkpoint 15's reader and its presence check agree on one session, and the
    /// check is exact: a prefix of the id is not the session (R19).
    #[test]
    fn claude_codes_presence_check_finds_the_session_its_reader_names_and_no_other() {
        let seat = std::env::temp_dir()
            .join(format!("fleetor-cc-has-session-{}", fleetor_core::time::now_ms()));
        let project = seat.join("projects").join("-tmp-slug");
        std::fs::create_dir_all(&project).expect("scratch seat");
        std::fs::write(project.join("abc-123.jsonl"), r#"{"type":"user"}"#).expect("a transcript");

        let cc = claude_code();
        let id = cc.session_id(&seat).expect("the reader names the session");
        assert!(cc.has_session(&seat, &id), "the check must find what the reader named");
        assert!(!cc.has_session(&seat, "abc"), "a prefix of the id is not the session");
        assert!(!cc.has_session(&seat, "zzz-999"), "an id with no transcript is absent");
        assert!(!cc.has_session(&seat.join("nowhere"), &id), "a missing seat holds nothing");
        let _ = std::fs::remove_dir_all(&seat);
    }

    #[test]
    fn seeding_through_the_harness_writes_the_keys_the_spec_names() {
        // Checkpoints 4 and 14 end to end, against a scratch directory — the same
        // property `placement`'s own rule buys: every filesystem effect is
        // confined to the directory it was handed.
        let scratch = std::env::temp_dir()
            .join(format!("fleetor-harness-seed-{}", fleetor_core::time::now_ms()));
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

    #[test]
    fn no_assistant_line_means_no_reading_at_all() {
        let transcript = [user_line("hello")].join("\n");
        assert_eq!(last_turn_usage(&transcript), None);
    }

    /// The load-bearing finding from the spike: `input_tokens` alone would
    /// *drop* the instant prompt caching kicks in, which reads as the
    /// conversation shrinking when it only grew. The sum must keep climbing.
    #[test]
    fn usage_is_the_sum_of_all_three_input_fields_not_input_tokens_alone() {
        let turn_one = assistant_line(27_723, 0, 0);
        let turn_two = assistant_line(96, 0, 27_648);

        let after_turn_one = last_turn_usage(&turn_one).unwrap();
        let after_turn_two = last_turn_usage(&[turn_one, turn_two].join("\n")).unwrap();

        assert_eq!(after_turn_one, 27_723);
        assert_eq!(
            after_turn_two, 27_744,
            "must track the growing conversation, not just input_tokens (96)"
        );
        assert!(
            after_turn_two > after_turn_one,
            "a second turn must never look like it shrank the context"
        );
    }

    /// Two identical usage lines per real turn (same content, as Claude Code
    /// actually writes them) must not be double-counted or picked
    /// inconsistently — "last line" and "last real turn" have to agree.
    #[test]
    fn a_duplicated_usage_line_per_turn_does_not_change_the_reading() {
        let line = assistant_line(500, 0, 0);
        let once = last_turn_usage(&line).unwrap();
        let twice = last_turn_usage(&[line.clone(), line].join("\n")).unwrap();
        assert_eq!(once, twice);
    }

    /// Only the *last* assistant line counts — an earlier turn's usage must
    /// never win over a later, larger one.
    #[test]
    fn only_the_last_assistant_lines_usage_is_read() {
        let transcript =
            [assistant_line(1_000, 0, 0), user_line("more"), assistant_line(1_200, 0, 800)]
                .join("\n");
        assert_eq!(last_turn_usage(&transcript), Some(2_000));
    }

    #[test]
    fn a_line_that_is_not_json_is_skipped_rather_than_fatal() {
        let transcript = ["not json at all".to_string(), assistant_line(50, 0, 0)].join("\n");
        assert_eq!(last_turn_usage(&transcript), Some(50));
    }

    /// Claude Code names a transcript directory by replacing every character
    /// that is not a letter or digit — `_` included, and every worker's cwd is
    /// under `_shell`. Measured on disk: `.fleetor/_shell` is `-fleetor--shell`.
    #[test]
    fn the_transcript_directory_is_found_for_a_cwd_under_shell() {
        let source = TranscriptSource {
            harness: crate::placement::harness::claude_code(),
            config_dir: PathBuf::from("/cfg"),
            cwd: PathBuf::from("/nonexistent/.fleetor/_shell/worktrees/repo-f7f9/worker-2"),
            window_tokens: None,
        };
        let dir = project_dir(&source);
        assert!(
            dir.ends_with("-nonexistent--fleetor--shell-worktrees-repo-f7f9-worker-2"),
            "{}",
            dir.display()
        );
    }

    #[test]
    fn a_compaction_after_the_last_call_is_what_the_context_holds() {
        let boundary = serde_json::json!({
            "type": "system",
            "subtype": "compact_boundary",
            "compactMetadata": { "preTokens": 588_656, "postTokens": 11_284 }
        })
        .to_string();
        let compacted = [assistant_line(96, 0, 588_560), boundary.clone()].join("\n");
        assert_eq!(last_turn_usage(&compacted), Some(11_284));
        let then_a_call = [compacted, assistant_line(40, 0, 48_542)].join("\n");
        assert_eq!(
            last_turn_usage(&then_a_call),
            Some(48_582),
            "the next call is the newer reading"
        );
    }

    #[test]
    fn an_api_error_line_is_not_a_reading_of_zero() {
        let transcript = [assistant_line(390, 0, 500_000), assistant_line(0, 0, 0)].join("\n");
        assert_eq!(last_turn_usage(&transcript), Some(500_390));
        assert_eq!(last_turn_usage(&assistant_line(0, 0, 0)), None);
    }
}
