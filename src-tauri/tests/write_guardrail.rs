//! The write guardrail (WP-17), end to end and at the one boundary it may not
//! cross.
//!
//! Two kinds of test, and the first kind is the point of the file:
//!
//! 1. **Tier 1.4** — nothing between `fleet send` and a pty may delay, refuse,
//!    reorder, drop or alter a message. A hook that could refuse a *delivery* is
//!    the thing `building.md` §9.3 records as argued and lost twice. This one
//!    governs a pane's own tool calls and nothing else, and that is checked the
//!    way `tests/dev_mode.rs` checks the same invariant for dev mode: by reading
//!    the source, so a future session cannot add the branch without the test
//!    noticing.
//! 2. **The decision itself**, driven through the *installed* artifact — the
//!    `settings.json` command a real [`placement::place`] wrote, executed by a real
//!    shell against a real `PreToolUse` payload. No `claude`, no tokens, and no
//!    second copy of the policy for the test to agree with while the pane runs
//!    something else. `examples/write-guardrail-spike/probe.py` is the arm that
//!    proves Claude Code honours the answer; this is the arm that proves the
//!    answer is right.
//!
//! ## What changed in WP-21, and what deliberately did not
//!
//! **The setup.** This file used to rebuild the bring-up sequence by hand, because
//! it needed the roots and could not call the code that computes them: it invented a
//! `_shell` and a worktree directory, then called `guardrail::roots_for` and
//! `guardrail::install` itself. That copy had already drifted — `roots_for` is
//! unconditional, so the hand-built version could never have produced the
//! evaluator's narrower pair, and the one pane whose roots matter most was the one
//! pane this file could not test. There is no copy now: every `settings.json` below
//! is written by the same `place` call the application makes, against a scratch
//! layout.
//!
//! **Not the seam.** Everything from `Pane::ask` down is untouched. The hook still
//! goes through `sh -c`, still reads a genuine `PreToolUse` payload on stdin, and is
//! still believed only when it answers. This is not a unit test of a Rust function
//! and must not become one.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use fleetor_core::pane::PaneId;
use fleetor_shell::guardrail;
use fleetor_shell::placement::PaneSpec;
use fleetor_shell::placement::harness::claude_code;
/// One whole installation in a scratch directory — the four arguments `place` takes,
/// shared with `tests/placement.rs` rather than retyped here.
mod common;

// --- Tier 1.4 -------------------------------------------------------------------

/// Every spelling of *this* guardrail a search would plausibly find, lowercased:
/// the module path, the file, the hook event, and the name in prose.
///
/// Note what is deliberately **not** here, for `tests/dev_mode.rs`'s reason.
/// Bare `guardrail` is a word the message path already uses about itself
/// (`deliver.rs`'s once-per-session Notice calls itself "the invariant guardrail
/// with teeth"), and `allowlist` is the `fleet cmd` allowlist D-045 checks at
/// accept time. Both are real, both are load-bearing, and matching on them would
/// make this test fail for a reason that has nothing to do with WP-17.
const SPELLINGS: [&str; 4] =
    ["write guardrail", "write_guardrail", "pretooluse", "guardrail::"];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("src-tauri has a parent").to_path_buf()
}

