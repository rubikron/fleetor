//! Per-ticket records, fleet-wide aggregation, and markdown rendering.
//!
//! Cost is computed from real DeepSeek token counts — never `total_cost_usd`,
//! which Claude Code fabricates from its Anthropic price table (Phase 0 finding).

use crate::probe::classify::{CallRecord, Outcome};
use serde::Serialize;
use std::collections::BTreeMap;

// DeepSeek V4 Flash headline rates (USD per 1M tokens). Cache-read is estimated
// at 0.1x input; if DeepSeek publishes exact cache pricing, update here. The
// go/no-go turns on fidelity, not on cents, so this only needs to be close.
pub const FLASH_IN_PER_M: f64 = 0.14;
pub const FLASH_OUT_PER_M: f64 = 0.28;
pub const FLASH_CACHE_READ_PER_M: f64 = 0.014;

#[derive(Debug, Clone, Serialize)]
pub struct TicketRun {
    pub id: String,
    pub category: String,
    pub targets: String,
    pub turns: u64,
    /// The model's own claim: result arrived, subtype success, not is_error.
    pub claimed_success: bool,
    /// Independent gate: the ticket's verify command passed against the repo.
    pub verify_passed: bool,
    pub timed_out: bool,
    pub total_calls: usize,
    pub fidelity_failures: usize,
    pub task_errors: usize,
    pub no_result: usize,
    pub tool_calls: BTreeMap<String, usize>,
    pub fidelity_by_kind: BTreeMap<String, usize>,
    /// (tool, kind, excerpt) for the report's failure-shape section.
    pub fidelity_examples: Vec<(String, String, String)>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub est_cost_usd: f64,
    pub wall_ms: u128,
}

impl TicketRun {
    pub fn est_cost(input: u64, output: u64, cache_read: u64, cache_creation: u64) -> f64 {
        let billed_input = input + cache_creation;
        billed_input as f64 * FLASH_IN_PER_M / 1e6
            + output as f64 * FLASH_OUT_PER_M / 1e6
            + cache_read as f64 * FLASH_CACHE_READ_PER_M / 1e6
    }

    pub fn from_calls(
        id: String,
        category: String,
        targets: String,
        turns: u64,
        claimed_success: bool,
        verify_passed: bool,
        timed_out: bool,
        calls: &[CallRecord],
        input_tokens: u64,
        output_tokens: u64,
        cache_read_tokens: u64,
        cache_creation_tokens: u64,
        wall_ms: u128,
    ) -> Self {
        let mut tool_calls = BTreeMap::new();
        let mut fidelity_by_kind = BTreeMap::new();
        let mut fidelity_examples = Vec::new();
        let (mut fidelity_failures, mut task_errors, mut no_result) = (0, 0, 0);

        for c in calls {
            *tool_calls.entry(c.tool.clone()).or_insert(0) += 1;
            match c.outcome {
                Outcome::Fidelity(k) => {
                    fidelity_failures += 1;
                    *fidelity_by_kind.entry(k.label().to_string()).or_insert(0) += 1;
                    fidelity_examples.push((
                        c.tool.clone(),
                        k.label().to_string(),
                        c.error_excerpt.clone().unwrap_or_default(),
                    ));
                }
                Outcome::TaskError => task_errors += 1,
                Outcome::NoResult => no_result += 1,
                Outcome::Ok => {}
            }
        }

        let est_cost_usd = Self::est_cost(
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_creation_tokens,
        );

        TicketRun {
            id,
            category,
            targets,
            turns,
            claimed_success,
            verify_passed,
            timed_out,
            total_calls: calls.len(),
            fidelity_failures,
            task_errors,
            no_result,
            tool_calls,
            fidelity_by_kind,
            fidelity_examples,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_creation_tokens,
            est_cost_usd,
            wall_ms,
        }
    }

