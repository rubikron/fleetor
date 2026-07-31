//! Phase 0 probe runner: drive N tickets through one isolated Flash worker each,
//! on a fresh copy of the toy repo, then verify the acceptance criterion
//! independently and aggregate a fidelity report.

pub mod classify;
pub mod report;
pub mod tickets;
pub mod toy;

use anyhow::{Context, Result};
use fleetor_cc::{parse_transcript, Event, WorkerConfig};
use report::{aggregate, render_markdown, Aggregate, TicketRun};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

pub struct ProbeOptions {
    pub repo_root: PathBuf,
    pub only: Vec<String>,
    pub timeout_secs: u64,
    pub out: PathBuf,
    pub cc_version: String,
}

/// Base directory for all probe scratch — deliberately OUTSIDE the repo so the
/// Tier-1 repo-boundary test holds (`rm -rf ~/.fleetor/_probe` leaves no trace).
fn probe_base() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME not set")?;
    Ok(PathBuf::from(home).join(".fleetor").join("_probe"))
}

/// Read `DEEPSEEK_API_KEY` from the process env, or fall back to the repo's
/// gitignored `.env`. Never logged.
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
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("DEEPSEEK_API_KEY=") {
            let val = rest.trim().trim_matches('"').trim_matches('\'');
            if !val.is_empty() {
                return Ok(val.to_string());
            }
        }
    }
    anyhow::bail!("DEEPSEEK_API_KEY not found in env or {env_path:?}")
}

pub fn run(opts: ProbeOptions) -> Result<()> {
    let key = load_api_key(&opts.repo_root)?;
    let base = probe_base()?;
    let config_dir = base.join("cc-config");
    let runs_dir = base.join("runs");
    std::fs::create_dir_all(&config_dir)?;
    std::fs::create_dir_all(&runs_dir)?;

    let all = tickets::all();
    let selected: Vec<_> = all
        .into_iter()
        .filter(|t| opts.only.is_empty() || opts.only.iter().any(|o| t.id.contains(o.as_str())))
        .collect();
    if selected.is_empty() {
        anyhow::bail!("no tickets matched --only {:?}", opts.only);
    }

    eprintln!(
        "probe: {} ticket(s), model=deepseek-v4-flash, timeout={}s, config={}",
        selected.len(),
        opts.timeout_secs,
        config_dir.display()
    );

    let mut runs = Vec::new();
    for (i, t) in selected.iter().enumerate() {
        eprintln!(
            "\n[{}/{}] {}  ({}; targets: {})",
            i + 1,
            selected.len(),
            t.id,
            t.category,
            t.targets
        );
        let run = run_ticket(t, &config_dir, &runs_dir, &key, opts.timeout_secs)?;
        eprintln!(
            "    -> verify={} claimed={} calls={} fidelity_fails={} cost=${:.4} wall={}ms{}",
            if run.timed_out { "TIMEOUT" } else if run.verify_passed { "PASS" } else { "FAIL" },
            run.claimed_success,
            run.total_calls,
            run.fidelity_failures,
            run.est_cost_usd,
            run.wall_ms,
            if run.false_success() { "  <<< FALSE SUCCESS" } else { "" },
        );
        runs.push(run);
    }

    let agg = aggregate(&runs);
    write_artifacts(&opts, &runs, &agg)?;
    print_summary(&agg);
    Ok(())
}

fn run_ticket(
    t: &tickets::Ticket,
    config_dir: &Path,
    runs_dir: &Path,
    key: &str,
    timeout_secs: u64,
) -> Result<TicketRun> {
    let run_dir = runs_dir.join(t.id);
    let repo = run_dir.join("repo");
    toy::materialize(&repo)?;

    let cfg = WorkerConfig::probe(repo.clone(), config_dir.to_path_buf(), key.to_string());
    let mut cmd = cfg.command(t.prompt);

    let started = Instant::now();
    let (stdout, _stderr, timed_out) = run_with_timeout(&mut cmd, timeout_secs)?;
    let wall_ms = started.elapsed().as_millis();

    // Persist the raw transcript next to the repo (out-of-repo scratch).
    std::fs::write(run_dir.join("transcript.ndjson"), &stdout)?;

    let events = parse_transcript(&stdout);
    let calls = classify::classify(&events);
    let (claimed_success, turns, usage) = result_summary(&events);

    // Independent acceptance gate.
    let verify_passed = !timed_out && run_verify(&repo, t.verify);

    Ok(TicketRun::from_calls(
        t.id.to_string(),
        t.category.to_string(),
        t.targets.to_string(),
        turns,
        claimed_success,
        verify_passed,
        timed_out,
        &calls,
        usage.0,
        usage.1,
        usage.2,
        usage.3,
        wall_ms,
    ))
}

