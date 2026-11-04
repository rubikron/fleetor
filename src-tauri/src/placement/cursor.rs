//! Cursor's `cursor-agent`: its spec, its `Harness` impl and its diagnostic
//! (WP-24). Measured in `docs/notes/cursor-spike-notes.md`; D-128, D-129, D-130.

use std::path::{Path, PathBuf};

use fleetor_core::event::NoticeLevel;

use super::claude_code::{first_on_path, install_json_hooks, vendor_version};
use super::harness::*;
use crate::guardrail;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cursor;

pub static CURSOR: Cursor = Cursor;

const SEED_FILE: &str = "cli-config.json";
const TRUST_FILE: &str = "projects/{key}/.workspace-trusted";
const CHATS: &str = "chats";
const TRANSCRIPT_EXT: &str = "db";

pub const CURSOR_SPEC: HarnessSpec = HarnessSpec {
    name: "cursor",
    mark: "CR",

    login: LoginInstruction {
        command: "cursor-agent login",
        then: None,
        credential_home: "your login keychain",
    },

    program: Program { bin: "cursor-agent", base_args: &[] },

    // No argv carrier: an `alwaysApply` rule in the checkout reaches the model and
    // survives `/clear`, appended under Cursor's own prompt (D-130).
    brief: BriefCarrier {
        argv_flag: None,
        config_key: None,
        replaces_system_prompt: false,
        writes_into_worktree: true,
        worktree_file: Some(".cursor/rules/fleetor.mdc"),
        // Without `alwaysApply` the rule is never attached and the model runs on
        // cursor's prompt alone.
        worktree_header: "---\ndescription: fleet brief\nalwaysApply: true\n---\n",
    },

    posture: Posture {
        model_env: None,
        model_flag: Some("--model"),
        declared_models: &[("auto", "Auto")],
        permission_flag: None,
        sandbox_keys: &[],
        verified_as: &[],
        verified_against_schema: None,
        bounded_by_guardrail: true,
    },

    // `approvalMode: allowlist` with `Shell(*)` allowed: the pane never prompts,
    // and the guardrail refuses writes outside its roots (D-129).
    config_dir: ConfigDir {
        env_var: "CURSOR_CONFIG_DIR",
        seed_file: SEED_FILE,
        seed_keys: &["approvalMode", "permissions"],
        seed_merges: true,
    },

    credentials: Credentials { scrubbed_env: &["CURSOR_API_KEY", "CURSOR_AUTH_TOKEN"] },

    // The login is a keychain item only reachable on the operator's own HOME, so
    // there is no private HOME (D-128). `CURSOR_DATA_DIR` holds trust and project
    // state; it is pointed at the same directory.
    isolation: ConfigAndCredentialIsolation {
        config_env: "CURSOR_DATA_DIR",
        credential_env: None,
        operator_store_follows_home: true,
        private_home: false,
        fenced_env: &[],
        seeds_from_operator: false,
    },

    // Cursor honours a deny only from Claude Code's settings files with the event
    // spelled `PreToolUse` (D-129).
    guardrail: GuardrailInstall {
        settings_file: ".claude/settings.local.json",
        hook_event: "PreToolUse",
        write_tools: &["Shell", "Write", "Delete"],
        hook_file: guardrail::HOOK_FILE,
        settings_in_worktree: true,
    },

    outbound: Outbound { sandboxed: false, socket_reachable: true, reachability_keys: &[] },

    // Unmeasured: whether input sent before the composer paints is kept. Phase 4's
    // live round trip is the check.
    bring_up: BringUp::AtOnce,

    typing: TypingProfile {
        bracketed_paste: true,
        paste_start: b"\x1b[200~",
        paste_end: b"\x1b[201~",
        submit_bytes: b"\r",
        submit_gap_ms: 30,
    },

    commands: CommandChannel { spellings: &[("/clear", "/clear"), ("/compact", "/compact")] },

    // The usage figure lives in the SQLite chat store, which has no reader yet.
    gauge: GaugeSource { reads_transcript: false, window_tokens: None },

    orphans: OrphanNames { comm_suffixes: &["cursor-agent"] },

    transcript: Transcript {
        subdir: CHATS,
        file_ext: TRANSCRIPT_EXT,
        format: "cursor-chat-store-sqlite",
        transport: Transport::SqliteBackup,
        depth: 2,
    },

    project_identity: ProjectIdentityAndTrust {
        canonicalize: true,
        exact_path_match: true,
        trust_file: TRUST_FILE,
        trust_keys: &["workspacePath", "trustedAt"],
    },

    resume: Resume::Supported,
};