    /// The dangerous case: the worker reported success but the AC did not hold.
    pub fn false_success(&self) -> bool {
        self.claimed_success && !self.verify_passed
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Aggregate {
    pub n_tickets: usize,
    pub verify_passed: usize,
    pub claimed_success: usize,
    pub false_successes: usize,
    pub total_calls: usize,
    pub total_fidelity_failures: usize,
    pub total_task_errors: usize,
    pub total_no_result: usize,
    pub fidelity_rate: f64,
    pub per_tool: BTreeMap<String, (usize, usize)>, // (calls, fidelity failures)
    pub fidelity_by_kind: BTreeMap<String, usize>,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_est_cost_usd: f64,
}

pub fn aggregate(runs: &[TicketRun]) -> Aggregate {
    let mut per_tool: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut fidelity_by_kind: BTreeMap<String, usize> = BTreeMap::new();
    let (mut calls, mut fid, mut task, mut nores) = (0, 0, 0, 0);
    let (mut input, mut output) = (0u64, 0u64);
    let mut cost = 0.0;

    for r in runs {
        calls += r.total_calls;
        fid += r.fidelity_failures;
        task += r.task_errors;
        nores += r.no_result;
        input += r.input_tokens;
        output += r.output_tokens;
        cost += r.est_cost_usd;
        for (tool, n) in &r.tool_calls {
            per_tool.entry(tool.clone()).or_insert((0, 0)).0 += n;
        }
        for (tool, _, _) in &r.fidelity_examples {
            per_tool.entry(tool.clone()).or_insert((0, 0)).1 += 1;
        }
        for (kind, n) in &r.fidelity_by_kind {
            *fidelity_by_kind.entry(kind.clone()).or_insert(0) += n;
        }
    }

    Aggregate {
        n_tickets: runs.len(),
        verify_passed: runs.iter().filter(|r| r.verify_passed).count(),
        claimed_success: runs.iter().filter(|r| r.claimed_success).count(),
        false_successes: runs.iter().filter(|r| r.false_success()).count(),
        total_calls: calls,
        total_fidelity_failures: fid,
        total_task_errors: task,
        total_no_result: nores,
        fidelity_rate: if calls > 0 { fid as f64 / calls as f64 } else { 0.0 },
        per_tool,
        fidelity_by_kind,
        total_input_tokens: input,
        total_output_tokens: output,
        total_est_cost_usd: cost,
    }
}

pub fn render_markdown(runs: &[TicketRun], agg: &Aggregate, cc_version: &str) -> String {
    let mut s = String::new();
    s.push_str("# FLEETOR — Phase 0 Probe Report\n\n");
    s.push_str(&format!(
        "Worker model: `deepseek-v4-flash` via `https://api.deepseek.com/anthropic` · \
         Claude Code `{cc_version}` · isolated `CLAUDE_CONFIG_DIR`.\n\n"
    ));

    s.push_str("## Headline\n\n");
    s.push_str(&format!(
        "- Tickets: **{}** · verify-passed: **{}/{}** · worker-claimed success: **{}/{}**\n",
        agg.n_tickets, agg.verify_passed, agg.n_tickets, agg.claimed_success, agg.n_tickets
    ));
    s.push_str(&format!(
        "- **False successes** (claimed done, AC failed): **{}**\n",
        agg.false_successes
    ));
    s.push_str(&format!(
        "- Tool calls: **{}** · fidelity failures: **{}** · **fidelity failure rate: {:.1}%**\n",
        agg.total_calls,
        agg.total_fidelity_failures,
        agg.fidelity_rate * 100.0
    ));
    s.push_str(&format!(
        "- Task-level errors (legit non-zero, not fidelity): {} · orphaned tool calls: {}\n",
        agg.total_task_errors, agg.total_no_result
    ));
    s.push_str(&format!(
        "- Real cost (token-based estimate): **${:.4}** total · ${:.4}/ticket avg · \
         {} in / {} out tokens\n\n",
        agg.total_est_cost_usd,
        agg.total_est_cost_usd / agg.n_tickets.max(1) as f64,
        agg.total_input_tokens,
        agg.total_output_tokens
    ));

    s.push_str("## Per-ticket\n\n");
    s.push_str("| Ticket | Category | Verify | Claimed | Turns | Calls | Fidelity fails | Task errs | Cost |\n");
    s.push_str("|---|---|---|---|---|---|---|---|---|\n");
    for r in runs {
        let verify = if r.timed_out {
            "⏱ timeout"
        } else if r.verify_passed {
            "✅ pass"
        } else {
            "❌ fail"
        };
        let claimed = if r.claimed_success { "yes" } else { "no" };
        let flag = if r.false_success() { " ⚠️" } else { "" };
        s.push_str(&format!(
            "| `{}` | {} | {}{} | {} | {} | {} | {} | {} | ${:.4} |\n",
            r.id, r.category, verify, flag, claimed, r.turns, r.total_calls,
            r.fidelity_failures, r.task_errors, r.est_cost_usd
        ));
    }
    s.push('\n');

    s.push_str("## Fidelity failures by tool\n\n");
    s.push_str("| Tool | Calls | Fidelity failures |\n|---|---|---|\n");
    for (tool, (calls, fails)) in &agg.per_tool {
        s.push_str(&format!("| `{tool}` | {calls} | {fails} |\n"));
    }
    s.push('\n');

    if !agg.fidelity_by_kind.is_empty() {
        s.push_str("## Fidelity failures by shape\n\n");
        for (kind, n) in &agg.fidelity_by_kind {
            s.push_str(&format!("- `{kind}`: {n}\n"));
        }
        s.push('\n');
        s.push_str("### Examples\n\n");
        for r in runs {
            for (tool, kind, excerpt) in &r.fidelity_examples {
                s.push_str(&format!(
                    "- `{}` / `{tool}` / `{kind}` — {}\n",
                    r.id, excerpt
                ));
            }
        }
        s.push('\n');
    }

    if agg.false_successes > 0 {
        s.push_str("## ⚠️ False successes (worker claimed done, AC failed)\n\n");
        for r in runs.iter().filter(|r| r.false_success()) {
            s.push_str(&format!("- `{}` ({})\n", r.id, r.category));
        }
        s.push('\n');
    }

    s.push_str("## Notes\n\n");
    s.push_str(
        "- `total_cost_usd` from Claude Code is Anthropic-priced and ignored; cost above is \
         token-based at Flash rates ($0.14/$0.28 per M, cache-read at 0.1×).\n",
    );
    s.push_str(
        "- A `tool_result` with `is_error:true` from Bash is a task-level error (e.g. a \
         failing test), not a fidelity failure — classified separately.\n",
    );
    s
}
