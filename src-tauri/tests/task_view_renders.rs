//! The Tasks view, rendered from the real component under node (D-100, Seam 3).
//! The harness is `history_row_renders.rs`'s; see that file for why it skips
//! loudly rather than fails when the JS toolchain is absent.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

const COMPONENT: &str = "ui/src/components/TaskBoard.tsx";
const PROBE: &str = "src-tauri/tests/task_probe/render.tsx";
const BUNDLER: &str = "node_modules/.bin/esbuild";
const RUNTIME: &str = "node";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("src-tauri has a parent").to_path_buf()
}

fn announce(lines: &[String]) {
    let banner = format!(
        "\n\
         ┌──────────────────────────────────────────────────────────────────┐\n\
         {}\n\
         └──────────────────────────────────────────────────────────────────┘\n",
        lines.iter().map(|l| format!("│ {l}")).collect::<Vec<_>>().join("\n")
    );
    let stderr = std::io::stderr();
    let mut stderr = stderr.lock();
    let _ = stderr.write_all(banner.as_bytes());
    let _ = stderr.flush();
}

fn rendered() -> Option<serde_json::Value> {
    let root = repo_root();

    for required in [PROBE, COMPONENT] {
        assert!(
            root.join(required).is_file(),
            "{required} is part of this test. It is missing, which is a deleted component or a \
             deleted probe rather than an absent toolchain (C63). Restore it; do not delete \
             this test."
        );
    }

    let bundler = root.join(BUNDLER);
    if !bundler.is_file() {
        announce(&[
            format!("SKIPPED: the Tasks view render tier — {BUNDLER} is not installed."),
            "  Run `npm install`. Nothing below was measured: the task list, the".into(),
            "  task page and its chain, and the read-only banner (D-100).".into(),
            "  This machine is running a strictly weaker suite.".into(),
        ]);
        return None;
    }

    let bundle = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("task_probe.cjs");
    let built = Command::new(&bundler)
        .arg(root.join(PROBE))
        .args(["--bundle", "--platform=node", "--format=cjs", "--jsx=automatic"])
        .arg(format!("--outfile={}", bundle.display()))
        .arg("--log-level=warning")
        .current_dir(&root)
        .output();

    let built = match built {
        Ok(out) => out,
        Err(e) => {
            announce(&[
                format!("SKIPPED: the Tasks view render tier — could not run {BUNDLER}: {e}."),
                "  Nothing about the Tasks view's markup was measured.".into(),
            ]);
            return None;
        }
    };
    assert!(
        built.status.success(),
        "bundling {PROBE} failed. The probe imports the real {COMPONENT}, so this is that \
         component failing to build:\n{}\n{}",
        String::from_utf8_lossy(&built.stdout),
        String::from_utf8_lossy(&built.stderr)
    );

    let ran = match Command::new(RUNTIME).arg(&bundle).current_dir(&root).output() {
        Ok(out) => out,
        Err(e) => {
            announce(&[
                format!("SKIPPED: the Tasks view render tier — no `{RUNTIME}` on PATH ({e})."),
                "  The view was not rendered, so nothing about it was measured.".into(),
            ]);
            return None;
        }
    };
    assert!(
        ran.status.success(),
        "{PROBE} did not render. This is the component throwing, not a missing toolchain:\n{}",
        String::from_utf8_lossy(&ran.stderr)
    );

    let out = String::from_utf8_lossy(&ran.stdout).to_string();
    Some(serde_json::from_str(&out).unwrap_or_else(|e| {
        panic!("{PROBE} must print the renderings as JSON: {e}\n--- stdout ---\n{out}")
    }))
}

fn markup(all: &serde_json::Value, key: &str) -> String {
    all.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("{PROBE} printed no `{key}`: {all}"))
        .to_string()
}

fn rows(list: &str) -> Vec<String> {
    list.split("<button type=\"button\" class=\"task-row").skip(1).map(str::to_string).collect()
}

