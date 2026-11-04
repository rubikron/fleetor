//! opencode: its spec, its `Harness` impl, its probe and its seat config (WP-32, D-140–D-150).

use std::path::Path;

use fleetor_core::event::NoticeLevel;
use serde_json::{json, Map, Value};

use super::claude_code::{first_on_path, project_key, vendor_version};
use super::harness::*;
use crate::guardrail;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OpenCode;

pub(super) static OPENCODE: OpenCode = OpenCode;

pub fn opencode() -> &'static dyn Harness {
    &OPENCODE
}

/// The custom primary agent that carries the brief (D-145).
const AGENT: &str = "fleetor";
const BRIEF_FILE: &str = "brief.md";
/// Where the operator's own config lives, under their `HOME` (D-146).
const OPERATOR_CONFIG: &str = ".config/opencode/opencode.json";
/// The runtime override, which outranks a target repo's `opencode.json` (D-147).
const CONTENT_ENV: &str = "OPENCODE_CONFIG_CONTENT";
/// The shape `debug agents` reports a resolved posture in (D-149).
const AGENTS_SCHEMA: &str = "agents[].permissions[action,resource,effect]";

pub const OPENCODE_SPEC: HarnessSpec = HarnessSpec {
    name: "opencode",
    mark: "OC",
    login: LoginInstruction {
        command: "opencode auth login",
        then: None,
        credential_home: "opencode's own database in its data directory",
    },
    // D-144 as amended by D-149: `--standalone` so the seat's environment reaches
    // config; the posture is config, not `--auto`.
    program: Program { bin: "opencode", base_args: &["--standalone"] },
    brief: BriefCarrier {
        argv_flag: None,
        config_key: Some("prompt"),
        replaces_system_prompt: true,
        writes_into_worktree: false,
    },
    posture: Posture {
        model_env: None,
        declared_models: &[],
        model_flag: None,
        permission_flag: None,
        sandbox_keys: &[("permission.*", "allow"), ("permission.external_directory", "deny")],
        verified_as: &[
            PostureExpectation { row: "approval", written_key: "permission.*", resolved: "allow" },
            PostureExpectation {
                row: "filesystem",
                written_key: "permission.external_directory",
                resolved: "deny",
            },
        ],
        verified_against_schema: Some(AGENTS_SCHEMA),
    },
    config_dir: ConfigDir {
        env_var: "OPENCODE_CONFIG_DIR",
        seed_file: "opencode.json",
        seed_keys: &["default_agent", "agent"],
        seed_merges: false,
    },
    credentials: Credentials {
        scrubbed_env: &["OPENCODE_CONFIG", "OPENCODE_DB", "OPENCODE_PERMISSION"],
    },
    // D-143: the operator's real data dir holds the login; only config moves.
    isolation: ConfigAndCredentialIsolation {
        config_env: "OPENCODE_CONFIG_DIR",
        credential_env: None,
        operator_store_follows_home: false,
        private_home: false,
        fenced_env: &[],
        seeds_from_operator: true,
    },
    guardrail: GuardrailInstall {
        settings_file: "plugins/fleetor-guardrail.js",
        hook_event: "execute.before",
        write_tools: &["shell", "write", "edit", "patch", "worktree_remove"],
        hook_file: guardrail::HOOK_FILE,
    },
    outbound: Outbound { sandboxed: false, socket_reachable: true, reachability_keys: &[] },
    bring_up: BringUp::AtOnce,
    typing: TypingProfile {
        bracketed_paste: true,
        paste_start: b"\x1b[200~",
        paste_end: b"\x1b[201~",
        submit_bytes: b"\r",
        submit_gap_ms: 30,
    },
    commands: CommandChannel { spellings: &[("/clear", "/new"), ("/compact", "/compact")] },
    // D-148, slice 1.
    gauge: GaugeSource { reads_transcript: false, window_tokens: None },
    orphans: OrphanNames { comm_suffixes: &["opencode"] },
    transcript: Transcript {
        subdir: "sessions",
        file_ext: "json",
        format: "opencode-export-json",
        transport: Transport::Rename,
    },
    // D-150: no first-run dialog, measured.
    project_identity: ProjectIdentityAndTrust {
        canonicalize: true,
        exact_path_match: true,
        first_run_gate: false,
        trust_file: "",
        trust_keys: &[],
    },
    resume: Resume::NotSupported {
        why: "opencode keeps a seat's sessions in the operator's own database, and this build \
              records no pointer to them yet",
    },
};