/// Pull (claimed_success, num_turns, (input, output, cache_read, cache_creation))
/// from the final `result` event.
fn result_summary(events: &[Event]) -> (bool, u64, (u64, u64, u64, u64)) {
    for ev in events.iter().rev() {
        if let Event::Result(r) = ev {
            let claimed = !r.is_error && r.subtype.as_deref() == Some("success");
            return (
                claimed,
                r.num_turns.unwrap_or(0),
                (
                    r.usage.input_tokens,
                    r.usage.output_tokens,
                    r.usage.cache_read_input_tokens,
                    r.usage.cache_creation_input_tokens,
                ),
            );
        }
    }
    (false, 0, (0, 0, 0, 0))
}

fn run_verify(repo: &Path, verify_cmd: &str) -> bool {
    Command::new("bash")
        .arg("-c")
        .arg(verify_cmd)
        .current_dir(repo)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Spawn a child, stream stdout/stderr on threads, and enforce a wall-clock
/// deadline (a headless worker can wedge on a permission prompt — BUILDING §5).
fn run_with_timeout(cmd: &mut Command, timeout_secs: u64) -> Result<(String, String, bool)> {
    let mut child = cmd.spawn().context("failed to spawn `claude`")?;
    let stdout = child.stdout.take().context("no stdout pipe")?;
    let stderr = child.stderr.take().context("no stderr pipe")?;

    let (otx, orx) = mpsc::channel::<String>();
    let out_handle = thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut collected = String::new();
        for line in reader.lines().map_while(Result::ok) {
            let _ = otx.send(line.clone());
            collected.push_str(&line);
            collected.push('\n');
        }
        collected
    });
    let err_handle = thread::spawn(move || {
        let mut s = String::new();
        let mut r = stderr;
        let _ = r.read_to_string(&mut s);
        s
    });

    // Drain the progress channel just to keep it from filling; we could surface
    // live tool calls here later.
    let drain = thread::spawn(move || while orx.recv().is_ok() {});

    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let mut timed_out = false;
    loop {
        match child.try_wait()? {
            Some(_) => break,
            None => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    timed_out = true;
                    break;
                }
                thread::sleep(Duration::from_millis(150));
            }
        }
    }

    let stdout = out_handle.join().unwrap_or_default();
    let stderr = err_handle.join().unwrap_or_default();
    let _ = drain.join();
    Ok((stdout, stderr, timed_out))
}

fn write_artifacts(opts: &ProbeOptions, runs: &[TicketRun], agg: &Aggregate) -> Result<()> {
    let md = render_markdown(runs, agg, &opts.cc_version);
    if let Some(parent) = opts.out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&opts.out, md)?;
    // Machine-readable sibling.
    let json_path = opts.out.with_extension("json");
    let payload = serde_json::json!({ "aggregate": agg, "runs": runs });
    std::fs::write(&json_path, serde_json::to_string_pretty(&payload)?)?;
    eprintln!("\nreport: {}\n  json: {}", opts.out.display(), json_path.display());
    Ok(())
}

fn print_summary(agg: &Aggregate) {
    eprintln!("========== PHASE 0 SUMMARY ==========");
    eprintln!(
        "tickets={} verify_pass={} claimed={} FALSE_SUCCESS={}",
        agg.n_tickets, agg.verify_passed, agg.claimed_success, agg.false_successes
    );
    eprintln!(
        "tool_calls={} fidelity_fails={} rate={:.1}% task_errors={} orphaned={}",
        agg.total_calls,
        agg.total_fidelity_failures,
        agg.fidelity_rate * 100.0,
        agg.total_task_errors,
        agg.total_no_result
    );
    eprintln!(
        "cost=${:.4} tokens_in={} tokens_out={}",
        agg.total_est_cost_usd, agg.total_input_tokens, agg.total_output_tokens
    );
    eprintln!("=====================================");
}
