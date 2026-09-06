//! **The pane head names the harness the pane was actually placed as** (WP-25 #50;
//! C56, C57, C63, C24, M24, M25).
//!
//! The bug this closes was not a missing feature. `TerminalGrid.tsx` labelled every
//! seat `orchestrator · claude` and `worker-N · claude`, so a codex worker rendered
//! as `worker-2 · claude deepseek-v4-flash` — **the model right and the vendor
//! wrong**, which is worse than stating none, and worse still because the run's own
//! `manifest.json` knew the answer the live screen was getting wrong (C56).
//!
//! `gate_pickers.rs::the_pane_chrome_spells_no_vendors_name` is the half that reads
//! source: it proves no vendor's name or program is spelled anywhere in the chrome,
//! and it fires on a planted literal. **What it cannot prove is that anything
//! renders.** A head that dropped the harness entirely would pass it, and so would
//! one restructured into a branch that never mounts — the failure #35 shipped once
//! and C63 was written about. So this file renders the real component and asserts on
//! markup React actually produced.
//!
//! ## Why a Rust test for a TypeScript component
//!
//! The same move `tests/gauge_unavailable_renders.rs` and
//! `tests/gate_seat_row_renders.rs` make, for the same reason: the frontend has no
//! test runner and this arc does not add one (C24). This bundles
//! `tests/pane_probe/render.tsx` with the esbuild already in the repo's
//! `node_modules` and runs it under `node` — no vitest, no jest, no `node:test`, no
//! new dependency.
//!
//! ## The criterion that has no other home
//!
//! **"Registering a third harness needs no `ui/` change" is asserted here**, and it
//! is asserted the only way it can be short of registering one: every harness the
//! probe renders — `alpha-cli`, `beta-cli`, `gamma-cli`, `delta-cli` — is registered
//! in no build, and the components render all four without ever having been edited
//! for any of them. The mark comes off `HarnessSpec::mark` and travels beside the
//! name on `FleetEvent::PaneState`; a component that chose the glyph itself would
//! render `gamma-cli` blank here and would fail `gate_pickers.rs` by construction.
//!
//! `harness.rs::every_registered_harness_supplies_its_own_mark` is the other half:
//! that the wire will carry a mark for whatever is registered.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

const COMPONENT: &str = "ui/src/components/PaneHead.tsx";
const PROBE: &str = "src-tauri/tests/pane_probe/render.tsx";
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
/// not measure.
///
/// **A missing probe or a missing component is a failure, never a skip** (D-081) — a
/// deleted head or a deleted probe, not an absent toolchain. Only a missing
/// `esbuild`/`node` skips.
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
            format!("SKIPPED: the pane-head render tier — {BUNDLER} is not installed."),
            "  Run `npm install`. Nothing below was measured: that the pane".into(),
            "  head renders the harness it was handed, and invents none when".into(),
            "  the pane has not spawned (#50).".into(),
            "  This machine is running a strictly weaker suite.".into(),
        ]);
        return None;
    }

    let bundle = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("pane_probe.cjs");
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
                format!("SKIPPED: the pane-head render tier — could not run {BUNDLER}: {e}."),
                "  Nothing about the pane head's markup was measured.".into(),
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
                format!("SKIPPED: the pane-head render tier — no `{RUNTIME}` on PATH ({e})."),
                "  The head was not rendered, so nothing about it was measured.".into(),
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

/// **A spawned pane's head states its own harness, its mark and its model.**
///
/// The behavioural half of the ticket: all three come off `FleetEvent::PaneState`
/// and none of them is a literal in the interface. The pane's *name* is still the
/// interface's own — `worker-2` — and the harness sits beside it rather than inside
/// it, which is the shape the old `worker-2 · claude` label could not have.
#[test]
fn the_head_states_the_harness_and_model_the_pane_was_placed_as() {
    let Some(all) = rendered() else { return };
    let head = markup(&all, "spawned");
    assert!(
        head.contains(">worker-2<"),
        "the head no longer names the pane it describes:\n{head}"
    );
    assert!(
        head.contains(">alpha-cli<"),
        "the head no longer states the harness the pane was placed as — the fact this \
         ticket exists to put on screen:\n{head}"
    );
    assert!(
        head.contains(">deepseek-v4-flash<"),
        "the head no longer states the model:\n{head}"
    );
    assert!(
        head.contains(">AC<"),
        "the head carries no mark, so a mixed fleet is told apart by reading:\n{head}"
    );
    // The harness is a fact beside the pane's name, never spliced into it. A head
    // rendering `worker-2 · alpha-cli` as one string would satisfy every check above
    // and would be the old label with the literal swapped for a variable.
    assert!(
        !head.contains("worker-2 ·") && !head.contains("worker-2 alpha-cli"),
        "the harness is spliced into the pane's label again:\n{head}"
    );
}

/// **A pane whose harness is not yet known invents none.**
///
/// The acceptance criterion the shipped literal failed hardest: an unspawned pane
/// had a vendor's name in its head before any placement had happened at all. It now
/// renders its name and its status and nothing else — the same rule `PaneGauge`
/// keeps about a figure nobody has read (C69).
#[test]
fn a_pane_that_has_not_spawned_names_no_harness() {
    let Some(all) = rendered() else { return };
    let head = markup(&all, "unspawned");
    assert!(
        head.contains(">worker-3<"),
        "the unspawned head stopped naming its own pane, which is not the absent fact:\n{head}"
    );
    assert!(
        !head.contains("pane__harness") && !head.contains("harness-mark"),
        "a pane with no identity renders a harness anyway. Absence means unknown here, and a \
         head that fills it in is the bug #50 closed with a different value in it:\n{head}"
    );
    let bare = markup(&all, "tab_mark_unspawned");
    assert!(
        bare.is_empty(),
        "the tab strip renders a mark for a pane that has not spawned:\n{bare}"
    );
}

