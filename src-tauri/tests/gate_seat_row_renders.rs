//! **`SeatRow` actually renders the picker it is declared to** (WP-26 redesign;
//! C63, C24, C12).
//!
//! `gate_pickers.rs` reads the source and proves `SeatRow`'s JSX *says* the harness
//! list is never filtered, that a refused option carries `harness.reason`, and that
//! every status word has a label. That file's own doc comment names the risk this
//! one closes: a source-reading tripwire cannot tell you the component it is
//! quoting is actually the one that mounts, or that a later restructuring did not
//! wrap the whole thing in a branch that never renders. #35 shipped exactly that
//! shape once, behind a fifteen-check passing tripwire with a working negative
//! control (`tests/gauge_unavailable_renders.rs`'s own doc comment). This redesign
//! touches `SeatRow`'s markup directly — the harness `<select>` and the model
//! `<input>` move from a flex row to a grid, specifically to stop the harness picker
//! collapsing to `c.` and the model field clipping `deepseek-v4-flash` to
//! `deepsee` — so it is exactly the kind of change C63 says needs a render, not a
//! read.
//!
//! ## Why a Rust test for a TypeScript component
//!
//! The same move `tests/gauge_unavailable_renders.rs` makes, for the same reason:
//! the frontend has no test runner and this arc does not add one (C24). This file
//! bundles `tests/gate_probe/render.tsx` with the esbuild already in the repo's
//! `node_modules` and runs it under `node` — no vitest, no jest, no `node:test`, no
//! new dependency.
//!
//! ## What is asserted here, and what is asserted next door
//!
//! This proves the component mounts and produces real markup: every harness is an
//! `<option>` regardless of which one is selected, a refused harness carries its
//! reason both as the option's `title` and as the row's visible facts line, the
//! orchestrator's absent model renders as a placeholder rather than a value, and a
//! long model name survives into the field uncut. It does not re-derive
//! `gate_pickers.rs`'s source-level rules (the `.filter(` check, the vendor-name
//! sweep) — those stay where they are, cheaper and already passing.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

const COMPONENT: &str = "ui/src/components/StartGate.tsx";
const PROBE: &str = "src-tauri/tests/gate_probe/render.tsx";
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

/// What the probe printed, or `None` with a banner explaining what this machine
/// did not measure.
///
/// **A missing probe or a missing component is a failure, never a skip** — a
/// deleted render or a deleted picker, not an absent toolchain. Only a missing
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
            format!("SKIPPED: the seat-row render tier — {BUNDLER} is not installed."),
            "  Run `npm install`. Nothing below was measured: that the harness".into(),
            "  picker and the model field actually render, uncrushed and".into(),
            "  uncut (WP-26 redesign).".into(),
            "  This machine is running a strictly weaker suite.".into(),
        ]);
        return None;
    }

    let bundle = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("gate_probe.cjs");
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
                format!("SKIPPED: the seat-row render tier — could not run {BUNDLER}: {e}."),
                "  Nothing about SeatRow's markup was measured.".into(),
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
                format!("SKIPPED: the seat-row render tier — no `{RUNTIME}` on PATH ({e})."),
                "  SeatRow was not rendered, so nothing about it was measured.".into(),
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

/// **Every registered harness is an `<option>`, on every row, whoever is
/// selected.**
///
/// The behavioural half of `gate_pickers.rs`'s
/// `no_harness_is_hidden_and_a_refused_one_carries_its_reason`: that check reads
/// `.filter(` out of the source, which cannot tell a filtered list from one that
/// renders nothing at all for an unrelated reason. This renders four different
/// seats — including one currently sitting on the refused harness — and checks the
/// same two options survive every one of them.
#[test]
fn every_row_offers_both_harnesses_never_just_the_one_selected() {
    let Some(all) = rendered() else { return };
    for key in [
        "orchestrator",
        "worker_named_model",
        "worker_on_refused_harness",
        "worker_on_runnable_harness",
    ] {
        let row = markup(&all, key);
        assert!(
            row.contains("<option value=\"alpha-cli\""),
            "`{key}` dropped the runnable harness from its list:\n{row}"
        );
        assert!(
            row.contains("<option value=\"beta-cli\""),
            "`{key}` dropped the refused harness from its list — exactly the failure user \
             story 9 refuses, an unsupported-looking feature instead of a disabled one:\n{row}"
        );
    }
}