/// The list groups tasks under their goal, counts a goal's tasks by status as
/// text, and gives each row its status, owner, latest entry and comment count.
#[test]
fn the_list_groups_tasks_under_their_goal_and_each_row_answers_at_a_glance() {
    let Some(all) = rendered() else { return };
    let list = markup(&all, "list");
    let rows = rows(&list);
    assert_eq!(rows.len(), 4, "a goal, its two tasks and one stray:\n{list}");

    assert!(rows[0].contains("task-row--goal") && rows[0].contains("#1"), "{}", rows[0]);
    assert!(rows[0].contains("1 done · 1 dropped"), "counts are text, by status: {}", rows[0]);
    assert!(!list.contains("<progress") && !list.contains("✓"), "nothing claims completion:\n{list}");

    assert!(rows[1].contains("#2") && rows[1].contains("task__status--done"), "{}", rows[1]);
    assert!(rows[1].contains("worker-2"), "the owner: {}", rows[1]);
    assert!(rows[1].contains("marked it done"), "the latest entry: {}", rows[1]);
    assert!(rows[1].contains("1 comment"), "{}", rows[1]);
    assert!(rows[2].contains("unowned"), "{}", rows[2]);

    let stray = list.split("No goal").nth(1).expect("a No goal group");
    assert!(stray.contains("#4") && stray.contains("the tokenizer leaks"), "{stray}");
    assert!(!list.contains("task-page"), "no page until a row is opened");
    assert!(!list.contains("read-only"), "a live fleet's store is not read-only");
}

/// The task page shows the current text and the whole chain: who did what,
/// both texts of an edit, and where one session ended.
#[test]
fn the_task_page_shows_the_chain_with_edits_and_run_boundaries() {
    let Some(all) = rendered() else { return };
    let page = markup(&all, "page");
    let page = page.split("<article class=\"task-page\"").nth(1).expect("an open task page");

    assert!(page.contains("clippy is clean"), "the edited criteria are current: {page}");
    assert!(page.contains("serves goal") && page.contains("one grammar"), "{page}");
    let entries: Vec<&str> = page.split("<li class=\"chain__").skip(1).collect();
    let kinds: Vec<&str> = entries.iter().map(|e| e.split('"').next().unwrap()).collect();
    assert_eq!(
        kinds,
        vec![
            "entry chain__entry--opened",
            "entry chain__entry--taken-up",
            "entry chain__entry--commented",
            "entry chain__entry--edited",
            "run",
            "entry chain__entry--status",
        ],
        "one line per entry, and a boundary where the run id changes",
    );
    assert!(entries[2].contains("worker-3") && entries[2].contains("the tokenizer leaks a buffer"));
    let edit = entries[3];
    let (was, now) = edit.split_once(">now<").expect("an edit shows both texts");
    assert!(was.contains(">was<") && !was.contains("clippy is clean"), "{edit}");
    assert!(now.contains("clippy is clean"), "{edit}");
    assert!(entries[5].contains("cargo test passes"), "a status keeps its note: {}", entries[5]);
}

/// With no fleet running the same records show, marked read-only.
#[test]
fn with_no_fleet_the_view_is_marked_read_only() {
    let Some(all) = rendered() else { return };
    let readonly = markup(&all, "readonly");
    assert!(readonly.contains("read-only · start a fleet to change tasks"), "{readonly}");
    assert_eq!(rows(&readonly).len(), 4);

    // Every operator control is there, off, and says why.
    let controls: Vec<&str> = readonly
        .split("<button type=\"")
        .skip(1)
        .filter(|b| !b.contains("aria-pressed"))
        .collect();
    let labels = ["New goal", "New task", "Edit", "Close", "Reopen", "Comment"];
    assert_eq!(controls.len(), labels.len(), "{controls:#?}");
    for (control, label) in controls.iter().zip(labels) {
        let tag = control.split('>').next().unwrap();
        assert!(control.contains(&format!(">{label}<")), "{control}");
        assert!(tag.contains("disabled") && tag.contains("start a fleet to change tasks"), "{label}: {tag}");
    }
    let empty = markup(&all, "empty");
    assert!(empty.contains("No goals or tasks for this repository yet"), "{empty}");
}

