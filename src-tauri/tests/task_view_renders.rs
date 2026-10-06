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
    let empty = markup(&all, "empty");
    assert!(empty.contains("No goals or tasks for this repository yet"), "{empty}");
}