/// Tier 1.4, as a property of the source rather than a promise about it.
///
/// The list is every file a message actually passes through: the hub that routes
/// it, the bus and store it is recorded in, the wire and message contracts it is
/// spelled in, the delivery loop that types it, the registry that owns the pty,
/// and the CLI that sends it. It is the same list `tests/dev_mode.rs` pins, for
/// the same reason.
#[test]
fn the_delivery_path_cannot_read_the_write_guardrail() {
    let root = repo_root();
    let hits: Vec<String> = [
        "src-tauri/src/deliver.rs",
        "src-tauri/src/pty.rs",
        "crates/fleetor-server/src/hub.rs",
        "crates/fleetor-server/src/bus.rs",
        "crates/fleetor-server/src/lib.rs",
        "crates/fleetor-core/src/message.rs",
        "crates/fleetor-core/src/wire.rs",
        "crates/fleetor-core/src/command.rs",
        "crates/fleetor-cli/src/main.rs",
    ]
    .iter()
    .flat_map(|rel| {
        let file = root.join(rel);
        let text = std::fs::read_to_string(&file)
            .unwrap_or_else(|e| panic!("{} must exist to be checked: {e}", file.display()));
        text.lines()
            .enumerate()
            .filter(|(_, line)| {
                let lowered = line.to_lowercase();
                SPELLINGS.iter().any(|needle| lowered.contains(needle))
            })
            .map(|(i, line)| format!("  {}:{} {}", file.display(), i + 1, line.trim()))
            .collect::<Vec<String>>()
    })
    .collect();

    assert!(
        hits.is_empty(),
        "Tier 1.4: the write guardrail governs a pane's own tool calls and must never be \
         readable from the message path. A path that can read a guardrail is a path that can \
         one day consult one, and a delivery that can be refused is the gate D-034 rejected \
         twice. Keep the check at the spawn site.\n{}",
        hits.join("\n"),
    );
}

/// The other half of the same requirement, from the other side: the guardrail
/// must not reach *into* the message path either. It talks to the feed by
/// appending to a file that a task of its own drains — never by sending, never
/// by holding a pty, never by touching the hub.
#[test]
fn the_guardrail_cannot_reach_the_message_path_either() {
    let source = std::fs::read_to_string(repo_root().join("src-tauri/src/guardrail.rs")).unwrap();
    for forbidden in ["deliver::", "PaneRegistry", "AppCommand", "Hub", "Op::", "OpResult"] {
        assert!(
            !source.contains(forbidden),
            "guardrail.rs mentions `{forbidden}` — it must not be able to send, deliver or \
             refuse a message. Its only outputs are a settings file and a journal line.",
        );
    }
}

// --- the decision, through the installed artifact --------------------------------

/// One placed pane, seen the way Claude Code sees it: the shell command it will run
/// for every matched tool call, and the directory it will run in.
struct Pane {
    /// The pane's own working directory, as *placement* resolved it — a worker's
    /// worktree, `orch`'s target repo, the evaluator's laid-out run. Read off the
    /// command rather than chosen by the test, so a placement that put a pane
    /// somewhere else would move these assertions with it.
    cwd: PathBuf,
    command: String,
    journal: PathBuf,
}

impl common::Bench {
    /// Place `spec` for real and hand back the hook the pane's Claude Code will
    /// load.
    ///
    /// **This is the whole of the change WP-21 made to this file.** Every root,
    /// every deny path and every journal below comes from the application's own
    /// bring-up sequence rather than from a copy of it, so a rule that changes there
    /// changes here — which is exactly what did not happen the last time one did.
    fn pane(&self, spec: PaneSpec) -> Pane {
        let pane = spec.pane();
        let placed = self.place(spec);
        Pane {
            cwd: PathBuf::from(placed.command.get_cwd().expect("every pane is placed somewhere")),
            command: common::hook_command(&self.config_dir(pane)),
            journal: self.journal(),
        }
    }
}

