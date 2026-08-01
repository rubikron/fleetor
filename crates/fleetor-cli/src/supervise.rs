//! `fleetor supervise` — drive one ticket through the Phase 1 supervisor.
//!
//! Two backends behind the `AgentProcess` seam:
//!   --fake <scenario>   deterministic, free (default; the exit test's stand-in)
//!   --real              one real Claude Code Flash worker (costs tokens)
//!
//! State lands in a scratch SQLite db under `~/.fleetor/_supervise/`, outside
//! the repo (Tier-1 boundary). After the run it prints the outcome and the
//! event log tail so the lifecycle is visible from the terminal.

use anyhow::{Context, Result};
use fleetor_cc::{FakeClaude, RealClaude, WorkerConfig};
use fleetor_core::{Store, Ticket};
use fleetor_db::SqliteStore;
use fleetor_server::{run_ticket, SuperviseOptions};
use std::path::{Path, PathBuf};

pub struct SuperviseArgs {
    pub real: bool,
    pub scenario: String,
    pub timeout: u64,
    pub repo_root: PathBuf,
}

fn base_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME not set")?;
    Ok(PathBuf::from(home).join(".fleetor").join("_supervise"))
}

/// The demo ticket used for a one-shot supervise run.
fn demo_ticket() -> Ticket {
    Ticket::new(
        "T-demo",
        "add a greeting helper",
        "In the working directory, create `greet.sh` that prints `hello fleet` \
         when run with `bash greet.sh`. Keep it to one file.",
    )
    .with_files(vec!["greet.sh".into()])
}

pub fn run(args: SuperviseArgs) -> Result<()> {
    let base = base_dir()?;
    std::fs::create_dir_all(&base)?;
    let store = SqliteStore::open(&base.join("state.db"))?;

    let ticket = demo_ticket();
    let slot = 1u8;
    let mut opts = SuperviseOptions::new(slot, args.timeout);
    opts.raw_log = Some(base.join("logs").join(format!("worker-{slot}")).join(format!("{}.jsonl", ticket.id)));

    let outcome = if args.real {
        let key = load_api_key(&args.repo_root)?;
        let cwd = base.join("wt").join(format!("worker-{slot}"));
        std::fs::create_dir_all(&cwd)?;
        let config_dir = base.join("cc-config");
        let cfg = WorkerConfig::probe(cwd, config_dir, key);
        eprintln!("supervise: REAL claude worker (Flash), ticket {}", ticket.id);
        run_ticket(&RealClaude { config: cfg }, &ticket, &store, &opts)?
    } else {
        let script = args.repo_root.join("tests/fake-claude/fake-claude.mjs");
        let agent = FakeClaude {
            script,
            cwd: base.join("wt").join(format!("worker-{slot}")),
            scenario: args.scenario.clone(),
        };
        std::fs::create_dir_all(&agent.cwd)?;
        eprintln!("supervise: fake:{} worker, ticket {}", args.scenario, ticket.id);
        run_ticket(&agent, &ticket, &store, &opts)?
    };

    println!("\noutcome: {outcome:?}");
    print_event_tail(&store)?;
    if let Some(log) = &opts.raw_log {
        println!("raw transcript: {}", log.display());
    }
    Ok(())
}

fn print_event_tail(store: &SqliteStore) -> Result<()> {
    println!("\nevent log:");
    for (seq, ev) in store.events_since(0)? {
        println!("  [{seq:>3}] {:<14} {}", ev.kind(), serde_json::to_string(&ev)?);
    }
    Ok(())
}

/// Read `DEEPSEEK_API_KEY` from the env or the repo's gitignored `.env`. Never
/// logged. (Mirrors the probe's loader.)
fn load_api_key(repo_root: &Path) -> Result<String> {
    if let Ok(k) = std::env::var("DEEPSEEK_API_KEY") {
        if !k.is_empty() {
            return Ok(k);
        }
    }
    let env_path = repo_root.join(".env");
    let text = std::fs::read_to_string(&env_path)
        .with_context(|| format!("no DEEPSEEK_API_KEY in env and cannot read {env_path:?}"))?;
    for line in text.lines() {
        if let Some(rest) = line.trim().strip_prefix("DEEPSEEK_API_KEY=") {
            let val = rest.trim().trim_matches('"').trim_matches('\'');
            if !val.is_empty() {
                return Ok(val.to_string());
            }
        }
    }
    anyhow::bail!("DEEPSEEK_API_KEY not found in env or {env_path:?}")
}