/// **A harness the build has never registered renders exactly like the ones it
/// has** — the acceptance criterion "registering a third harness requires no `ui/`
/// change", asserted rather than asserted-about.
///
/// `gamma-cli` is in no registry and no `ui/` file names it. If the head chose its
/// glyph or its label by asking which harness this is, this rendering would be the
/// one that came out empty.
#[test]
fn a_harness_this_build_never_registered_renders_the_same_way() {
    let Some(all) = rendered() else { return };
    let known = markup(&all, "spawned");
    let third = markup(&all, "third_harness");
    assert!(
        third.contains(">gamma-cli<") && third.contains(">GC<")
            && third.contains(">orion-2-thinking<"),
        "a harness this build does not know renders without its name, mark or model — which \
         is a branch in the component, and the archaeology C57 says this seam ends:\n{third}"
    );
    // The same markup shape, not merely the same strings: whatever wrapper the known
    // harness's facts get, the unknown one gets too.
    for class in ["pane__harness", "harness-mark", "pane__mark", "pane__meta"] {
        assert_eq!(
            known.matches(class).count(),
            third.matches(class).count(),
            "`{class}` appears a different number of times for a registered harness than for \
             one this build has never heard of:\n--- known ---\n{known}\n--- third ---\n{third}"
        );
    }
}

/// **An attended seat states its harness and no model** (M2, C56).
///
/// The orchestrator runs the operator's own login, and the model that account
/// defaults to is not a name this side of the pty. The old head printed
/// `opus (operator)` there — a model literal standing in for a fact nobody has.
#[test]
fn an_attended_seat_states_its_harness_and_names_no_model() {
    let Some(all) = rendered() else { return };
    let head = markup(&all, "attended");
    assert!(
        head.contains(">beta-cli<") && head.contains(">BC<"),
        "the attended seat no longer says what it is running:\n{head}"
    );
    // One `pane__meta` span, the harness's. A second would be a model rendered for a
    // seat that named none.
    assert_eq!(
        head.matches("pane__meta").count(),
        1,
        "the attended seat renders a second fact beside its harness. Its model is absent on \
         purpose and an absence may not be filled in (M2):\n{head}"
    );
}

/// **A harness that supplied no mark gets none, and is still named.**
///
/// An older run replayed out of the log carries a `harness` and no `mark`. The two
/// are separate facts and the absent one is not substituted for.
#[test]
fn a_harness_that_supplied_no_mark_is_still_named() {
    let Some(all) = rendered() else { return };
    let head = markup(&all, "no_mark");
    assert!(
        head.contains(">delta-cli<") && head.contains(">delta-1<"),
        "a spawn event carrying no mark lost the harness and model it did carry:\n{head}"
    );
    assert!(
        !head.contains("harness-mark"),
        "a mark was rendered for a harness that supplied none — a substitute glyph is the \
         same invention as a substitute name:\n{head}"
    );
}

/// **The mark reaches the worker tab strip**, which is the reason it exists: four
/// workers are compared there without selecting any of them.
#[test]
fn the_mark_renders_on_the_tab_strip_too() {
    let Some(all) = rendered() else { return };
    let mark = markup(&all, "tab_mark");
    assert!(
        mark.contains("tab__mark") && mark.contains(">GC<"),
        "the tab strip's mark is gone, so a mixed fleet is only distinguishable by opening \
         each tab and reading its head:\n{mark}"
    );
    assert!(
        mark.contains("title=\"gamma-cli\""),
        "the mark carries no hover naming the harness it stands for. A glyph nobody can \
         expand is a glyph nobody can learn:\n{mark}"
    );
}

/// **The probe cannot fake a pass.** The strings the assertions above look for must
/// come out of `PaneHead`/`HarnessMark`, not out of the probe's own source —
/// otherwise a component that stopped rendering any of them would still leave this
/// suite green.
#[test]
fn the_probe_supplies_data_and_never_the_markup() {
    let probe = std::fs::read_to_string(repo_root().join(PROBE)).expect("probe reads");
    for phrase in ["pane__harness", "pane__meta", "pane__title", "harness-mark", "title=", "<span"]
    {
        assert!(
            !probe.contains(phrase),
            "{PROBE} contains `{phrase}`, which is markup `PaneHead`/`HarnessMark` are \
             supposed to produce — a test asserting on it would pass on the probe's own text \
             rather than on what the component rendered"
        );
    }
}

/// **The declarations this file reads are still there.**
#[test]
fn the_declarations_this_file_reads_still_exist() {
    for rel in [COMPONENT, PROBE] {
        let file = repo_root().join(rel);
        assert!(file.is_file(), "{} is read by this file and is gone", file.display());
    }
    let component = std::fs::read_to_string(repo_root().join(COMPONENT)).expect("component reads");
    for declared in ["export function PaneHead(", "export function HarnessMark("] {
        assert!(
            component.contains(declared),
            "`{declared}` is gone from {COMPONENT} — the probe imports both by name"
        );
    }
}