/// **The refused harness's option is disabled and carries the vendor's reason**,
/// both as the hover title and — when it is the one selected — as the row's
/// visible facts line underneath. The reason travels as one string to two places
/// rather than being respelled for the tooltip.
#[test]
fn the_refused_harness_carries_its_reason_on_the_option_and_inline() {
    let Some(all) = rendered() else { return };
    let reason = "no OAuth token found in ~/.beta/auth.json — run `beta-cli login` and try again.";

    let refused_row = markup(&all, "worker_on_refused_harness");
    assert!(
        refused_row.contains(&format!("title=\"{reason}\"")),
        "the refused option's title no longer carries the vendor's reason:\n{refused_row}"
    );
    assert!(
        refused_row.contains("<option value=\"beta-cli\" disabled=\"\""),
        "the refused option is no longer disabled:\n{refused_row}"
    );
    assert!(
        refused_row.contains(&format!("<p class=\"seat-row__facts\">{reason}</p>")),
        "a row sitting on the refused harness no longer shows the reason inline, in the facts \
         line beneath the controls — the state the redesign brief says the shipped card \
         handled worst:\n{refused_row}"
    );
    assert!(
        refused_row.contains("<option value=\"beta-cli\" disabled=\"\" title=")
            && refused_row.contains("selected=\"\">beta-cli — not logged in</option>"),
        "the row on the refused harness no longer shows it as the selected option:\n{refused_row}"
    );

    // And the same option, unselected, still carries the same disabled+reason pair
    // on a row that is not sitting on it — the option's own state, not something
    // that only appears because it happens to be picked.
    let runnable_row = markup(&all, "worker_on_runnable_harness");
    assert!(
        runnable_row.contains(&format!(
            "<option value=\"beta-cli\" disabled=\"\" title=\"{reason}\">beta-cli — not logged in</option>"
        )),
        "the refused option loses its disabled state and reason when it is not the one \
         selected:\n{runnable_row}"
    );
}

/// **The orchestrator's absent model is a placeholder, never a printed value.**
///
/// `DEFAULT_YOUR_LOGIN` is a seat *default*, not a name (M2) — if it rendered as the
/// field's value rather than its placeholder, an operator editing the field would
/// have to clear real-looking text first, and the row would look like it was
/// already naming something nobody chose.
#[test]
fn the_orchestrators_sentinel_is_a_placeholder_not_a_value() {
    let Some(all) = rendered() else { return };
    let row = markup(&all, "orchestrator");
    assert!(
        row.contains("placeholder=\"default (your login)\""),
        "the sentinel is no longer offered as the model field's placeholder:\n{row}"
    );
    assert!(
        row.contains("aria-label=\"orchestrator model\" value=\"\""),
        "the orchestrator's model field now carries a printed value instead of staying empty \
         with the sentinel as its placeholder:\n{row}"
    );
}

/// **A long model name survives into the field whole** — the behavioural half of
/// the `deepsee` clip: `gate_pickers.rs` cannot see a CSS `max-width`, so this
/// renders a seat with a model name long enough to have been cut and asserts the
/// full string is what the `<input>`'s `value` attribute carries.
#[test]
fn a_long_model_name_reaches_the_field_uncut() {
    let Some(all) = rendered() else { return };
    let row = markup(&all, "worker_named_model");
    assert!(
        row.contains("value=\"deepseek-v4-flash-preview\""),
        "the model field no longer carries the full model string in its value attribute — a \
         truncated one would still contain `deepseek-v4-flash` as a prefix, which is why this \
         checks the whole string:\n{row}"
    );
}

/// **The probe cannot fake a pass.** The strings the assertions above look for must
/// come out of `SeatRow`/`optionLabel`/`factsFor`, not out of the probe's own
/// source — otherwise a component that stopped rendering any of them would still
/// leave this suite green.
#[test]
fn the_probe_supplies_data_and_never_the_markup() {
    let probe = std::fs::read_to_string(repo_root().join(PROBE)).expect("probe reads");
    for phrase in [
        "— not logged in",
        "seat-row__facts",
        "seat-row__harness",
        "<option",
        "disabled=",
    ] {
        assert!(
            !probe.contains(phrase),
            "{PROBE} contains `{phrase}`, which is markup `optionLabel`/`SeatRow` are supposed \
             to produce — a test asserting on it would pass on the probe's own text rather \
             than on what the component rendered"
        );
    }
}

/// **The declaration this file reads is still there.**
#[test]
fn the_declarations_this_file_reads_still_exist() {
    for rel in [COMPONENT, PROBE] {
        let file = repo_root().join(rel);
        assert!(file.is_file(), "{} is read by this file and is gone", file.display());
    }
    let component = std::fs::read_to_string(repo_root().join(COMPONENT)).expect("component reads");
    assert!(
        component.contains("export function SeatRow("),
        "`SeatRow` is no longer an exported function in {COMPONENT} — the probe imports it by \
         that name"
    );
}
