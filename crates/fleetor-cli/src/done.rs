//! `fleet done` — the receipt (WP-06).
//!
//! A worker finishing a task block runs `fleet done <task-id> "<check>"`. The
//! check runs **here**, in this process, in the worker's own worktree; the hub
//! never executes anything and never sees the command. What crosses the socket is
//! an ordinary [`Op::Send`] to `orch` carrying the receipt as its body — no new
//! op, no new event kind, no schema. The message path's diff for this package is
//! empty by construction, not by care.
//!
//! **The CLI's exit code still means delivery, and only delivery.** The check's
//! own exit code travels *in the body*, where a reader can see it. Conflating the
//! two would break the one promise both briefs make verbatim — a failing check
//! would exit non-zero, the model would read "not delivered", and it would resend
//! a receipt that already arrived (D-034's shape, in a new place).
//!
//! **No deadline on the check.** Same argument as the delivery path's: a ceiling
//! could only turn a slow check into a reported failure for a command that then
//! finishes. The pane's own Bash tool already has a timeout, and it is the caller's
//! to set.
//!
//! Nothing here judges. A non-zero check is reported as faithfully as a zero one,
//! because the receipt is evidence and the judgement is the reviewer's (Tier 1.8).

use anyhow::{bail, Result};
use fleetor_core::pane::PaneId;
use fleetor_core::wire::Op;
use std::path::Path;
use std::process::Command;

/// How much of the check's output travels with the receipt.
///
/// About a test summary and its first failure: enough to act on, not enough for
/// one worker's `cargo test` to eat a pane's context window. The reader who needs
/// more can run the command themselves — the receipt says exactly which one.
pub const TAIL_CAP: usize = 2048;

/// What a check did. Facts only — there is no `passed` field on purpose, because
/// a bool would be this file forming an opinion about somebody's criteria.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// The command as the worker typed it, so the receipt is reproducible.
    pub command: String,
    /// `exit 0`, `exit 101`, `killed by a signal` — rendered rather than kept as
    /// an `i32`, because a command a signal killed has no exit code at all and
    /// `exit -1` would be an invention.
    pub status: String,
    /// stdout and stderr as the terminal would have shown them, already capped.
    pub output: String,
    /// Bytes dropped off the front of `output` by the cap. Named on the receipt
    /// rather than silently truncated — a tail that pretends to be the whole
    /// output is the same lie as `accepted` rendering as "delivered" (L3).
    pub dropped: usize,
}

/// Where the check ran, as git sees it. Not an `Option<(String, String)>` because
/// the two halves fail independently: a fresh repo has a branch and no commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub branch: Option<String>,
    pub commit: Option<String>,
    /// Whether the worktree has uncommitted changes.
    ///
    /// **The receipt has to say so.** Review reads the *commit* the receipt names,
    /// from the reviewer's own worktree — so a receipt claiming a hash while the
    /// work is still sitting in the worker's index points the reviewer at code
    /// that does not contain what was checked. That is a review of the wrong
    /// thing, arrived at honestly, which is the worst kind.
    pub dirty: bool,
}

/// Build the op `fleet done` sends: run the check, then compose the receipt into
/// an ordinary message to `orch`.
///
/// Takes `me` rather than reading the environment so the whole verb is testable
/// without a live pane.
pub fn op(me: PaneId, task: &str, check_command: &str) -> Result<Op> {
    let task = task.trim();
    if task.is_empty() {
        bail!("a receipt needs the block it is about — `fleet task list` shows the ids");
    }
    // `orch` is the one pane with nowhere to send a receipt, and the hub refuses a
    // self-send. Say so here, before running anything, rather than letting the
    // check burn a minute and then fail at the socket.
    if me == PaneId::Orch {
        bail!(
            "`fleet done` sends its receipt to `orch`, and you are `orch` — there is nobody \
             to send it to. Run the check yourself, and put the result on the block with \
             `fleet task update`"
        );
    }

    let check = run(check_command)?;
    Ok(Op::Send { to: PaneId::Orch, text: receipt(task, &here(Path::new(".")), &check) })
}

