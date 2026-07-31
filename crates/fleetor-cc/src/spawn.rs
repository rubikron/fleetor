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

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub const DEEPSEEK_ANTHROPIC_BASE_URL: &str = "https://api.deepseek.com/anthropic";
pub const MODEL_FLASH: &str = "deepseek-v4-flash";

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
        }
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

        // Isolated, deterministic environment. We do NOT clear the whole env
        // (PATH etc. are needed) — we override exactly the Claude/DeepSeek vars
        // and repoint the config dir.
        cmd.env("CLAUDE_CONFIG_DIR", &self.config_dir)
            .env("ANTHROPIC_BASE_URL", &self.base_url)
            .env("ANTHROPIC_AUTH_TOKEN", &self.api_key)
            .env("ANTHROPIC_API_KEY", &self.api_key)
            .env("ANTHROPIC_MODEL", &self.model)
            // Strip any inherited interactive-session hints that could bias runs.
            .env_remove("ANTHROPIC_DEFAULT_OPUS_MODEL")
            .env_remove("ANTHROPIC_DEFAULT_SONNET_MODEL");

        if let Some(effort) = &self.effort_level {
            cmd.env("CLAUDE_CODE_EFFORT_LEVEL", effort);
        }

        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
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