impl Pane {
    /// Ask the installed hook about one tool call, exactly as Claude Code does:
    /// the command through a shell, the event as JSON on stdin.
    fn ask(&self, tool: &str, input: serde_json::Value) -> Option<String> {
        let payload = serde_json::json!({
            "hook_event_name": "PreToolUse",
            "cwd": self.cwd.display().to_string(),
            "tool_name": tool,
            "tool_input": input,
        });
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(&self.command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the hook command must be runnable");
        child.stdin.take().unwrap().write_all(payload.to_string().as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "the hook must exit cleanly: {out:?}");
        let answer: serde_json::Value =
            serde_json::from_slice(&out.stdout).expect("the hook must answer JSON");
        answer
            .get("hookSpecificOutput")
            .filter(|h| h["permissionDecision"] == serde_json::json!("deny"))
            .map(|h| h["permissionDecisionReason"].as_str().unwrap_or_default().to_string())
    }

    fn bash(&self, command: &str) -> Option<String> {
        self.ask("Bash", serde_json::json!({ "command": command }))
    }

    /// `touch <dir>/<name>` — the shortest write that names its destination, which
    /// is the only kind this guardrail can see (see the last test in this file).
    fn touch(&self, dir: &Path, name: &str) -> Option<String> {
        self.bash(&format!("touch {}/{name}", dir.display()))
    }
}

/// **The allowlist holds.** Everything a worker legitimately does inside its own
/// worktree — and everything the fleet's own flows do around it — is allowed.
#[test]
fn the_allowlist_holds_for_the_work_a_worker_actually_does() {
    let bench = common::Bench::new("holds");
    let pane = bench.pane(PaneSpec::worker(1, claude_code()));
    let worktree = &pane.cwd;

    for allowed in [
        "cargo build --workspace",
        "cargo test --workspace 2>&1 | tail -20",
        "npx tsc --noEmit && npx vite build",
        "git add parser.rs",
        "git commit -m 'worker-1: a parser'",
        "git diff HEAD...fleet/worker-1",
        "fleet done T-4 cargo test --workspace",
        "touch newfile.rs",
        "echo hi > out.txt",
        "cat /etc/hosts",
        "grep -rn TODO /Users/somebody/else",
    ] {
        assert_eq!(pane.bash(allowed), None, "a worker must be able to run: {allowed}");
    }

    // Reads are open, deliberately and structurally: the hook is not even
    // registered for the tools that do them.
    assert_eq!(pane.ask("Read", serde_json::json!({ "file_path": "/etc/hosts" })), None);
    // And the two roots themselves: its own worktree, and the fleet's state root —
    // both the real ones placement chose, not directories this test invented.
    assert_eq!(pane.touch(worktree, "x"), None);
    assert_eq!(pane.touch(&bench.layout.shell(), "x"), None);
}

/// **A write outside it is refused with a usable reason.** The refusal is
/// written for the model that has to recover from it — the same principle as
/// `prompts/delivery-contract.md`, where a model reads its own exit code and
/// adapts. A denial that did not name the path and the rule would leave a pane
/// unable to tell a guardrail from a broken path.
#[test]
fn a_write_outside_the_roots_is_refused_with_a_reason_a_pane_can_act_on() {
    let bench = common::Bench::new("refusal");
    let pane = bench.pane(PaneSpec::worker(1, claude_code()));

    let why = pane
        .bash("echo notes > /Users/somebody/notes.txt")
        .expect("a write outside every root must be refused");

    assert!(why.contains("/Users/somebody/notes.txt"), "it names the path: {why}");
    assert!(why.contains("worker-1"), "and whose workspace it is outside of: {why}");
    assert!(why.contains(&pane.cwd.display().to_string()), "and where it may write: {why}");
    assert!(why.contains("Reading is not restricted"), "and what the rule actually is: {why}");
    assert!(why.contains("fleet send operator"), "and what to do if it truly belongs there: {why}");
    assert!(
        why.contains("Retrying the same path will be refused again"),
        "and that a retry is not the move — this is the difference between self-correcting \
         and looping: {why}",
    );

    // Structured writes are the exact half: the destination is a field, so the
    // refusal is enforcement rather than a scan of a command string. The relative
    // one climbs far enough to land outside every root wherever the worktree is —
    // a resolved path above `/` clamps at `/`, which is outside them too.
    let escaped = format!("{}escaped.rs", "../".repeat(12));
    for (tool, input) in [
        ("Write", serde_json::json!({ "file_path": "/Users/somebody/CLAUDE.md", "content": "x" })),
        ("Edit", serde_json::json!({ "file_path": escaped })),
        ("NotebookEdit", serde_json::json!({ "notebook_path": "/Users/somebody/x.ipynb" })),
    ] {
        assert!(pane.ask(tool, input).is_some(), "{tool} must be refused");
    }

    // And the refusal reached the journal, which is what puts it on the feed — the
    // journal placement pointed the hook at, inside the layout it was handed.
    let journal = std::fs::read_to_string(&pane.journal).unwrap();
    assert!(journal.contains("worker-1"), "{journal}");
    assert!(journal.contains("notes.txt"), "{journal}");
}

/// **`orch` and workers get correct per-pane roots.** `orch` works in the target
/// repo and a worker does not; a worker works in its own worktree and `orch` has
/// no business in it. Both share `_shell` and nothing else.
///
/// Both are placed against **one** layout and one target, which is what makes the
/// two directories genuinely the ones the application would use: the worktree is a
/// real `git worktree add` under the layout, not a path this test picked and hoped
/// production agreed with.
#[test]
fn orch_gets_the_target_repo_and_a_worker_gets_only_its_own_worktree() {
    let bench = common::Bench::new("per-pane");
    let orch = bench.pane(PaneSpec::orch(claude_code()));
    let worker = bench.pane(PaneSpec::worker(2, claude_code()));
    assert_eq!(orch.cwd, bench.target, "orch works in the target itself");
    assert_ne!(worker.cwd, bench.target, "and a worker got a checkout of its own");

    assert_eq!(orch.touch(&bench.target, "src-main.rs"), None, "orch works in the target repo");
    assert!(
        worker.touch(&bench.target, "src-main.rs").is_some(),
        "a worker may not write into the target repo — that is what its worktree is for",
    );

    assert_eq!(worker.touch(&worker.cwd, "parser.rs"), None, "a worker works in its own worktree");
    assert_eq!(
        orch.touch(&worker.cwd, "parser.rs"),
        None,
        "and `_shell` is on both lists, so orch reaching a worktree is allowed by that root",
    );

    let outside = "touch /Users/somebody/scratch";
    assert!(orch.bash(outside).is_some(), "orch is fenced too, not merely workers");
    assert!(worker.bash(outside).is_some());
}

/// The one directory inside the roots that is still off limits. `_shell` has to
/// be writable — the worktrees are in it — and the pane config dirs inside it
/// hold this guardrail's own rules. A pane that could edit its own
/// `settings.json` could switch the guardrail off between two tool calls.
#[test]
fn no_pane_may_write_its_own_policy_even_though_it_is_inside_a_root() {
    let bench = common::Bench::new("policy");
    let pane = bench.pane(PaneSpec::worker(1, claude_code()));
    // The file placement actually wrote, not a path shaped like one.
    let settings = bench.config_dir(PaneId::Worker(1)).join("settings.json");
    assert!(settings.is_file(), "{} is the guardrail this pane is running", settings.display());
    assert!(
        settings.starts_with(guardrail::policy_dir(&bench.layout.shell())),
        "and it is inside the deny list's own directory, which is what makes this a rule \
         rather than a coincidence: {}",
        settings.display(),
    );

    let why = pane
        .bash(&format!("rm {}", settings.display()))
        .expect("a pane must not be able to delete its own guardrail");
    assert!(why.contains("fleet policy"), "the reason says what kind of rule this is: {why}");

    assert!(pane
        .ask("Edit", serde_json::json!({ "file_path": settings.display().to_string() }))
        .is_some());
    // Reading it is fine. The pane may know exactly what it is not allowed to do.
    assert_eq!(
        pane.ask("Read", serde_json::json!({ "file_path": settings.display().to_string() })),
        None,
    );
}

/// **The operator's extension mechanism works** — `[fence] allow` in
/// `prompts/launch.conf`, through `LaunchConfig::fence_allow`, to a root the
/// pane may really write in.
#[test]
fn a_root_the_operator_added_is_a_root_the_pane_can_write_in() {
    let mut bench = common::Bench::new("extended");
    let plain = bench.pane(PaneSpec::worker(1, claude_code()));
    let scratch = bench.root.join("operator-scratch");
    let write = format!("echo hi > {}/notes.txt", scratch.display());
    assert!(plain.bash(&write).is_some(), "not a root until the operator says so");

    // The operator edits `prompts/launch.conf` and the fleet is relaunched: the same
    // placement, with `[fence] allow` set.
    assert_eq!(bench.operator_allows("operator-scratch"), scratch);
    let extended = bench.pane(PaneSpec::worker(1, claude_code()));
    assert_eq!(extended.bash(&write), None, "and a root once they do");
    assert!(
        extended.bash("echo hi > /Users/somebody/else").is_some(),
        "one added root is one added root, not an open door",
    );
}

/// The measured half of `docs/notes/write-guardrail-notes.md`, pinned: the two
/// writes a pane cannot work without are writes it does not *name*, and the
/// guardrail lets them through because of that rather than by accident. A
/// future tightening that broke either would wedge every worker on `fleet
/// done`'s first step, and this is the test that would say so.
#[test]
fn the_two_unnamed_writes_a_pane_cannot_work_without_are_allowed() {
    let bench = common::Bench::new("unnamed");
    let pane = bench.pane(PaneSpec::worker(1, claude_code()));

    // Measured: writes 54 files into the worktree's own target/ and one into the
    // operator's ~/.cargo/registry, and names neither.
    assert_eq!(pane.bash("cargo build"), None);
    // Measured: writes 8 files into the target repo's .git — outside a worker's
    // roots — and names none of them. `fleet done` runs this first.
    assert_eq!(pane.bash("git commit -m 'worker-1: a parser'"), None);
    // The same operation aimed somewhere it *does* name is refused, which is the
    // whole distinction this package rests on.
    assert!(pane.bash("git -C /Users/somebody/repo commit -am wip").is_some());
}

/// **The evaluator is refused writes every other pane is allowed** — the narrowest
/// roots in the fleet, run through the hook rather than read off a file.
///
/// This is the test the hand-built setup could not have written. It called
/// `guardrail::roots_for` unconditionally, so an evaluator built by it would have
/// carried a worker's roots and passed every assertion below for the wrong reason.
/// Driving the real placement is what makes the veil's filesystem half checkable at
/// all.
///
/// The three refusals are the three things a grader must not be able to touch:
///
///  1. **`_shell`** — where the live event log it is reading lives. A judge that can
///     write its own evidence is not one.
///  2. **The target repo** — the work it is judging.
///  3. **The operator's `[fence] allow` extra** — widening what the fleet may reach
///     must not widen what its judge may reach.
///
/// And it may write in the run it was handed, or it cannot do its job at all.
#[test]
#[cfg(feature = "devmode")]
fn the_evaluator_may_write_in_the_run_it_was_given_and_in_nothing_else() {
    let mut bench = common::Bench::new("evaluator");
    bench.dev_mode(true);
    let extra = bench.operator_allows("operator-scratch");

    // `orch` is the comparison, and it is the exact one: its three roots are these
    // three directories, so every assertion below is the *asymmetry* between the two
    // panes rather than a fence in general.
    let orch = bench.pane(PaneSpec::orch(claude_code()));
    let evaluator = bench.pane(PaneSpec::Evaluator);
    assert_eq!(evaluator.cwd, bench.retro_dir(), "it works in the snapshot of the run");

    // What it must be able to do: write inside the run it was given.
    assert_eq!(evaluator.touch(&evaluator.cwd, "verdict.md"), None, "the sealed verdict");

    for (why, dir) in [
        ("the state root, which holds the log it is reading", bench.layout.shell()),
        ("the target repo, which holds the work it is judging", bench.target.clone()),
        ("the operator's own extra root", extra),
    ] {
        assert_eq!(orch.touch(&dir, "x"), None, "orch may write in {why}");
        assert!(
            evaluator.touch(&dir, "x").is_some(),
            "the evaluator must be refused {why}: {}",
            dir.display(),
        );
    }
}