const NEEDLES: &[Needle] = &[
    ("cp 1 — program.base_args", "--standalone", |s| s.program.base_args.to_vec()),
    ("cp 3 — posture.sandbox_keys", "permission.external_directory", |s| {
        s.posture.sandbox_keys.iter().map(|(k, _)| *k).collect()
    }),
    ("cp 4 — config_dir.env_var", "OPENCODE_CONFIG_DIR", |s| vec![s.config_dir.env_var]),
    ("cp 4 — config_dir.seed_file", "opencode.json", |s| vec![s.config_dir.seed_file]),
    ("cp 4 — config_dir.seed_keys", "default_agent", |s| s.config_dir.seed_keys.to_vec()),
    ("cp 5 — credentials.scrubbed_env", "OPENCODE_PERMISSION", |s| {
        s.credentials.scrubbed_env.to_vec()
    }),
    ("cp 7 — guardrail.settings_file", "plugins/fleetor-guardrail.js", |s| {
        vec![s.guardrail.settings_file]
    }),
    ("cp 7 — guardrail.hook_event", "execute.before", |s| vec![s.guardrail.hook_event]),
    ("cp 13 — transcript.format", "opencode-export-json", |s| vec![s.transcript.format]),
];

/// The seat's own config: the brief's agent, made the default, and the posture
/// keys off the spec. `model` only when the gate picked one (D-141).
fn seat_config(prompt: &str, model: Option<&str>) -> Value {
    let mut root = json!({
        "default_agent": AGENT,
        "agent": { AGENT: { "description": "FLEETOR seat", "mode": "primary", "prompt": prompt } },
    });
    if let Some(model) = model {
        root["model"] = json!(model);
    }
    for (key, value) in OPENCODE_SPEC.posture.sandbox_keys {
        set_path(&mut root, key, json!(value));
    }
    root
}

fn set_path(root: &mut Value, dotted: &str, value: Value) {
    let mut at = root;
    let segments: Vec<&str> = dotted.split('.').collect();
    for segment in &segments[..segments.len() - 1] {
        if !at.get(*segment).is_some_and(Value::is_object) {
            at[*segment] = Value::Object(Map::new());
        }
        at = at.get_mut(*segment).expect("just ensured");
    }
    at[segments[segments.len() - 1]] = value;
}