/// Run the check where this process is, and report what happened.
///
/// Through `sh -c` because the criteria a block carries are shell text —
/// `cargo test -p parser && npx tsc --noEmit` is one criterion, not two words.
/// stdout and stderr are merged **by the shell**, so the tail interleaves in the
/// order the worker would have seen; capturing them separately would put a panic
/// message after the test summary that caused it.
///
/// The brace-and-newline wrapper is what makes that merge safe for arbitrary
/// text: `{ … } 2>&1` applies the redirect to the whole command however it is
/// built up, and the newlines keep a trailing `#` comment from swallowing the
/// closing brace.
fn run(command: &str) -> Result<Check> {
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!("{{\n{command}\n}} 2>&1"))
        // The child is writing to a pipe, so well-behaved tools already drop
        // colour; this is for the ones that do not. Escape sequences would spend
        // the cap on bytes nobody can read.
        .env("NO_COLOR", "1")
        .output()
        .map_err(|e| {
            anyhow::anyhow!(
                "could not run the check ({e}) — nothing was checked and no receipt was sent"
            )
        })?;

    let text = readable(&String::from_utf8_lossy(&output.stdout));
    let (kept, dropped) = tail_of(&text);
    Ok(Check {
        command: command.trim().to_string(),
        status: match output.status.code() {
            Some(code) => format!("exit {code}"),
            None => "killed by a signal".to_string(),
        },
        output: kept,
        dropped,
    })
}

/// The branch, commit and cleanliness of the checkout this process is in.
///
/// Best-effort on purpose: the target may not be a git repo at all (the shared
/// checkout fallback in `src-tauri/src/fleet.rs`), and a receipt that refused to
/// exist because git was silent would take the evidence away exactly when the
/// arrangement is already degraded.
fn here(dir: &Path) -> Place {
    Place {
        branch: git(dir, &["rev-parse", "--abbrev-ref", "HEAD"]),
        commit: git(dir, &["rev-parse", "--short", "HEAD"]),
        dirty: git(dir, &["status", "--porcelain"]).is_some_and(|s| !s.is_empty()),
    }
}

/// One git fact, or `None` if git could not answer. Empty output is `None` too —
/// a blank branch name on a receipt reads as a branch called nothing.
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(dir).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// The receipt itself. Pure, so the format is pinned by tests rather than by a
/// live worker having run something.
///
/// ```text
/// [receipt] task-1-0 · fleet/worker-2 @ a1b2c3d · exit 0
/// $ cargo test -p parser
/// ```
///
/// One line answers *what, where, did it pass*; the command is quoted so the
/// reader can run it themselves; the output follows in a fenced block.
pub fn receipt(task: &str, place: &Place, check: &Check) -> String {
    let mut lines = vec![
        format!("[receipt] {task} · {} · {}", describe(place), check.status),
        format!("$ {}", check.command),
    ];

    if check.output.is_empty() {
        lines.push("(the check printed nothing)".to_string());
        return lines.join("\n");
    }

    let fence = fence_for(&check.output);
    lines.push(fence.clone());
    if check.dropped > 0 {
        lines.push(format!(
            "… {} earlier bytes dropped — this is the last {} of the output",
            check.dropped,
            check.output.len()
        ));
    }
    lines.push(check.output.clone());
    lines.push(fence);
    lines.join("\n")
}

/// `fleet/worker-2 @ a1b2c3d`, and what to say when git cannot supply one half or
/// the other. Never invents a hash: a receipt that named a commit which does not
/// exist would send its reviewer to look at nothing.
fn describe(place: &Place) -> String {
    let dirty = if place.dirty { " + uncommitted changes" } else { "" };
    match (&place.branch, &place.commit) {
        (Some(branch), Some(commit)) => format!("{branch} @ {commit}{dirty}"),
        (Some(branch), None) => format!("{branch} @ no commit yet{dirty}"),
        (None, Some(commit)) => format!("detached @ {commit}{dirty}"),
        (None, None) => "not a git checkout — nothing here can be reviewed by branch".to_string(),
    }
}