fn fields(form: &str) -> Vec<String> {
    form.split("class=\"composer__label\">").skip(1).map(|f| f.split('<').next().unwrap().to_string()).collect()
}

/// The operator's forms carry the fields `fleet task post` and `edit` take.
#[test]
fn the_operators_forms_carry_the_same_fields_an_agent_uses() {
    let Some(all) = rendered() else { return };

    let goal = markup(&all, "newGoal");
    let goal = goal.split("<form class=\"task-form\"").nth(1).expect("the new goal form");
    assert!(goal.contains("New goal"));
    assert_eq!(fields(goal), vec!["outcome", "vision criteria, one per line"], "a goal needs no technical checks");

    let task = markup(&all, "newTask");
    let task = task.split("<form class=\"task-form\"").nth(1).expect("the new task form");
    assert_eq!(
        fields(task),
        vec![
            "outcome",
            "technical criteria, one per line",
            "vision criteria, one per line",
            "instructions (optional)",
            "goal",
            "owner",
        ],
    );
    assert!(task.contains(">No goal<") && task.contains("#1 one grammar"), "a goal is optional: {task}");
    assert!(task.contains(">unowned<") && task.contains(">worker-4<"), "an owner is optional: {task}");
    let tell = task.split("type=\"checkbox\"").nth(1).expect("the message-the-owner choice");
    assert!(tell.contains("message the owner with this task"));
    assert!(tell.split('>').next().unwrap().contains("disabled"), "off until an owner is chosen, and never ticked for you");

    let edit = markup(&all, "editing");
    let edit = edit.split("<form class=\"task-form\"").nth(1).expect("the edit form");
    assert!(edit.contains("Edit #2") && edit.contains("value=\"nested groups parse\""), "prefilled: {edit}");
    assert!(edit.contains("cargo test -p parser\nclippy is clean"), "the whole current list, to restate: {edit}");
}

/// A live record offers comment, edit and close; a closed one offers reopen.
#[test]
fn a_live_record_offers_comment_edit_close_and_reopen() {
    let Some(all) = rendered() else { return };
    let controls = |key: &str| {
        let page = markup(&all, key);
        page.split("<div class=\"task-controls\">").nth(1).expect("the controls").to_string()
    };
    let done = controls("page");
    for label in ["Edit", "Close", "Reopen", "Comment"] {
        assert!(done.contains(&format!(">{label}<")), "a done task offers {label}: {done}");
    }
    assert!(!done.contains("start a fleet"), "nothing is off while a fleet runs: {done}");
    let dropped = controls("dropped");
    assert!(dropped.contains(">Reopen<") && !dropped.contains(">Close<"), "{dropped}");
}

/// The list narrows to one status; a goal stays while one of its tasks matches.
#[test]
fn the_list_filters_by_status_and_keeps_the_goal_of_a_matching_task() {
    let Some(all) = rendered() else { return };
    let list = markup(&all, "list");
    for label in ["All", "Planned", "In progress", "Done"] {
        assert!(list.contains(&format!(">{label}</button>")), "the {label} filter is offered");
    }

    let done = markup(&all, "done");
    let rows = rows(&done);
    assert_eq!(rows.len(), 2, "the goal and its one done task:\n{done}");
    assert!(rows[0].contains("task-row--goal") && rows[0].contains("1 done · 1 dropped"), "the goal still counts every task: {}", rows[0]);
    assert!(rows[1].contains("#2"), "{}", rows[1]);
    assert!(!done.contains("No goal"), "a group with no match is hidden");

    let none = markup(&all, "inProgress");
    assert!(self::rows(&none).is_empty() && none.contains("No in-progress tasks."), "{none}");
}

/// Task entries sit in the Activity feed where they arrived among the run-log
/// events, newest first.
#[test]
fn task_entries_join_the_activity_feed_in_arrival_order() {
    let Some(all) = rendered() else { return };
    let feed = markup(&all, "activity");
    let lines: Vec<&str> = feed.split("line__text\">").skip(1).map(|l| l.split('<').next().unwrap()).collect();
    assert_eq!(
        lines,
        vec![
            "third",
            "worker-2 marked #2 done — cargo test passes",
            "second",
            "worker-2 took up #2 — starting",
            "first",
        ],
    );
    assert!(feed.contains(">seq 3<"), "the head still names the latest run-log seq: {feed}");
}

