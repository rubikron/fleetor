//! **A past session that cannot be reopened says why, says what survives, and
//! offers nothing to click** (WP-27 S2; R8, R10, R19).
//!
//! `runs.rs` proves the reopen gate produces a reason. What it cannot prove is that
//! the reason reaches a screen: a History row that dropped `cannot_reopen`, or
//! stayed clickable, would pass every backend test and be a refusal that explains
//! itself only after it fails. So this renders the real component and asserts on
//! markup React actually produced — `pane_head_renders.rs`'s move, for its reason:
//! the frontend has no test runner and this does not add one (C24).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

const COMPONENT: &str = "ui/src/components/RunHistory.tsx";
const PROBE: &str = "src-tauri/tests/history_probe/render.tsx";
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

/// What the probe printed, or `None` with a banner explaining what this machine did
/// not measure. A missing probe or component is a failure, never a skip (D-081);
/// only a missing `esbuild`/`node` skips.
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
            format!("SKIPPED: the History row render tier — {BUNDLER} is not installed."),
            "  Run `npm install`. Nothing below was measured: that a row which".into(),
            "  cannot reopen says why and offers nothing to click (WP-27 S2).".into(),
            "  This machine is running a strictly weaker suite.".into(),
        ]);
        return None;
    }

    let bundle = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("history_probe.cjs");
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
                format!("SKIPPED: the History row render tier — could not run {BUNDLER}: {e}."),
                "  Nothing about the History rows' markup was measured.".into(),
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
                format!("SKIPPED: the History row render tier — no `{RUNTIME}` on PATH ({e})."),
                "  The list was not rendered, so nothing about it was measured.".into(),
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

/// One rendering, by the key the probe printed it under.
fn markup(all: &serde_json::Value, key: &str) -> String {
    all.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("{PROBE} printed no `{key}`: {all}"))
        .to_string()
}

/// The rows of one rendered list, each `<li>` on its own.
fn rows(list: &str) -> Vec<String> {
    list.split("<li class=\"run").skip(1).map(str::to_string).collect()
}

/// **The row that cannot open states its cause and what survives, and is not a
/// button** — beside a control row that is one, so a list that rendered no buttons
/// at all could not pass.
#[test]
fn a_row_that_cannot_reopen_says_why_and_offers_nothing_to_click() {
    let Some(all) = rendered() else { return };
    let list = markup(&all, "mixed");
    let rows = rows(&list);
    assert_eq!(rows.len(), 2, "the probe renders two sessions:\n{list}");

    let blocked = rows
        .iter()
        .find(|r| r.contains("worker-2 recorded no session"))
        .unwrap_or_else(|| panic!("the blocked row no longer renders its cause:\n{list}"));
    assert!(
        blocked.contains("Can’t reopen — worker-2 recorded no session"),
        "the cause must be stated as a refusal, not left as a bare fragment:\n{blocked}"
    );
    assert!(
        blocked.contains("intact and still export"),
        "a blocked row must say what survives the refusal:\n{blocked}"
    );
    assert!(
        !blocked.contains("<button class=\"run__label\""),
        "a row that cannot open must offer nothing to open:\n{blocked}"
    );

    let opens = rows.iter().find(|r| !r.contains("Can’t reopen")).expect("the openable row");
    assert!(
        opens.contains("<button class=\"run__label\""),
        "the control: a row that opens is a button:\n{opens}"
    );
}

/// **When nothing opens, what survives is said once, and every row still gives
/// its own cause** — the causes differ, the survivors do not.
#[test]
fn when_nothing_opens_what_survives_is_said_once() {
    let Some(all) = rendered() else { return };
    let list = markup(&all, "nothing_opens");
    assert_eq!(
        list.matches("intact and still export").count(),
        1,
        "said once for the whole list, not once per row:\n{list}"
    );
    let rows = rows(&list);
    assert_eq!(rows.len(), 2, "{list}");
    assert!(
        rows.iter().all(|r| r.contains("Can’t reopen — ")),
        "each row still states its own cause:\n{list}"
    );
}
