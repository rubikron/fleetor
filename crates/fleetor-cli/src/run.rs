//! `fleetor run` — the Phase 4a **multi-worker fleet runner**, headless.
//!
//! Boots the hub on a fleet socket, spawns real Flash worker(s) fully wired
//! (shim MCP + Stop hook), and runs a stand-in lead loop that answers `ask_lead`
//! and sends one piece of mid-turn mail. This is the driver for the 4a live
//! confirmation gate: a live worker calls `ask_lead` through the shim and
//! receives the lead's mail via its Stop hook (phase2-spikes checklist).
//!
//! State lands under `~/.fleetor/_run/`, outside the repo (Tier-1 boundary), so
//! `rm -rf ~/.fleetor/_run` fully undoes a run.

use anyhow::{Context, Result};
use fleetor_cc::spawn::FleetWiring;
use fleetor_cc::WorkerConfig;
use fleetor_core::{Store, Ticket};
use fleetor_db::SqliteStore;
use fleetor_ipc::UnixTransport;
use fleetor_server::{run_fleet, HubConfig, LeadPolicy, WorkerSpec};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct RunArgs {
    /// Per-ticket wall-clock timeout in seconds.
    pub timeout: u64,
    /// Repo root (for `.env` and the fake-claude script); defaults to cwd.
    pub repo_root: PathBuf,
    /// Use real Flash worker(s) — costs tokens. Default: the free fake path.
    pub real: bool,
}

fn base_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME not set")?;
    Ok(PathBuf::from(home).join(".fleetor").join("_run"))
}

/// The `fleetor-shim` binary sits next to this CLI in the same target dir.
fn shim_path() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("locating the fleetor binary")?;
    let shim = exe.with_file_name("fleetor-shim");
    anyhow::ensure!(shim.exists(), "fleetor-shim not found next to {exe:?} — run `cargo build` first");
    Ok(shim)
}

/// A demo ticket engineered to force an `ask_lead` before any file work, so the
/// live confirmation exercises the blocking round-trip and the Stop-hook mail.
fn demo_ticket() -> Ticket {
    Ticket::new(
        "T-run",
        "add a greeting helper (ask first)",
        "Before writing anything, you MUST call the `mcp__fleet__ask_lead` tool to \
         ask the lead whether the file should be named `hello.sh` or `greet.sh`. \
         Wait for the answer, then create that one file in the working directory \
         so `bash <file>` prints `hello fleet`. Keep it to one file.",
    )
    .with_files(vec!["hello.sh".into(), "greet.sh".into()])
}

pub fn run(args: RunArgs) -> Result<()> {
    let base = base_dir()?;
    std::fs::create_dir_all(&base)?;
    let sock = base.join("fleet.sock");
    let store: Arc<dyn Store> = Arc::new(SqliteStore::open(&base.join("state.db"))?);
    let transport = Arc::new(UnixTransport::new(&sock));

    let slot = 1u8;
    let ticket = demo_ticket();
    let raw_log = base.join("logs").join(format!("worker-{slot}")).join(format!("{}.jsonl", ticket.id));

    let spec = if args.real {
        let key = load_api_key(&args.repo_root)?;
        let cwd = base.join("wt").join(format!("worker-{slot}"));
        std::fs::create_dir_all(&cwd)?;
        let config_dir = base.join("cc-config").join(format!("worker-{slot}"));
        let wiring = FleetWiring {
            shim_path: shim_path()?,
            socket_path: sock.clone(),
            slot,
            fleet_dir: base.clone(),
        };
        let config = WorkerConfig::probe(cwd, config_dir, key).with_wiring(wiring);
        eprintln!("run: REAL Flash worker, wired to the hub at {}", sock.display());
        WorkerSpec::real(config, ticket, slot, Some(raw_log.clone()), args.timeout)
    } else {
        anyhow::bail!(
            "`fleetor run` currently drives the live-CC confirmation only; pass --real. \
             The free fake path is covered by the fleet_runner integration test."
        );
    };

    let lead = LeadPolicy::answering("Name it hello.sh — that matches our convention.")
        .with_mail(slot, "FYI from the lead: a teammate finished the shared header you may reuse. Coordination only — keep to your ticket.");

    let outcome = tokio::runtime::Runtime::new()
        .context("starting the tokio runtime")?
        .block_on(run_fleet(store.clone(), transport, vec![spec], HubConfig::default(), lead))?;

    println!("\noutcome: {outcome:?}");
    print_event_tail(store.as_ref())?;
    println!("raw transcript: {}", raw_log.display());
    Ok(())
}

fn print_event_tail(store: &dyn Store) -> Result<()> {
    println!("\nevent log:");
    for (seq, ev) in store.events_since(0)? {
        println!("  [{seq:>3}] {:<14} {}", ev.kind(), serde_json::to_string(&ev)?);
    }
    Ok(())
}

/// Read `DEEPSEEK_API_KEY` from the env or the repo's gitignored `.env`. Never
/// logged. (Mirrors `supervise`/the probe's loader.)
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