const NEEDLES: &[Needle] = &[
    ("cp 4 — config_dir.env_var", "CURSOR_CONFIG_DIR", |s| vec![s.config_dir.env_var]),
    ("cp 4 — config_dir.seed_file", "cli-config.json", |s| vec![s.config_dir.seed_file]),
    ("cp 5 — credentials.scrubbed_env", "CURSOR_API_KEY", |s| {
        s.credentials.scrubbed_env.to_vec()
    }),
    ("cp 6 — isolation.config_env", "CURSOR_DATA_DIR", |s| vec![s.isolation.config_env]),
    ("cp 2 — brief.worktree_file", ".cursor/rules/fleetor.mdc", |s| {
        s.brief.worktree_file.into_iter().collect()
    }),
    ("cp 7 — guardrail.settings_file", ".claude/settings.local.json", |s| {
        vec![s.guardrail.settings_file]
    }),
    ("cp 13 — transcript.format", "cursor-chat-store-sqlite", |s| vec![s.transcript.format]),
    ("cp 14 — project_identity.trust_file", "projects/{key}/.workspace-trusted", |s| {
        vec![s.project_identity.trust_file]
    }),
];

impl Harness for Cursor {
    fn spec(&self) -> &'static HarnessSpec {
        &CURSOR_SPEC
    }

    fn literal_needles(&self) -> &'static [Needle] {
        NEEDLES
    }

    /// The canonical cwd with every run of non-alphanumerics collapsed to one `-`
    /// and the ends trimmed: `/private/tmp/a/-b` keys as `private-tmp-a-b`.
    fn project_key(&self, cwd: &Path) -> String {
        let real = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
        let mut key = String::new();
        for c in real.to_string_lossy().chars() {
            if c.is_ascii_alphanumeric() {
                key.push(c);
            } else if !key.ends_with('-') {
                key.push('-');
            }
        }
        key.trim_matches('-').to_string()
    }

    fn seed_config_dir(&self, seed: &Seed<'_>) -> Result<Vec<(NoticeLevel, String)>, String> {
        let dir = seed.config_dir;
        std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;

        let path = dir.join(SEED_FILE);
        let mut root = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .filter(serde_json::Value::is_object)
            .unwrap_or_else(|| serde_json::json!({}));
        let object = root.as_object_mut().expect("just filtered to an object");
        object.insert("approvalMode".into(), "allowlist".into());
        object
            .insert("permissions".into(), serde_json::json!({ "allow": ["Shell(*)"], "deny": [] }));
        write_json(&path, &root)?;

        let real = std::fs::canonicalize(seed.cwd).unwrap_or_else(|_| seed.cwd.to_path_buf());
        let trust =
            dir.join(CURSOR_SPEC.project_identity.trust_file_for(&self.project_key(seed.cwd)));
        if let Some(parent) = trust.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        write_json(
            &trust,
            &serde_json::json!({
                "trustedAt": utc_now(),
                "workspacePath": real.to_string_lossy(),
                "trustMethod": "cli-flag",
            }),
        )?;
        Ok(Vec::new())
    }

    /// Cursor's credential is a keychain item read on the operator's own HOME
    /// (D-128), so nothing is planted: the document is empty and only says a
    /// login exists. `authInfo` in `~/.cursor/cli-config.json` is that evidence.
    fn read_operator_login(&self, operator_home: Option<&Path>) -> Option<OperatorLogin> {
        operator_auth(operator_home?).ok().flatten().map(|_| OperatorLogin::new(String::new()))
    }

    fn diagnose(&self, facts: &MachineFacts<'_>) -> HarnessReadiness {
        let spec = &CURSOR_SPEC;
        let invoked = spec.program.bin;
        let Some(resolved) = first_on_path(facts.path, invoked) else {
            return HarnessReadiness::not_installed(spec);
        };
        let through_the_name = vendor_version(Path::new(invoked), facts.path);
        let through_the_binary = vendor_version(&resolved, facts.path);
        let login = match facts.home {
            None => LoginState::Unreadable {
                why: "this machine reported no HOME, so `~/.cursor` could not be located".into(),
            },
            Some(home) => match operator_auth(home) {
                Ok(Some(_)) => LoginState::LoggedIn(AccountShape::SubscriptionPlan { plan: None }),
                Ok(None) => LoginState::NoCredential {
                    summary: format!(
                        "{} records no completed `cursor-agent login`",
                        home.join(".cursor").join(SEED_FILE).display(),
                    ),
                    provider_key: None,
                },
                Err(why) => LoginState::Unreadable { why },
            },
        };
        HarnessReadiness {
            harness: spec,
            invoked: invoked.to_string(),
            resolved: Some(resolved),
            readings_agree: through_the_name == through_the_binary,
            version: through_the_binary,
            login,
            provider: None,
            models: spec
                .posture
                .declared_models
                .iter()
                .map(|(slug, display_name)| ModelChoice {
                    slug: (*slug).to_string(),
                    display_name: (*display_name).to_string(),
                })
                .collect(),
            posture: ResolvedPosture::default(),
        }
    }

    fn install_guardrail(
        &self,
        at: &crate::guardrail::GuardrailPlacement<'_>,
    ) -> Result<Vec<(NoticeLevel, String)>, String> {
        install_json_hooks(&self.spec().guardrail, at)
    }

    fn command_args(&self, seat: &Seat<'_>) -> Vec<String> {
        let spec = self.spec();
        let mut args: Vec<String> =
            spec.program.base_args.iter().map(|a| (*a).to_string()).collect();
        if let (Some(flag), Some(model)) = (spec.posture.model_flag, seat.model) {
            args.push(flag.to_string());
            args.push(model.to_string());
        }
        args
    }

    /// `--resume <chat id>` keeps the chat's history (spike notes). The brief
    /// is the rule file in the checkout, rewritten on every placement.
    fn resume_args(&self, seat: &Seat<'_>, session_id: &str) -> Option<Vec<String>> {
        let mut args = self.command_args(seat);
        args.push("--resume".to_string());
        args.push(session_id.to_string());
        Some(args)
    }

    /// The newest chat with a store under `chats/<cwd hash>/<chat id>/`.
    fn session_id(&self, seat_dir: &Path) -> Option<String> {
        let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
        for file in chat_stores(seat_dir) {
            let Ok(at) = std::fs::metadata(&file).and_then(|m| m.modified()) else { continue };
            if newest.as_ref().is_none_or(|(best, _)| at > *best) {
                newest = Some((at, file));
            }
        }
        let file = newest?.1;
        file.parent()?.file_name()?.to_str().map(str::to_string)
    }

    fn has_session(&self, seat_dir: &Path, session_id: &str) -> bool {
        chat_stores(seat_dir).iter().any(|file| {
            file.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str()) == Some(session_id)
        })
    }

    fn read_usage(
        &self,
        _source: &crate::context_gauge::TranscriptSource,
    ) -> Option<crate::context_gauge::UsageReading> {
        None
    }
}