fn write_atomically(file: &Path, text: &str) -> Result<(), String> {
    let tmp = file.with_extension("tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, file).map_err(|e| format!("install {}: {e}", file.display()))
}

/// The guardrail plugin (D-142): shapes opencode's tool input into what
/// `write_guardrail.py` reads, runs it, and throws on a refusal.
const PLUGIN: &str = r#"import { spawnSync } from 'node:child_process'
import { resolve } from 'node:path'

const COMMAND = __COMMAND__
const EVENT = __EVENT__
const PATCH_HEADER = /^\*\*\* (?:Add File|Update File|Delete File|Move to): (.+)$/gm

function shaped(tool, input, cwd) {
  if (tool === 'shell') return [{ command: input.command }, input.workdir ? resolve(cwd, input.workdir) : cwd]
  if (tool === 'patch') return [{ edits: [...String(input.patchText ?? '').matchAll(PATCH_HEADER)].map((m) => ({ file_path: m[1].trim() })) }, cwd]
  if (tool === 'worktree_remove') return [{ path: input.directory }, cwd]
  return [input, cwd]
}

export default {
  id: 'fleetor-guardrail',
  async setup(ctx) {
    ctx.tool.hook(EVENT, async (event) => {
      const tool = typeof event.tool === 'string' ? event.tool : event.tool?.name
      const [tool_input, cwd] = shaped(tool, event.input ?? {}, process.cwd())
      const run = spawnSync('/bin/sh', ['-c', COMMAND], {
        input: JSON.stringify({ tool_name: tool, tool_input, cwd }),
        encoding: 'utf8',
      })
      let answer
      try {
        answer = JSON.parse(run.stdout)
      } catch {
        throw new Error('FLEETOR write guardrail: the guardrail itself failed and refused this call (' +
          (run.error?.message || run.stderr || 'no answer') + '). Tell the operator with `fleet send operator`.')
      }
      const out = answer?.hookSpecificOutput
      if (out?.permissionDecision === 'deny') throw new Error(out.permissionDecisionReason)
    })
  },
}
"#;

impl Harness for OpenCode {
    fn spec(&self) -> &'static HarnessSpec {
        &OPENCODE_SPEC
    }

    fn literal_needles(&self) -> &'static [Needle] {
        NEEDLES
    }

    fn project_key(&self, cwd: &Path) -> String {
        project_key(cwd)
    }

    fn seed_config_dir(&self, seed: &Seed<'_>) -> Result<Vec<(NoticeLevel, String)>, String> {
        if seed.brief.trim().is_empty() {
            return Err("an opencode seat needs its brief before it is seeded".to_string());
        }
        let dir = seed.config_dir;
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("create config dir {}: {e}", dir.display()))?;
        let mut notices = Vec::new();

        let snapshot = seed.operator_home.map(|home| home.join(OPERATOR_CONFIG));
        let mut root = match snapshot.as_deref().map(std::fs::read_to_string) {
            Some(Ok(text)) => match serde_json::from_str::<Value>(&text) {
                Ok(value) if value.is_object() => value,
                _ => {
                    notices.push((
                        NoticeLevel::Warn,
                        format!(
                            "{} is not plain JSON, so this opencode seat starts without your \
                             providers from it",
                            snapshot
                                .as_deref()
                                .map(|p| p.display().to_string())
                                .unwrap_or_default(),
                        ),
                    ));
                    Value::Object(Map::new())
                }
            },
            _ => Value::Object(Map::new()),
        };

        let brief = dir.join(BRIEF_FILE);
        std::fs::write(&brief, seed.brief)
            .map_err(|e| format!("write {}: {e}", brief.display()))?;
        let seat = seat_config(&format!("{{file:{}}}", brief.display()), None);
        for (key, value) in seat.as_object().expect("an object") {
            root[key] = value.clone();
        }

        let text =
            serde_json::to_string_pretty(&root).map_err(|e| format!("encode config: {e}"))?;
        write_atomically(&dir.join(OPENCODE_SPEC.config_dir.seed_file), &text)?;
        Ok(notices)
    }

    /// D-140: the login is opencode's own store, so there is nothing to carry.
    fn read_operator_login(&self, _operator_home: Option<&Path>) -> Option<OperatorLogin> {
        Some(OperatorLogin::new(String::new()))
    }

    fn diagnose(&self, facts: &MachineFacts<'_>) -> HarnessReadiness {
        let spec = &OPENCODE_SPEC;
        let invoked = spec.program.bin;
        let Some(resolved) = first_on_path(facts.path, invoked) else {
            return HarnessReadiness::not_installed(spec);
        };
        let through_the_name = vendor_version(Path::new(invoked), facts.path);
        let through_the_binary = vendor_version(&resolved, facts.path);
        HarnessReadiness {
            harness: spec,
            invoked: invoked.to_string(),
            readings_agree: through_the_name == through_the_binary,
            version: through_the_binary,
            login: LoginState::LoggedIn(AccountShape::CustomProvider {
                name: "opencode's own providers".to_string(),
                env_var: None,
            }),
            provider: None,
            models: catalog(&resolved, facts),
            posture: resolved_posture(&resolved, facts),
            resolved: Some(resolved),
        }
    }

    fn install_guardrail(
        &self,
        at: &crate::guardrail::GuardrailPlacement<'_>,
    ) -> Result<Vec<(NoticeLevel, String)>, String> {
        let spec = &self.spec().guardrail;
        let (command, notices) = guardrail::prepare(spec, at)?;
        let plugin = at.config_dir.join(spec.settings_file);
        if let Some(parent) = plugin.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        let text = PLUGIN
            .replace("__COMMAND__", &json!(command).to_string())
            .replace("__EVENT__", &json!(spec.hook_event).to_string());
        guardrail::install_document(spec, at.config_dir, &text)?;
        Ok(notices)
    }

    fn command_args(&self, _seat: &Seat<'_>) -> Vec<String> {
        self.spec().program.base_args.iter().map(|a| (*a).to_string()).collect()
    }

    /// The brief rides inline: `CONTENT` outranks the seed file and a target
    /// repo's own `opencode.json` (D-147).
    fn pane_env(&self, seat: &Seat<'_>) -> Vec<(String, String)> {
        vec![(CONTENT_ENV.to_string(), seat_config(seat.brief, seat.model).to_string())]
    }

    fn resume_args(&self, _seat: &Seat<'_>, _session_id: &str) -> Option<Vec<String>> {
        None
    }

    fn session_id(&self, _seat_dir: &Path) -> Option<String> {
        None
    }

    fn has_session(&self, _seat_dir: &Path, _session_id: &str) -> bool {
        false
    }

    fn read_usage(
        &self,
        _source: &crate::context_gauge::TranscriptSource,
    ) -> Option<crate::context_gauge::UsageReading> {
        None
    }
}

/// One probe command, shaped like a seat: the seat's config applied, the
/// scrubbed names removed.
fn probe(bin: &Path, args: &[&str], facts: &MachineFacts<'_>) -> Option<String> {
    let mut cmd = std::process::Command::new(bin);
    cmd.args(args)
        .env("PATH", facts.path)
        .env(CONTENT_ENV, seat_config("probe", None).to_string())
        .current_dir(facts.home.unwrap_or(Path::new("/")));
    for name in OPENCODE_SPEC.credentials.scrubbed_env {
        cmd.env_remove(name);
    }
    let out = cmd.output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn catalog(bin: &Path, facts: &MachineFacts<'_>) -> Vec<ModelChoice> {
    probe(bin, &["models"], facts)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| line.contains('/') && !line.contains(' '))
        .map(|slug| ModelChoice { slug: slug.to_string(), display_name: slug.to_string() })
        .collect()
}