/// Keep the **last** `TAIL_CAP` bytes, and say how many went. The tail rather
/// than the head because that is where a test summary and the first failure are;
/// a head-capped receipt of a long build is all compiler noise and no verdict.
fn tail_of(text: &str) -> (String, usize) {
    if text.len() <= TAIL_CAP {
        return (text.to_string(), 0);
    }
    let want = text.len() - TAIL_CAP;
    // Slice on a char boundary, or a multi-byte character straddling the cap
    // would panic on a receipt whose output happened to contain one.
    let start = text
        .char_indices()
        .map(|(i, _)| i)
        .find(|i| *i >= want)
        .unwrap_or(text.len());
    (text[start..].to_string(), start)
}

/// A fence longer than any run of backticks inside it, so output that itself
/// contains a code block cannot end the block that carries it. The same class of
/// bug as `message::sanitize`'s bracketed-paste escape, one layer up and with
/// nothing at stake but legibility.
fn fence_for(text: &str) -> String {
    let longest = text
        .chars()
        .fold((0usize, 0usize), |(max, run), c| {
            let run = if c == '`' { run + 1 } else { 0 };
            (max.max(run), run)
        })
        .0;
    "`".repeat(longest.max(2) + 1)
}

/// Drop what a terminal would act on rather than print, keeping newlines and tabs.
///
/// This is a **legibility and budget** filter, not a security one: the pty
/// boundary is `message::sanitize` and this package does not touch it. The reason
/// it is here anyway is the cap — escape bytes nobody can read would be spent out
/// of the 2 KB a reviewer gets, and the body would arrive in the Messages view as
/// litter. Eight lines rather than a shared helper, for the reason `task.rs` gives
/// for its own copy: coupling the two boundaries is how one of them drifts.
fn readable(raw: &str) -> String {
    raw.replace("\r\n", "\n")
        .chars()
        .filter_map(|c| match c {
            '\n' | '\t' => Some(c),
            '\r' => Some('\n'),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place() -> Place {
        Place { branch: Some("fleet/worker-2".into()), commit: Some("a1b2c3d".into()), dirty: false }
    }

    fn check(status: &str, output: &str) -> Check {
        Check {
            command: "cargo test -p parser".into(),
            status: status.into(),
            output: output.into(),
            dropped: 0,
        }
    }

    /// The first line is the whole point of the format: what block, which branch
    /// and commit, and whether the check passed — one glance, before any output.
    #[test]
    fn the_first_line_says_what_where_and_whether_it_passed() {
        let body = receipt("task-1-0", &place(), &check("exit 0", "test result: ok"));
        let first = body.lines().next().unwrap();
        assert_eq!(first, "[receipt] task-1-0 · fleet/worker-2 @ a1b2c3d · exit 0");
        assert!(body.contains("$ cargo test -p parser"), "the command is reproducible: {body}");
        assert!(body.contains("test result: ok"), "{body}");
    }

    /// A failing check is evidence, not something to soften. The exit code goes on
    /// the receipt exactly as the command produced it — this file forms no opinion
    /// about whether a block is done.
    #[test]
    fn a_failing_check_is_reported_as_faithfully_as_a_passing_one() {
        let body = receipt("task-1-0", &place(), &check("exit 101", "1 failed"));
        assert!(body.starts_with("[receipt] task-1-0 · fleet/worker-2 @ a1b2c3d · exit 101"));
        assert!(body.contains("1 failed"));
        assert!(!body.to_lowercase().contains("fail to"), "no verdict, only the facts: {body}");
    }

    /// A command a signal killed has no exit code. Rendering one would be an
    /// invention, and `exit -1` is the kind of number a reader would act on.
    #[test]
    fn a_signal_is_named_rather_than_turned_into_a_fake_exit_code() {
        let body = receipt("task-1-0", &place(), &check("killed by a signal", "…"));
        assert!(body.contains("killed by a signal"), "{body}");
        assert!(!body.contains("exit -1"), "{body}");
    }

    /// Uncommitted work is the receipt's honesty hole: review reads the commit the
    /// receipt names, so a hash that does not contain what was checked points the
    /// reviewer at the wrong code.
    #[test]
    fn uncommitted_work_is_named_on_the_receipt() {
        let dirty = Place { dirty: true, ..place() };
        let body = receipt("task-1-0", &dirty, &check("exit 0", "ok"));
        assert!(body.contains("+ uncommitted changes"), "{body}");
    }

    /// No git, no invented hash. The shared-checkout fallback and a non-repo target
    /// both land here, and the receipt has to say the review-by-branch move is off
    /// rather than name a commit nobody can look at.
    #[test]
    fn a_place_that_is_not_a_git_checkout_says_so_instead_of_inventing_a_hash() {
        let nowhere = Place { branch: None, commit: None, dirty: false };
        let body = receipt("task-1-0", &nowhere, &check("exit 0", "ok"));
        assert!(body.contains("not a git checkout"), "{body}");
        assert!(!body.contains(" @ "), "nothing that reads as a commit: {body}");

        let fresh = Place { branch: Some("fleet/worker-1".into()), commit: None, dirty: true };
        let body = receipt("task-1-0", &fresh, &check("exit 0", "ok"));
        assert!(body.contains("fleet/worker-1 @ no commit yet"), "{body}");
    }

    /// The cap keeps the *tail*, because that is where the summary is — and it
    /// says what it dropped, because a tail pretending to be the whole output is
    /// the same class of lie as `accepted` rendering as "delivered".
    #[test]
    fn the_tail_is_capped_and_the_receipt_says_what_it_dropped() {
        let long = format!("{}\ntest result: ok. 12 passed", "noise\n".repeat(2000));
        let (kept, dropped) = tail_of(&long);
        assert!(kept.len() <= TAIL_CAP, "kept {} bytes", kept.len());
        assert!(dropped > 0);
        assert!(kept.ends_with("test result: ok. 12 passed"), "the summary survives");

        let body = receipt("task-1-0", &place(), &Check { output: kept, dropped, ..check("exit 0", "") });
        assert!(body.contains(&format!("{dropped} earlier bytes dropped")), "{body}");
    }

    /// Short output is not touched at all — no marker, no truncation note.
    #[test]
    fn output_under_the_cap_arrives_whole_and_unannotated() {
        let (kept, dropped) = tail_of("test result: ok");
        assert_eq!(kept, "test result: ok");
        assert_eq!(dropped, 0);
        let body = receipt("task-1-0", &place(), &check("exit 0", "test result: ok"));
        assert!(!body.contains("dropped"), "{body}");
    }

    /// Capping must not panic on output that happens to contain a multi-byte
    /// character across the boundary. Test output contains `→`, `≈` and worse.
    #[test]
    fn the_cap_lands_on_a_character_boundary() {
        let text = "→".repeat(4000);
        let (kept, dropped) = tail_of(&text);
        assert!(kept.len() <= TAIL_CAP);
        assert!(dropped > 0);
        assert!(kept.starts_with('→'), "the slice began mid-character");
    }

    /// Output containing a fenced block of its own must not end the fence carrying
    /// it — a receipt whose tail escaped its own block reads as prose.
    #[test]
    fn the_fence_is_longer_than_any_backtick_run_in_the_output() {
        assert_eq!(fence_for("plain output"), "```");
        assert_eq!(fence_for("see ```rust\nfn x(){}\n```"), "````");
        let body = receipt("task-1-0", &place(), &check("exit 0", "```\nnested\n```"));
        assert!(body.contains("````"), "{body}");
    }

    /// Escapes are dropped before the cap, so the 2 KB a reviewer gets is 2 KB of
    /// readable output rather than colour codes.
    #[test]
    fn escapes_are_dropped_before_anything_is_measured() {
        let cleaned = readable("\x1b[32mok\x1b[0m\ttab\r\nline");
        assert!(!cleaned.contains('\x1b'), "{cleaned:?}");
        assert!(cleaned.contains('\t'), "a tab is layout: {cleaned:?}");
        assert!(cleaned.contains("\nline"), "CRLF collapses: {cleaned:?}");
    }

    /// An empty check output says so. A bare empty fence reads as a broken receipt.
    #[test]
    fn a_silent_check_says_it_printed_nothing() {
        let body = receipt("task-1-0", &place(), &check("exit 0", ""));
        assert!(body.contains("(the check printed nothing)"), "{body}");
        assert!(!body.contains("```"), "no empty fence: {body}");
    }

    // --- the verb, end to end ---------------------------------------------------

    /// The delivery contract's shape, pinned: the receipt is an **ordinary send to
    /// `orch`**. No new op, no new event kind — and therefore an empty diff in the
    /// message path.
    #[test]
    fn a_receipt_is_an_ordinary_send_to_orch() {
        let op = op(PaneId::Worker(2), "task-1-0", "true").expect("a worker may report");
        let Op::Send { to, text } = op else { panic!("a receipt must be a plain send: {op:?}") };
        assert_eq!(to, PaneId::Orch);
        assert!(text.starts_with("[receipt] task-1-0 · "), "{text}");
    }

    /// The hardest-pinned assertion in the repo, in its new place: a **failing
    /// check still produces a receipt**, and the failure is in the body. If this
    /// ever returned `Err`, the CLI would exit non-zero, both briefs promise that
    /// means *not delivered*, and the worker would resend a receipt that arrived.
    #[test]
    fn a_failing_check_still_produces_a_receipt_rather_than_an_error() {
        let op = op(PaneId::Worker(3), "task-1-0", "echo boom; exit 7")
            .expect("a failing check is a receipt, never a CLI failure");
        let Op::Send { text, .. } = op else { panic!("expected a send") };
        assert!(text.contains("· exit 7"), "the check's code is in the body: {text}");
        assert!(text.contains("boom"), "and so is what it said: {text}");
    }

    /// The check runs in **this** process's working directory — the worker's own
    /// worktree — and nowhere else. The hub never sees the command.
    #[test]
    fn the_check_runs_where_the_cli_runs() {
        let cwd = std::env::current_dir().unwrap();
        let op = op(PaneId::Worker(1), "task-1-0", "pwd").expect("a receipt");
        let Op::Send { text, .. } = op else { panic!("expected a send") };
        assert!(text.contains(&cwd.to_string_lossy().to_string()), "{text}");
    }

    /// stdout and stderr interleave in the order the worker would have seen them.
    #[test]
    fn stderr_travels_with_stdout_in_the_order_it_happened() {
        let op = op(PaneId::Worker(1), "task-1-0", "echo first; echo second >&2; echo third")
            .expect("a receipt");
        let Op::Send { text, .. } = op else { panic!("expected a send") };
        let (first, second, third) = (
            text.find("first").expect(&text),
            text.find("second").expect(&text),
            text.find("third").expect(&text),
        );
        assert!(first < second && second < third, "streams were reordered: {text}");
    }

    /// The wrapper that makes the merge safe must not change what the command
    /// means — a trailing comment, a chain, a pipe are all one criterion.
    #[test]
    fn the_shell_wrapper_does_not_change_what_the_command_means() {
        for (command, expected) in [
            ("echo a && echo b", "exit 0"),
            ("false || echo recovered", "exit 0"),
            ("echo hi | tr a-z A-Z", "exit 0"),
            ("echo done # a trailing comment", "exit 0"),
            ("exit 3", "exit 3"),
        ] {
            let op = op(PaneId::Worker(1), "task-1-0", command).expect(command);
            let Op::Send { text, .. } = op else { panic!("expected a send") };
            assert!(text.contains(&format!("· {expected}")), "{command}: {text}");
        }
    }

    /// `orch` has nowhere to send a receipt, and the hub refuses a self-send. The
    /// refusal happens before the check runs, so it does not cost a test run.
    #[test]
    fn orch_is_told_why_it_cannot_report_before_anything_is_run() {
        let why = op(PaneId::Orch, "task-1-0", "exit 1").expect_err("must be refused").to_string();
        assert!(why.contains("you are `orch`"), "{why}");
        assert!(why.contains("fleet task update"), "the refusal says what to do instead: {why}");
    }

    /// A receipt with no block is a receipt about nothing.
    #[test]
    fn a_receipt_needs_a_block_to_be_about() {
        let why = op(PaneId::Worker(1), "  ", "true").expect_err("must be refused").to_string();
        assert!(why.contains("fleet task list"), "{why}");
    }
}