fn chat_stores(seat_dir: &Path) -> Vec<PathBuf> {
    let t = &CURSOR_SPEC.transcript;
    crate::runs::transcript_files_within(&seat_dir.join(t.subdir), t.file_ext, t.depth)
}

/// The operator's `authInfo`, `Ok(None)` when there is no login on record.
fn operator_auth(home: &Path) -> Result<Option<serde_json::Value>, String> {
    let file = home.join(".cursor").join(SEED_FILE);
    let text = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{} could not be read: {e}", file.display())),
    };
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("{} is not readable as JSON: {e}", file.display()))?;
    Ok(value.get("authInfo").filter(|a| a.is_object()).cloned())
}

fn write_json(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|e| format!("encode: {e}"))?;
    let tmp = path.with_extension("fleetor-tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("install {}: {e}", path.display()))
}

/// Now as `YYYY-MM-DDTHH:MM:SS.000Z`, the shape Cursor writes `trustedAt` in.
fn utc_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.000Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_project_key_collapses_separator_runs_as_cursor_does() {
        // Measured: `/…/claude-503/-Users-…/same2/repo` keyed as `…claude-503-Users-…`.
        assert_eq!(
            CURSOR.project_key(Path::new("/nowhere/a-503/-Users/b.c")),
            "nowhere-a-503-Users-b-c"
        );
    }

    #[test]
    fn utc_now_has_cursors_shape() {
        let now = utc_now();
        assert_eq!(now.len(), 24, "{now}");
        assert!(now.ends_with(".000Z") && now.as_bytes()[10] == b'T', "{now}");
    }
}
