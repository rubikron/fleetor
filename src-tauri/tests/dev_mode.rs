//! The two boundaries dev mode is not allowed to cross (WP-16).
//!
//! Both are properties of *which files mention it*, so both are checked by
//! reading the source rather than by running it — the same move
//! `crates/fleetor-core/src/task.rs` makes with its tripwire list, and for the
//! same reason: the thing worth pinning is that a future session cannot add the
//! branch without the test noticing.
//!
//! 1. **Tier 1.4** — nothing between `fleet send` and a pty may delay, refuse,
//!    reorder, drop or alter a message. A mode readable in there is a mode
//!    something can eventually branch on, so the delivery path does not get to
//!    read one at all.
//! 2. **The veil** (WP-12, open question 4) — dev mode is the *operator's*
//!    posture. `orch` does not learn about it from its brief or from `fleet
//!    roster`, so a pane that infers the evaluator from a banner it was told
//!    about is a failure mode that cannot happen.

use std::path::{Path, PathBuf};

/// Every spelling of the mode a search would plausibly find, lowercased. The
/// underscore form is the config key and the module path; the others are how
/// prose and TypeScript would say it.
const SPELLINGS: [&str; 4] = ["dev_mode", "devmode", "dev mode", "dev-mode"];

/// The repo root — this crate's manifest dir is `src-tauri/`.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("src-tauri has a parent").to_path_buf()
}

/// Every mention of the mode in `file`, as `line number: line`.
fn mentions(file: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(file)
        .unwrap_or_else(|e| panic!("{} must exist to be checked: {e}", file.display()));
    text.lines()
        .enumerate()
        .filter(|(_, line)| {
            let lowered = line.to_lowercase();
            SPELLINGS.iter().any(|needle| lowered.contains(needle))
        })
        .map(|(i, line)| format!("  {}:{} {}", file.display(), i + 1, line.trim()))
        .collect()
}

fn assert_never_mentions_dev_mode(files: &[&str], why: &str) {
    let root = repo_root();
    let hits: Vec<String> = files.iter().flat_map(|rel| mentions(&root.join(rel))).collect();
    assert!(hits.is_empty(), "{why}\n{}", hits.join("\n"));
}

/// Tier 1.4, as a property of the source rather than a promise about it.
///
/// The list is every file a message actually passes through: the hub that
/// routes it, the bus and store it is recorded in, the wire and message
/// contracts it is spelled in, the delivery loop that types it, the registry
/// that owns the pty, and the CLI that sends it.
#[test]
fn the_delivery_path_cannot_read_dev_mode() {
    assert_never_mentions_dev_mode(
        &[
            "src-tauri/src/deliver.rs",
            "src-tauri/src/pty.rs",
            "crates/fleetor-server/src/hub.rs",
            "crates/fleetor-server/src/bus.rs",
            "crates/fleetor-server/src/lib.rs",
            "crates/fleetor-core/src/message.rs",
            "crates/fleetor-core/src/wire.rs",
            "crates/fleetor-core/src/command.rs",
            "crates/fleetor-cli/src/main.rs",
        ],
        "Tier 1.4: nothing between `fleet send` and a pty may know what mode the app is in. \
         A path that can read a mode is a path that can branch on one, and a delivery that \
         can be branched on is the gate D-034 refused twice. Move the branch to the caller.",
    );
}

/// Every file that becomes words a pane is told. `orch`'s whole world is its
/// brief and what `fleet roster` answers.
const WHAT_A_PANE_IS_TOLD: [&str; 7] = [
    "prompts/orch.md",
    "prompts/worker.md",
    "prompts/delivery-contract.md",
    "prompts/broadcast-rule.md",
    "prompts/vision-tenets.md",
    "crates/fleetor-core/src/brief.rs",
    "crates/fleetor-core/src/pane.rs",
];

/// The veil, half one: the mode itself.
#[test]
fn no_pane_is_briefed_about_dev_mode() {
    assert_never_mentions_dev_mode(
        &WHAT_A_PANE_IS_TOLD,
        "WP-12 open question 4: dev mode is visible in the operator's UI and absent from \
         every word a pane is told. Teaching `orch` the mode exists invites it to infer the \
         evaluator behind it — and the veil is far cheaper to keep than to re-establish.",
    );
}

/// **The veil, half two: the evaluator itself** (WP-13).
///
/// The mode was the indirect leak; this is the direct one. WP-13's `fleet
/// handoff` is the first thing a pane is taught that something *else* will
/// eventually react to, and the whole design of that reaction depends on `orch`
/// not knowing it is coming: an orchestrator told its handoff will be read by an
/// evaluator writes the handoff for the evaluator, and the run stops being
/// evidence of how the fleet actually works.
///
/// The verb is taught as what it honestly is from the fleet's side — a report to
/// the operator that the goal is met — so this checks the vocabulary of the
/// thing on the other side of it never appears in any of those files. Note that
/// `review` is deliberately **not** on this list: peer review is a real, taught,
/// worker-facing thing (WP-06), and conflating the two words is how this test
/// would start failing for the wrong reason.
#[test]
fn no_pane_is_briefed_about_the_evaluator() {
    const EVALUATOR_WORDS: [&str; 6] =
        ["evaluat", "the retro", "answer key", "rubric", "grader", "proposal ledger"];
    let root = repo_root();
    let hits: Vec<String> = WHAT_A_PANE_IS_TOLD
        .iter()
        .flat_map(|rel| {
            let file = root.join(rel);
            let text = std::fs::read_to_string(&file)
                .unwrap_or_else(|e| panic!("{} must exist to be checked: {e}", file.display()));
            text.lines()
                .enumerate()
                .filter(|(_, line)| {
                    let lowered = line.to_lowercase();
                    EVALUATOR_WORDS.iter().any(|needle| lowered.contains(needle))
                })
                .map(|(i, line)| format!("  {}:{} {}", file.display(), i + 1, line.trim()))
                .collect::<Vec<String>>()
        })
        .collect();
    assert!(
        hits.is_empty(),
        "WP-12's veil: `orch` does not learn that an evaluator exists until the retro starts, \
         and `fleet handoff` is taught as a report to the operator and nothing more. A pane \
         that knows its handoff will be assessed optimizes for the assessment instead of \
         doing the work.\n{}",
        hits.join("\n"),
    );
}
