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
//!    `settings.json` command `guardrail::install` wrote, executed by a real
//!    shell against a real `PreToolUse` payload. No `claude`, no tokens, and no
//!    second copy of the policy for the test to agree with while the pane runs
//!    something else. `examples/write-guardrail-spike/probe.py` is the arm that
//!    proves Claude Code honours the answer; this is the arm that proves the
//!    answer is right.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use fleetor_core::pane::PaneId;
use fleetor_shell::guardrail;

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

struct Pane {
    dir: PathBuf,
    command: String,
    journal: PathBuf,
}

/// Install a real guardrail for `pane` and hand back the shell command Claude
/// Code would run, read out of the `settings.json` that was just written.
fn install(tag: &str, pane: PaneId, cwd: &Path, shell: &Path, extra: &[String]) -> Pane {
    let dir = std::env::temp_dir().join(format!(
        "fleetor-wg-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let journal = guardrail::journal_path(shell);
    std::fs::create_dir_all(shell).unwrap();
    guardrail::install(
        &dir,
        pane,
        &guardrail::roots_for(cwd, shell, extra),
        &guardrail::policy_dir(shell),
        &journal,
    )
    .unwrap();

    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
    let command = settings["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
        .as_str()
        .expect("the hook command Claude Code will run")
        .to_string();
    Pane { dir, command, journal }
}

impl Pane {
    /// Ask the installed hook about one tool call, exactly as Claude Code does:
    /// the command through a shell, the event as JSON on stdin.
    fn ask(&self, cwd: &Path, tool: &str, input: serde_json::Value) -> Option<String> {
        let payload = serde_json::json!({
            "hook_event_name": "PreToolUse",
            "cwd": cwd.display().to_string(),
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

    fn bash(&self, cwd: &Path, command: &str) -> Option<String> {
        self.ask(cwd, "Bash", serde_json::json!({ "command": command }))
    }

    fn cleanup(&self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn shell_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fleetor-wg-shell-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// **The allowlist holds.** Everything a worker legitimately does inside its own
/// worktree — and everything the fleet's own flows do around it — is allowed.
#[test]
fn the_allowlist_holds_for_the_work_a_worker_actually_does() {
    let shell = shell_dir("holds");
    let worktree = shell.join("worktrees/worker-1");
    let pane = install("holds", PaneId::Worker(1), &worktree, &shell, &[]);

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
        assert_eq!(pane.bash(&worktree, allowed), None, "a worker must be able to run: {allowed}");
    }

    // Reads are open, deliberately and structurally: the hook is not even
    // registered for the tools that do them.
    assert_eq!(
        pane.ask(&worktree, "Read", serde_json::json!({ "file_path": "/etc/hosts" })),
        None,
    );
    // And the two roots themselves.
    assert_eq!(pane.bash(&worktree, &format!("touch {}/x", worktree.display())), None);
    assert_eq!(pane.bash(&worktree, &format!("touch {}/worktrees/worker-2/x", shell.display())), None);

    pane.cleanup();
}

/// **A write outside it is refused with a usable reason.** The refusal is
/// written for the model that has to recover from it — the same principle as
/// `prompts/delivery-contract.md`, where a model reads its own exit code and
/// adapts. A denial that did not name the path and the rule would leave a pane
/// unable to tell a guardrail from a broken path.
#[test]
fn a_write_outside_the_roots_is_refused_with_a_reason_a_pane_can_act_on() {
    let shell = shell_dir("refusal");
    let worktree = shell.join("worktrees/worker-1");
    let pane = install("refusal", PaneId::Worker(1), &worktree, &shell, &[]);

    let why = pane
        .bash(&worktree, "echo notes > /Users/somebody/notes.txt")
        .expect("a write outside every root must be refused");

    assert!(why.contains("/Users/somebody/notes.txt"), "it names the path: {why}");
    assert!(why.contains("worker-1"), "and whose workspace it is outside of: {why}");
    assert!(why.contains(&worktree.display().to_string()), "and where it may write: {why}");
    assert!(why.contains("Reading is not restricted"), "and what the rule actually is: {why}");
    assert!(why.contains("fleet send operator"), "and what to do if it truly belongs there: {why}");
    assert!(
        why.contains("Retrying the same path will be refused again"),
        "and that a retry is not the move — this is the difference between self-correcting \
         and looping: {why}",
    );

    // Structured writes are the exact half: the destination is a field, so the
    // refusal is enforcement rather than a scan of a command string.
    for (tool, input) in [
        ("Write", serde_json::json!({ "file_path": "/Users/somebody/CLAUDE.md", "content": "x" })),
        ("Edit", serde_json::json!({ "file_path": "../../../escaped.rs" })),
        ("NotebookEdit", serde_json::json!({ "notebook_path": "/Users/somebody/x.ipynb" })),
    ] {
        assert!(pane.ask(&worktree, tool, input).is_some(), "{tool} must be refused");
    }

    // And the refusal reached the journal, which is what puts it on the feed.
    let journal = std::fs::read_to_string(&pane.journal).unwrap();
    assert!(journal.contains("worker-1"), "{journal}");
    assert!(journal.contains("notes.txt"), "{journal}");

    pane.cleanup();
}

/// **`orch` and workers get correct per-pane roots.** `orch` works in the target
/// repo and a worker does not; a worker works in its own worktree and `orch` has
/// no business in it. Both share `_shell` and nothing else.
#[test]
fn orch_gets_the_target_repo_and_a_worker_gets_only_its_own_worktree() {
    let shell = shell_dir("per-pane");
    let target = std::env::temp_dir().join("fleetor-wg-target");
    let worktree = shell.join("worktrees/worker-2");

    let orch = install("orch", PaneId::Orch, &target, &shell, &[]);
    let worker = install("worker", PaneId::Worker(2), &worktree, &shell, &[]);

    let in_target = format!("touch {}/src/main.rs", target.display());
    assert_eq!(orch.bash(&target, &in_target), None, "orch works in the target repo");
    assert!(
        worker.bash(&worktree, &in_target).is_some(),
        "a worker may not write into the target repo — that is what its worktree is for",
    );

    let in_worktree = format!("touch {}/parser.rs", worktree.display());
    assert_eq!(worker.bash(&worktree, &in_worktree), None, "a worker works in its own worktree");
    assert_eq!(
        orch.bash(&target, &in_worktree),
        None,
        "and `_shell` is on both lists, so orch reaching a worktree is allowed by that root",
    );

    let outside = "touch /Users/somebody/scratch";
    assert!(orch.bash(&target, outside).is_some(), "orch is fenced too, not merely workers");
    assert!(worker.bash(&worktree, outside).is_some());

    orch.cleanup();
    worker.cleanup();
}

/// The one directory inside the roots that is still off limits. `_shell` has to
/// be writable — the worktrees are in it — and the pane config dirs inside it
/// hold this guardrail's own rules. A pane that could edit its own
/// `settings.json` could switch the guardrail off between two tool calls.
#[test]
fn no_pane_may_write_its_own_policy_even_though_it_is_inside_a_root() {
    let shell = shell_dir("policy");
    let worktree = shell.join("worktrees/worker-1");
    let pane = install("policy", PaneId::Worker(1), &worktree, &shell, &[]);
    let settings = guardrail::policy_dir(&shell).join("worker-1/settings.json");

    let why = pane
        .bash(&worktree, &format!("rm {}", settings.display()))
        .expect("a pane must not be able to delete its own guardrail");
    assert!(why.contains("fleet policy"), "the reason says what kind of rule this is: {why}");

    assert!(pane
        .ask(&worktree, "Edit", serde_json::json!({ "file_path": settings.display().to_string() }))
        .is_some());
    // Reading it is fine. The pane may know exactly what it is not allowed to do.
    assert_eq!(
        pane.ask(&worktree, "Read", serde_json::json!({ "file_path": settings.display().to_string() })),
        None,
    );

    pane.cleanup();
}

/// **The operator's extension mechanism works** — `[fence] allow` in
/// `prompts/launch.conf`, through `LaunchConfig::fence_allow`, to a root the
/// pane may really write in.
#[test]
fn a_root_the_operator_added_is_a_root_the_pane_can_write_in() {
    let shell = shell_dir("extended");
    let worktree = shell.join("worktrees/worker-1");
    let scratch = std::env::temp_dir().join("fleetor-wg-operator-scratch");

    let plain = install("plain", PaneId::Worker(1), &worktree, &shell, &[]);
    let write = format!("echo hi > {}/notes.txt", scratch.display());
    assert!(plain.bash(&worktree, &write).is_some(), "not a root until the operator says so");
    plain.cleanup();

    let extended = install(
        "extended",
        PaneId::Worker(1),
        &worktree,
        &shell,
        &[scratch.display().to_string()],
    );
    assert_eq!(extended.bash(&worktree, &write), None, "and a root once they do");
    assert!(
        extended.bash(&worktree, "echo hi > /Users/somebody/else").is_some(),
        "one added root is one added root, not an open door",
    );
    extended.cleanup();
}

/// The measured half of `docs/notes/write-guardrail-notes.md`, pinned: the two
/// writes a pane cannot work without are writes it does not *name*, and the
/// guardrail lets them through because of that rather than by accident. A
/// future tightening that broke either would wedge every worker on `fleet
/// done`'s first step, and this is the test that would say so.
#[test]
fn the_two_unnamed_writes_a_pane_cannot_work_without_are_allowed() {
    let shell = shell_dir("unnamed");
    let worktree = shell.join("worktrees/worker-1");
    let pane = install("unnamed", PaneId::Worker(1), &worktree, &shell, &[]);

    // Measured: writes 54 files into the worktree's own target/ and one into the
    // operator's ~/.cargo/registry, and names neither.
    assert_eq!(pane.bash(&worktree, "cargo build"), None);
    // Measured: writes 8 files into the target repo's .git — outside a worker's
    // roots — and names none of them. `fleet done` runs this first.
    assert_eq!(pane.bash(&worktree, "git commit -m 'worker-1: a parser'"), None);
    // The same operation aimed somewhere it *does* name is refused, which is the
    // whole distinction this package rests on.
    assert!(pane.bash(&worktree, "git -C /Users/somebody/repo commit -am wip").is_some());

    pane.cleanup();
}