/// `debug agents` with the seat's config, read back for the `fleetor` agent (D-149).
fn resolved_posture(bin: &Path, facts: &MachineFacts<'_>) -> ResolvedPosture {
    probe(bin, &["debug", "agents", "--standalone"], facts)
        .map(|text| posture_of(&text))
        .unwrap_or_default()
}

/// The last rule matching an action wins, as opencode evaluates them.
fn posture_of(report: &str) -> ResolvedPosture {
    let Ok(Value::Array(agents)) = serde_json::from_str::<Value>(report) else {
        return ResolvedPosture::default();
    };
    let Some(rules) =
        agents.iter().find(|a| a["id"] == AGENT).and_then(|a| a["permissions"].as_array())
    else {
        return ResolvedPosture::default();
    };
    let shaped =
        rules.iter().all(|r| ["action", "resource", "effect"].iter().all(|k| r[*k].is_string()));
    let effect = |action: &str, resource: Option<&str>| {
        rules
            .iter()
            .rev()
            .find(|r| r["action"] == action && resource.is_none_or(|res| r["resource"] == res))
            .and_then(|r| r["effect"].as_str())
            .map(str::to_string)
    };
    ResolvedPosture {
        filesystem: effect("external_directory", None),
        network: None,
        approval: effect("*", Some("*")),
        schema: Some(if shaped { AGENTS_SCHEMA } else { "unrecognised" }.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seat_config_carries_the_agent_the_model_and_the_posture() {
        let config = seat_config("BRIEF", Some("p/m"));
        assert_eq!(config["default_agent"], AGENT);
        assert_eq!(config["agent"][AGENT]["prompt"], "BRIEF");
        assert_eq!(config["agent"][AGENT]["mode"], "primary");
        assert_eq!(config["model"], "p/m");
        assert_eq!(config["permission"]["*"], "allow");
        assert_eq!(config["permission"]["external_directory"], "deny");
        assert!(seat_config("BRIEF", None).get("model").is_none());
    }

    #[test]
    fn the_posture_is_the_last_matching_rule_off_the_fleetor_agent() {
        let report = json!([
            { "id": "build", "permissions": [{ "action": "*", "resource": "*", "effect": "ask" }] },
            { "id": AGENT, "permissions": [
                { "action": "*", "resource": "*", "effect": "allow" },
                { "action": "read", "resource": "*.env", "effect": "ask" },
                { "action": "*", "resource": "*", "effect": "allow" },
                { "action": "external_directory", "resource": "*", "effect": "deny" },
            ]},
        ])
        .to_string();
        let posture = posture_of(&report);
        assert_eq!(posture.approval.as_deref(), Some("allow"));
        assert_eq!(posture.filesystem.as_deref(), Some("deny"));
        assert_eq!(posture.schema.as_deref(), Some(AGENTS_SCHEMA));
        assert_eq!(posture_of("not json"), ResolvedPosture::default());
        assert_eq!(posture_of("[]").schema, None);
    }

    #[test]
    fn the_operators_config_is_snapshotted_and_the_seats_keys_win() {
        let scratch = std::env::temp_dir()
            .join(format!("fleetor-opencode-seed-{}", fleetor_core::time::now_ms()));
        let home = scratch.join("home");
        let operator = home.join(OPERATOR_CONFIG);
        std::fs::create_dir_all(operator.parent().unwrap()).unwrap();
        let mine = r#"{"provider":{"local":{"name":"x"}},"default_agent":"build"}"#;
        std::fs::write(&operator, mine).unwrap();
        let (config, cwd) = (scratch.join("seat"), scratch.join("work"));
        std::fs::create_dir_all(&cwd).unwrap();

        opencode()
            .seed_config_dir(&Seed::new(&config, &cwd, Some(&home)).with_brief("BRIEF"))
            .expect("seed");
        let seeded: Value =
            serde_json::from_str(&std::fs::read_to_string(config.join("opencode.json")).unwrap())
                .unwrap();
        assert_eq!(seeded["provider"]["local"]["name"], "x");
        assert_eq!(seeded["default_agent"], AGENT);
        assert!(seeded["agent"][AGENT]["prompt"].as_str().unwrap().starts_with("{file:"));
        assert_eq!(std::fs::read_to_string(config.join(BRIEF_FILE)).unwrap(), "BRIEF");
        assert_eq!(std::fs::read_to_string(&operator).unwrap(), mine);
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn a_seat_gets_no_prompt_and_no_auto_flag_in_argv() {
        let args = opencode().command_args(&Seat::new("BRIEF").with_model("p/m"));
        assert_eq!(args, vec!["--standalone"]);
    }
}
