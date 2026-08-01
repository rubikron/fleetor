//! Building the `claude` worker command: isolated config + DeepSeek env.
//!
//! This is the one place that knows *how a FLEETOR worker process differs from a
//! developer's interactive `claude`*. Two invariants live here:
//!
//!  1. **Config isolation.** A worker must NOT inherit the operator's personal
//!     `~/.claude` (plugins, MCP servers, hooks, global CLAUDE.md). Phase 0
//!     measured that leak at ~10k extra input tokens/turn and a polluted tool
//!     surface. `CLAUDE_CONFIG_DIR` points every worker at a fleet-owned dir.
//!  2. **No permission wedge.** A headless worker has no TTY to answer a prompt
//!     on. The probe uses `acceptEdits` + an explicit allow-list; the real
//!     worker (Phase 1+) swaps in a PreToolUse path-guard hook so auto-approve
//!     never exceeds the worktree + declared files (Tier 1.8).

use anyhow::{Context, Result};
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub const DEEPSEEK_ANTHROPIC_BASE_URL: &str = "https://api.deepseek.com/anthropic";
pub const MODEL_FLASH: &str = "deepseek-v4-flash";

/// The MCP server key the shim registers under. **Must** be `fleet` so the model
/// sees the tools as `mcp__fleet__ask_lead` etc. (phase2-spikes §A; matches
/// `fleetor_shim::protocol::SERVER_NAME`).
pub const FLEET_MCP_KEY: &str = "fleet";
/// The generated MCP config file, written into the isolated config dir.
pub const FLEET_MCP_CONFIG_FILE: &str = "fleet-mcp.json";

/// The socket + Stop-hook wiring that turns an isolated worker into a *fleet*
/// worker (phase2-spikes "wiring checklist"). Optional on [`WorkerConfig`]: the
/// probe and the Phase 1–3 supervisor paths run without it; the Phase 4 runner
/// supplies it so the worker's `claude` spawns the shim (MCP → hub) and runs the
/// Stop hook (mid-turn mail). Held by value so a config stays cheap to clone.
#[derive(Debug, Clone)]
pub struct FleetWiring {
    /// Absolute path to the `fleetor-shim` binary CC spawns for MCP + the hook.
    pub shim_path: PathBuf,
    /// The fleet unix socket (`FLEET_SOCKET`) both the shim and the hook dial.
    pub socket_path: PathBuf,
    /// This worker's slot (`FLEETOR_SLOT`, 1..=N) — its identity on the socket.
    pub slot: u8,
    /// The fleet dir (`~/.fleetor/<key>/`) exposed via `--add-dir` so the worker
    /// can read tickets and knowledge (handoff §2).
    pub fleet_dir: PathBuf,
}

/// Everything needed to spawn one isolated worker `claude` process.
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// Working directory (a worktree in production; the toy repo for the probe).
    pub cwd: PathBuf,
    /// Isolated `CLAUDE_CONFIG_DIR` — never the operator's `~/.claude`.
    pub config_dir: PathBuf,
    /// DeepSeek API key (from the repo's gitignored `.env`, never hard-coded).
    pub api_key: String,
    /// Anthropic-compatible base URL.
    pub base_url: String,
    /// Model id for this worker's main loop (Flash for workers).
    pub model: String,
    /// `CLAUDE_CODE_EFFORT_LEVEL` (DeepSeek recommends `max`).
    pub effort_level: Option<String>,
    /// Tools the worker may call without a prompt.
    pub allowed_tools: Vec<String>,
    /// Permission mode passed to `--permission-mode`.
    pub permission_mode: String,
    /// Fleet socket + Stop-hook wiring (Phase 4). `None` for the probe and the
    /// Phase 1–3 supervisor paths, which never touch the hub.
    pub wiring: Option<FleetWiring>,
}

