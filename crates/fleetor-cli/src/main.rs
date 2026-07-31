//! `fleetor` — headless driver for FLEETOR Phases 0–3.
//!
//! Phase 0: `fleetor probe` measures DeepSeek V4 Flash's tool-call fidelity
//! through real Claude Code before any supervision infrastructure is built.

mod probe;

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
