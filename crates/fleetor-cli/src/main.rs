//! `fleetor` — headless driver for FLEETOR Phases 0–3.
//!
//! Phase 0: `fleetor probe` measures DeepSeek V4 Flash's tool-call fidelity
//! through real Claude Code before any supervision infrastructure is built.

mod probe;
mod quality;
mod supervise;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "fleetor", version, about = "FLEETOR headless driver")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Phase 0 fidelity probe: drive N tickets through one Flash worker each.
    Probe {
        /// Substrings of ticket ids to run (default: all 10).
        #[arg(long)]
        only: Vec<String>,
        /// Per-ticket wall-clock timeout in seconds.
        #[arg(long, default_value_t = 300)]
        timeout: u64,
        /// Report output path (a `.json` sibling is written too).
        #[arg(long, default_value = "docs/phase0-report.md")]
        out: PathBuf,
        /// Repo root (for locating `.env`); defaults to the current dir.
        #[arg(long)]
        repo_root: Option<PathBuf>,
    },
    /// Print the ticket list without running anything.
    Tickets,
    /// Phase 1 supervisor: drive one ticket through spawn→assign→report.
    Supervise {
        /// Use a real Claude Code Flash worker (costs tokens). Default: fake.
        #[arg(long)]
        real: bool,
        /// fake-claude scenario when not `--real`.
        #[arg(long, default_value = "happy", value_parser = ["happy", "no-report", "bad-report", "hang"])]
        scenario: String,
        /// Per-ticket wall-clock timeout in seconds.
        #[arg(long, default_value_t = 300)]
        timeout: u64,
        /// Repo root (for `.env` and the fake-claude script); defaults to cwd.
        #[arg(long)]
        repo_root: Option<PathBuf>,
    },
    /// Phase 3 quality loop: drive one ticket through gate → bounce → review.
    Quality {
        /// Worker scenario driving the gate outcome.
        #[arg(long, default_value = "qa-bounce", value_parser = ["qa-bounce", "qa-clean"])]
        scenario: String,
        /// Reviewer scenario.
        #[arg(long, default_value = "review-approve", value_parser = ["review-approve", "review-count"])]
        reviewer: String,
        /// Per-turn wall-clock timeout in seconds.
        #[arg(long, default_value_t = 30)]
        timeout: u64,
        /// Repo root (for the fake-claude script); defaults to cwd.
        #[arg(long)]
        repo_root: Option<PathBuf>,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Tickets => {
            for t in probe::tickets::all() {
                println!("{:<24} [{}]  targets: {}", t.id, t.category, t.targets);
            }
            Ok(())
        }
        Commands::Probe { only, timeout, out, repo_root } => {
            let repo_root = repo_root
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| PathBuf::from("."));
            let cc_version = claude_version();
            probe::run(probe::ProbeOptions {
                repo_root,
                only,
                timeout_secs: timeout,
                out,
                cc_version,
            })
        }
        Commands::Supervise { real, scenario, timeout, repo_root } => {
            let repo_root = repo_root
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| PathBuf::from("."));
            supervise::run(supervise::SuperviseArgs { real, scenario, timeout, repo_root })
        }
        Commands::Quality { scenario, reviewer, timeout, repo_root } => {
            let repo_root = repo_root
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| PathBuf::from("."));
            quality::run(quality::QualityArgs { scenario, reviewer, timeout, repo_root })
        }
    }
}

fn claude_version() -> String {
    std::process::Command::new("claude")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}