impl WorkerConfig {
    /// Probe/default posture: Flash, `max` effort, coding tool surface,
    /// `acceptEdits` (no wedge on a throwaway repo).
    pub fn probe(cwd: PathBuf, config_dir: PathBuf, api_key: String) -> Self {
        Self {
            cwd,
            config_dir,
            api_key,
            base_url: DEEPSEEK_ANTHROPIC_BASE_URL.to_string(),
            model: MODEL_FLASH.to_string(),
            effort_level: Some("max".to_string()),
            allowed_tools: [
                "Read", "Write", "Edit", "MultiEdit", "Bash", "Grep", "Glob", "LS",
                "TodoWrite",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            permission_mode: "acceptEdits".to_string(),
            wiring: None,
        }
    }

    /// Attach fleet wiring, returning the wired config (immutable builder). The
    /// caller must run [`write_fleet_config`] once before spawning so the
    /// generated MCP config and Stop-hook settings exist on disk.
    ///
    /// [`write_fleet_config`]: WorkerConfig::write_fleet_config
    pub fn with_wiring(mut self, wiring: FleetWiring) -> Self {
        self.wiring = Some(wiring);
        self
    }

    /// Materialize the on-disk artifacts a wired worker needs in its isolated
    /// config dir (phase2-spikes "wiring checklist"): the `fleet` MCP server
    /// config (`--mcp-config`) and a `settings.json` registering the `Stop` hook.
    /// Idempotent; a no-op when unwired. Call once before [`supervised_command`].
    ///
    /// [`supervised_command`]: WorkerConfig::supervised_command
    pub fn write_fleet_config(&self) -> Result<()> {
        let Some(w) = &self.wiring else { return Ok(()) };
        std::fs::create_dir_all(&self.config_dir)
            .with_context(|| format!("creating config dir {:?}", self.config_dir))?;

        // The shim carries slot + socket in its own env too, so the model's MCP
        // calls resolve even if CC ever stops forwarding process env to servers.
        let mcp = serde_json::json!({
            "mcpServers": {
                FLEET_MCP_KEY: {
                    "type": "stdio",
                    "command": w.shim_path,
                    "env": {
                        "FLEET_SOCKET": w.socket_path,
                        "FLEETOR_SLOT": w.slot.to_string(),
                    },
                }
            }
        });
        std::fs::write(
            self.config_dir.join(FLEET_MCP_CONFIG_FILE),
            serde_json::to_vec_pretty(&mcp).context("serializing MCP config")?,
        )
        .context("writing fleet MCP config")?;

        // The Stop hook drains queued mail at turn end (`<shim> stop-hook`); an
        // empty queue emits nothing so the turn ends normally (D-014).
        let settings = serde_json::json!({
            "hooks": {
                "Stop": [ { "hooks": [ {
                    "type": "command",
                    "command": format!("{} stop-hook", shell_quote(&w.shim_path)),
                } ] } ]
            }
        });
        std::fs::write(
            self.config_dir.join("settings.json"),
            serde_json::to_vec_pretty(&settings).context("serializing settings")?,
        )
        .context("writing Stop-hook settings")?;
        Ok(())
    }

    /// Build a one-shot headless command that runs `prompt` to completion and
    /// emits `stream-json` on stdout. `-p` = print/non-interactive; a turn ends
    /// with a `result` event.
    pub fn command(&self, prompt: &str) -> Command {
        let mut cmd = Command::new("claude");
        cmd.current_dir(&self.cwd)
            .arg("-p")
            .arg(prompt)
            .arg("--output-format")
            .arg("stream-json")
            .arg("--verbose") // required for stream-json under -p
            .arg("--model")
            .arg(&self.model)
            .arg("--permission-mode")
            .arg(&self.permission_mode);

        if !self.allowed_tools.is_empty() {
            cmd.arg("--allowedTools").arg(self.allowed_tools.join(","));
        }

        // Isolated, deterministic environment (see `apply_env`).
        self.apply_env(&mut cmd);

        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    /// Build a **supervised** streaming command (Phase 1). Unlike [`command`],
    /// no prompt is passed as an argument: the process reads user messages as
    /// `stream-json` on stdin and stays alive across turns, so the supervisor
    /// can assign, detect turn end via the `result` event, then reprompt or
    /// assign again on the same session. stdin is piped, not null.
    ///
    /// [`command`]: WorkerConfig::command
    pub fn supervised_command(&self) -> Command {
        let mut cmd = Command::new("claude");
        cmd.current_dir(&self.cwd)
            .arg("-p")
            .arg("--input-format")
            .arg("stream-json")
            .arg("--output-format")
            .arg("stream-json")
            .arg("--verbose") // required for stream-json under -p
            .arg("--model")
            .arg(&self.model)
            .arg("--permission-mode")
            .arg(&self.permission_mode);

        // A wired worker exposes the fleet MCP surface and can read the fleet dir;
        // `mcp__fleet` joins the allow-list so headless auto-approval covers the
        // fleet tools (they are *not* covered by `--permission-mode`, phase2-spikes).
        let mut allowed = self.allowed_tools.clone();
        if let Some(w) = &self.wiring {
            cmd.arg("--mcp-config").arg(self.config_dir.join(FLEET_MCP_CONFIG_FILE));
            cmd.arg("--add-dir").arg(&w.fleet_dir);
            allowed.push(format!("mcp__{FLEET_MCP_KEY}"));
        }
        if !allowed.is_empty() {
            cmd.arg("--allowedTools").arg(allowed.join(","));
        }

        self.apply_env(&mut cmd);

        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    /// Apply the isolated Claude/DeepSeek environment shared by both command
    /// forms. We override exactly the vars that matter and repoint the config
    /// dir; PATH and friends are inherited.
    fn apply_env(&self, cmd: &mut Command) {
        cmd.env("CLAUDE_CONFIG_DIR", &self.config_dir)
            .env("ANTHROPIC_BASE_URL", &self.base_url)
            .env("ANTHROPIC_AUTH_TOKEN", &self.api_key)
            .env("ANTHROPIC_API_KEY", &self.api_key)
            .env("ANTHROPIC_MODEL", &self.model)
            .env_remove("ANTHROPIC_DEFAULT_OPUS_MODEL")
            .env_remove("ANTHROPIC_DEFAULT_SONNET_MODEL");

        if let Some(effort) = &self.effort_level {
            cmd.env("CLAUDE_CODE_EFFORT_LEVEL", effort);
        }

        // Fleet identity for the shim (MCP server) and the Stop hook, both of
        // which CC spawns as children and hands this process's environment.
        if let Some(w) = &self.wiring {
            cmd.env("FLEET_SOCKET", &w.socket_path)
                .env("FLEETOR_SLOT", w.slot.to_string());
        }
    }

    /// The env pairs this config applies, for logging/inspection (key order
    /// stable). Value of the API key is redacted.
    pub fn env_summary(&self) -> Vec<(String, OsString)> {
        vec![
            ("CLAUDE_CONFIG_DIR".into(), self.config_dir.clone().into()),
            ("ANTHROPIC_BASE_URL".into(), self.base_url.clone().into()),
            ("ANTHROPIC_MODEL".into(), self.model.clone().into()),
            ("ANTHROPIC_AUTH_TOKEN".into(), "<redacted>".into()),
        ]
    }
}

/// Minimal shell quoting for the Stop-hook command string (CC runs it via the
/// shell). Wraps in single quotes and escapes any embedded single quote, so a
/// shim path with spaces survives. Sufficient for the paths we generate.
fn shell_quote(path: &std::path::Path) -> String {
    let s = path.to_string_lossy();
    format!("'{}'", s.replace('\'', "'\\''"))
}