/// A release is one chain entry showing all four fields and who it was for,
/// and it leaves the task planned and unowned.
#[test]
fn a_release_shows_its_four_fields_and_returns_the_task_to_planned() {
    let Some(all) = rendered() else { return };
    let page = markup(&all, "released");
    let page = page.split("<article class=\"task-page\"").nth(1).expect("an open task page");
    let head = page.split("</header>").next().unwrap();
    assert!(head.contains("task__status--planned") && head.contains("unowned"), "{head}");

    let releases: Vec<&str> = page.split("<li class=\"chain__entry chain__entry--released\"").skip(1).collect();
    assert_eq!(releases.len(), 2, "{page}");
    for label in ["why", "done", "left", "where"] {
        assert!(releases[0].contains(&format!("<dt>{label}</dt>")), "{label}: {}", releases[0]);
    }
    assert!(releases[0].contains("out of context") && releases[0].contains("fleet/logstat/worker-3 @ a1b2c3d"));
    assert!(releases[0].contains(">released this<"), "the owner's own release: {}", releases[0]);
    assert!(releases[1].contains("released this on behalf of worker-1"), "{}", releases[1]);

    let feed = markup(&all, "released_activity");
    assert!(feed.contains("released #5 on behalf of worker-1 — left: the parser"), "{feed}");
}

/// An owner from another session reads as an earlier run, is counted in the
/// "Carried over" banner while its task is open, and can be released for.
#[test]
fn an_earlier_runs_owner_is_named_counted_and_can_be_released_for() {
    let Some(all) = rendered() else { return };
    let view = markup(&all, "releasing");
    let rows = rows(&view);
    let held = rows.iter().find(|row| row.contains("#6")).expect("task #6");
    assert!(held.contains("worker-4, earlier run"), "{held}");
    assert!(view.contains("<strong>Carried over</strong> 1 open task owned by an"), "{view}");
    assert!(!markup(&all, "list").contains("Carried over"), "nothing open is carried in the first board");

    let form = view.split("task-form--release").nth(1).expect("the release form");
    assert_eq!(
        fields(form),
        ["why it is being released", "done so far", "left to do", "where the work sits: branch @ commit"],
    );
    let submit = form.split("<button type=\"submit\"").nth(1).unwrap().split('>').next().unwrap();
    assert!(submit.contains("disabled") && submit.contains("all four fields are required"), "{submit}");

    let offered = markup(&all, "released");
    let controls = offered.split("<div class=\"task-controls\">").nth(1).unwrap();
    assert!(!controls.contains(">Release<"), "an unowned task has nothing to release: {controls}");
}

/// A receipt shows what was run, how it exited and where, and says so when
/// the work was uncommitted or the message never reached orch.
#[test]
fn a_receipt_shows_the_check_its_exit_and_where_it_ran() {
    let Some(all) = rendered() else { return };
    let page = markup(&all, "released");
    let receipt = page.split("<li class=\"chain__entry chain__entry--receipt\"").nth(1).expect("a receipt entry");
    let receipt = receipt.split("</li>").next().unwrap();
    assert!(receipt.contains("ran a check · exit 101"), "{receipt}");
    assert!(receipt.contains("cargo test -p parser"), "{receipt}");
    assert!(receipt.contains("fleet/logstat/worker-1 @ d4e5f6a") && receipt.contains("+ uncommitted changes"), "{receipt}");
    assert!(receipt.contains("was not delivered to orch"), "{receipt}");
    assert!(!receipt.contains("✓") && !receipt.contains("passed"), "a receipt asserts nothing: {receipt}");

    let feed = markup(&all, "released_activity");
    assert!(feed.contains("ran `cargo test -p parser` for #5 · exit 101"), "{feed}");
}
