//! The `AgentProcess` seam (BUILDING §3): real `claude` vs `fake-claude` vs a
//! future model. Both speak the identical `stream-json` wire protocol over
//! stdin/stdout, so the seam is just *which command to spawn* — the session
//! driver ([`crate::session`]) treats whatever it gets identically.

use crate::spawn::WorkerConfig;
use std::path::PathBuf;
use std::process::Command;

/// Anything that can be spawned as a stream-json agent the supervisor drives.
pub trait AgentProcess {
    /// A command configured to read `stream-json` on stdin and emit it on
    /// stdout (stdin piped, stdout piped, stderr piped). The session driver
    /// spawns it and owns the pipes.
    fn command(&self) -> Command;

    /// Short label for logs and events, e.g. `claude` or `fake:happy`.
    fn label(&self) -> String;
}

/// The real Claude Code worker: an isolated, DeepSeek-pointed `claude` in
/// streaming-input mode.
pub struct RealClaude {
    pub config: WorkerConfig,
}

impl AgentProcess for RealClaude {
    fn command(&self) -> Command {
        self.config.supervised_command()
    }
    fn label(&self) -> String {
        "claude".to_string()
    }
}

/// The scripted stand-in (`tests/fake-claude/fake-claude.mjs`). Same wire
/// protocol, deterministic, free — every supervision test runs against it
/// (BUILDING §5). The scenario is chosen by env var.
pub struct FakeClaude {
    pub script: PathBuf,
    pub cwd: PathBuf,
    pub scenario: String,
}

impl AgentProcess for FakeClaude {
    fn command(&self) -> Command {
        let mut cmd = Command::new("node");
        cmd.arg(&self.script)
            .current_dir(&self.cwd)
            .env("FAKE_CLAUDE_SCENARIO", &self.scenario)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        cmd
    }
    fn label(&self) -> String {
        format!("fake:{}", self.scenario)
    }
}
