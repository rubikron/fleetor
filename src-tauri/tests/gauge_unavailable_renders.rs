//! **An unavailable gauge says so, and this test watches it say it** (#48; C63,
//! C12, C24, C65, M22, D-054).
//!
//! The rule predates this arc: an unavailable gauge *says* unavailable, and never
//! shows a number that looks right and is wrong. The CLI honours it —
//! `augment_with_gauge` carries `Option::None` through and the roster prints `—`,
//! pinned by `an_unsampled_pane_renders_an_em_dash_not_a_zero`. The React rail did
//! not: both call sites guarded the gauge on bare truthiness, so an absent reading
//! rendered *nothing*, which is what a healthy idle pane looks like too. Silence is
//! not the word.
//!
//! ## Why this file runs the component instead of reading it
//!
//! **C63 is the whole design constraint here.** A source-reading tripwire proves the
//! code *says* the right thing and never that it runs: #35 shipped a gate screen that
//! could not load behind a passing fifteen-check tripwire with a working negative
//! control, and #36 found it only by trying to build on top. This ticket is exactly
//! that shape — a renderer whose entire job is to not be silent — so a `contains()`
//! over `PaneGauge.tsx` would be the same mistake with a different subject.
//!
//! So the assertions below are made against **markup React actually produced**.
//! `tests/gauge_probe/render.tsx` imports the real `PaneGauge`, renders it once per
//! reading through `react-dom/server`, and prints the results as JSON; this file
//! bundles that probe with the esbuild already in the repo's `node_modules` (vite's
//! own), runs it under `node`, and asserts on what came back. If `gaugeView` returns
//! `null` for `unavailable` again, or the component stops emitting the word, these
//! tests go red — which was verified by mutation, not assumed (see the note on
//! [`the_unavailable_reading_renders_the_word`]).
//!
//! **This does not introduce a JavaScript test runner** (C24). There is no vitest,
//! no jest, no `node:test`, no config file and no new dependency: the runner is
//! `cargo test`, and the probe is a subprocess it shells out to — the same shape
//! `tests/vendor_binary_tier.rs` uses for its python probes, and the same loud-skip
//! discipline when the toolchain to run it is absent (D-081).
//!
//! ## What is asserted here, and what is asserted next door
//!
//! The behavioural half is above. The other half is **reachability** — that the two
//! places the rail appears actually go through this component — and that genuinely
//! has no behaviour to run here, because mounting the pane head means mounting an
//! xterm against a pty. It is a source read, and it is labelled as one:
//! [`both_places_the_rail_appears_render_through_the_one_component`].
//!
//! **What no test in this file can prove:** that the pane head is on screen, that its
//! colours are legible, or that an operator noticed. The first is #36's lesson and
//! stays outside a suite with no runtime; the rest were never a test's job.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The renderer under test, and the two call sites that must reach it.
const COMPONENT: &str = "ui/src/components/PaneGauge.tsx";
const PANE_HEAD: &str = "ui/src/components/TerminalPane.tsx";
const TAB_STRIP: &str = "ui/src/components/TerminalGrid.tsx";
const STYLES: &str = "ui/src/styles.css";
/// The probe this file drives. Its own doc comment explains why it asserts nothing.
const PROBE: &str = "src-tauri/tests/gauge_probe/render.tsx";
/// The bundler, taken from the repo's existing `node_modules` — vite's own
/// dependency, not a new one. Bundling rather than transpiling because the probe
/// imports React and a component out of another directory.
const BUNDLER: &str = "node_modules/.bin/esbuild";
const RUNTIME: &str = "node";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("src-tauri has a parent").to_path_buf()
}

fn read(rel: &str) -> String {
    let file = repo_root().join(rel);
    std::fs::read_to_string(&file)
        .unwrap_or_else(|e| panic!("{} must exist to be checked: {e}", file.display()))
}

/// Say it on fd 2, around cargo's capture, so a skip reaches the operator's
/// terminal during an ordinary `cargo test` instead of dying inside a passing
/// test's transcript. Lifted from `tests/vendor_binary_tier.rs`, deliberately: a
/// tier that skips quietly rots into decoration (`building.md` §6, D-081).
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
/// **A missing probe or a missing component is a failure, never a skip.** Those are
/// a deleted test and a deleted feature; only an absent *toolchain* — no node, no
/// installed `node_modules` — is a machine that cannot run the tier.
fn rendered() -> Option<serde_json::Value> {
    let root = repo_root();

    for required in [PROBE, COMPONENT] {
        assert!(
            root.join(required).is_file(),
            "{required} is part of this test. It is missing, which is a deleted renderer or a \
             deleted probe rather than an absent toolchain (#48, C63). Restore it; do not \
             delete this test."
        );
    }

    let bundler = root.join(BUNDLER);
    if !bundler.is_file() {
        announce(&[
            format!("SKIPPED: the gauge-render tier — {BUNDLER} is not installed."),
            "  Run `npm install`. Nothing below was measured: that an unavailable".into(),
            "  reading renders the word rather than nothing, and that it differs".into(),
            "  from a not-yet-sampled pane (#48).".into(),
            "  This machine is running a strictly weaker suite.".into(),
        ]);
        return None;
    }

    // CARGO_TARGET_TMPDIR is cargo's own per-integration-test scratch, so nothing
    // is written into the source tree and two test binaries cannot collide.
    let bundle = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("gauge_probe.cjs");
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
                format!("SKIPPED: the gauge-render tier — could not run {BUNDLER}: {e}."),
                "  Nothing about the unavailable rail was measured (#48).".into(),
            ]);
            return None;
        }
    };
    // Past the toolchain check, a bundle that will not build is the component
    // failing to compile — a red test, not a machine without node.
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
                format!("SKIPPED: the gauge-render tier — no `{RUNTIME}` on PATH ({e})."),
                "  The rail was not rendered, so nothing about what it says when a".into(),
                "  reading is unavailable was measured (#48).".into(),
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

/// The rendering with its HTML character references resolved away, which is what
/// the operator actually reads.
///
/// **Not cosmetic.** React escapes the apostrophe in "this pane's usage" as
/// `&#x27;`, and a digit scan over raw markup counts the `2` and the `7` in that
/// escape as a displayed figure. Stripping the references is the difference between
/// asserting on what is shown and asserting on how it is encoded — and getting this
/// wrong in the other direction (skipping the scan) is how a `0%` would slip past.
fn as_read(markup: &str) -> String {
    let mut out = String::with_capacity(markup.len());
    let mut rest = markup;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        match rest[amp..].find(';') {
            // A reference stands for exactly one character, none of which this
            // rail displays as a digit; the placeholder keeps neighbouring text
            // from being joined into a word that was never rendered.
            Some(end) if end <= 8 => {
                out.push('\u{fffd}');
                rest = &rest[amp + end + 1..];
            }
            _ => {
                out.push('&');
                rest = &rest[amp + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The text **on screen**: what is between the tags, without the classes and
/// without the hover.
///
/// **This distinction was not free.** Asserting on whole markup, a rendering whose
/// visible label was replaced by the pending ellipsis still "contained" the word
/// `unavailable` — in its `title` — and the test went on passing. A tooltip is not
/// the rail saying something; it is the rail saying something to whoever already
/// suspected. Found by mutating the label and watching a green suite (#48, C63).
fn visible(markup: &str) -> String {
    let Some(open) = markup.find('>') else { return String::new() };
    let rest = &markup[open + 1..];
    let end = rest.rfind('<').unwrap_or(rest.len());
    as_read(&rest[..end])
}

/// One rendering, by the key the probe printed it under.
fn markup(all: &serde_json::Value, key: &str) -> String {
    all.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("{PROBE} printed no `{key}`: {all}"))
        .to_string()
}

/// **The unavailable reading renders the word** (#48, acceptance criterion 1).
///
/// Not "renders something", which a stray space satisfies: the rendered text is
/// asserted to contain `unavailable`, and to be a real element rather than an empty
/// string. This is the assertion the whole ticket exists for.
///
/// **Verified by mutation, not assumed.** Making `gaugeView` return `null` for the
/// `unavailable` arm — precisely the state this rail was in before #48 — turns this
/// test red on the empty-markup assertion below, and restoring it turns it green.
/// That is the negative control C63 asks for, and it is the one #35's tripwire had
/// on paper and not in the thing that mattered.
#[test]
fn the_unavailable_reading_renders_the_word() {
    let Some(all) = rendered() else { return };
    let pane = markup(&all, "pane_unavailable");

    assert!(
        !pane.trim().is_empty(),
        "an unavailable reading rendered nothing. Silence is indistinguishable from a pane \
         with no gauge and from one nobody has sampled — which is the bug #48 exists to \
         close, not a lesser form of honesty."
    );
    assert!(
        visible(&pane).contains("unavailable"),
        "the rail must say the word C12 and M22 chose where the operator can read it. A `title` \
         carrying it is a tooltip for someone who already suspected: {pane}"
    );
    // The same word in the tab strip, from the same component: a worker's usage is
    // readable from the tab strip without switching to it, and that half must not
    // be able to disagree with the pane head about whether it could be read.
    let tab = markup(&all, "tab_unavailable");
    assert!(
        visible(&tab).contains("unavailable") && tab.contains("tab__gauge"),
        "the tab strip renders the same reading through the same component: {tab}"
    );
}

/// **Nothing is synthesized for a reading that has none** (#48, acceptance
/// criterion 3; C24, C65, M22).
///
/// The failure this guards is not "the wrong number" but "any number at all": a
/// `0%`, a `—` with a figure beside it, a percentage reconstructed from what
/// FLEETOR sent. So the assertion is that the entire rendering — text *and* hover —
/// carries no digit and no percent sign.
#[test]
fn an_unavailable_rail_carries_no_figure_at_all() {
    let Some(all) = rendered() else { return };

    for key in ["pane_unavailable", "tab_unavailable", "pane_pending"] {
        let m = as_read(&markup(&all, key));
        assert!(
            !m.contains('%'),
            "`{key}` shows a percent for a reading that has none: {m}"
        );
        assert!(
            !m.chars().any(|c| c.is_ascii_digit()),
            "`{key}` shows a figure for a reading that has none. A number that looks right and \
             is wrong is the outcome the whole rule exists to prevent (D-054): {m}"
        );
    }
}

/// **Three states, three distinct renderings** (#48, acceptance criterion 2).
///
/// `unavailable` has to differ from a sampled gauge *and* from a pane nothing has
/// read yet. Pairwise inequality is the assertion, because that is the property —
/// an operator who cannot tell two states apart has one state.
#[test]
fn unavailable_sampled_and_not_yet_sampled_are_three_different_things() {
    let Some(all) = rendered() else { return };
    // The *visible* text, deliberately: two states that differ only by a class
    // name or a hover are two states the operator cannot tell apart, which is one
    // state with extra markup.
    let sampled = visible(&markup(&all, "pane_sampled"));
    let unavailable = visible(&markup(&all, "pane_unavailable"));
    let pending = visible(&markup(&all, "pane_pending"));

    assert_ne!(sampled, unavailable, "a read pane and an unreadable one look the same");
    assert_ne!(unavailable, pending, "an unreadable pane and an unsampled one look the same");
    assert_ne!(sampled, pending, "a read pane and an unsampled one look the same");

    // And each says its own thing rather than merely differing by a class name.
    assert!(sampled.contains("≈42%"), "the sampled rail shows the figure it was given: {sampled}");
    assert!(
        !pending.contains("unavailable"),
        "a pane nobody has read yet must not claim its usage could not be read: {pending}"
    );
    assert!(
        !pending.trim().is_empty(),
        "a not-yet-sampled pane says so too — it was the other half of the silence: {pending}"
    );
}

/// **A call site that was told nothing still says something** (#48).
///
/// `undefined` was the shape the bug arrived in: every consumer held an optional and
/// each decided for itself that absence meant render nothing. It now resolves to the
/// same `pending` rendering rather than to silence, in the component, so a future
/// call site cannot re-open the hole by forgetting to pass a reading.
#[test]
fn a_reading_nobody_supplied_renders_as_not_yet_sampled() {
    let Some(all) = rendered() else { return };
    assert_eq!(
        markup(&all, "pane_undefined"),
        markup(&all, "pane_pending"),
        "an absent prop must resolve to `pending` inside the component, not to nothing"
    );
}

/// **The one pane that is allowed to be silent, and only it** (#48).
///
/// The orchestrator's transcript is the operator's own and has no gauge by
/// construction (WP-04), so `out-of-scope` renders nothing. That is the single
/// exemption, and pinning it here is what stops "renders nothing" from creeping
/// back as a general answer.
#[test]
fn only_the_orchestrator_renders_nothing() {
    let Some(all) = rendered() else { return };
    assert_eq!(
        markup(&all, "pane_out_of_scope"),
        "",
        "a pane with no gauge to be unavailable renders nothing — deliberately, and alone"
    );
}

/// **Both places the rail appears render through the one component** (#48).
///
/// **This is a source read and is labelled as one.** It is the reachability half —
/// that the renderer the tests above exercise is the renderer the app mounts — and
/// it has no behaviour to run in this suite, because mounting a pane head means
/// mounting an xterm against a pty. It cannot tell you the pane head is on screen;
/// it can tell you neither call site went back to its own truthiness guard, which is
/// how both of them came to be silent in the first place.
#[test]
fn both_places_the_rail_appears_render_through_the_one_component() {
    for site in [PANE_HEAD, TAB_STRIP] {
        let src = read(site);
        assert!(
            src.contains("<PaneGauge"),
            "{site} must render the rail through {COMPONENT}, so the two places can never \
             disagree about a pane whose usage could not be read"
        );
        assert!(
            !src.contains("{gauge && ("),
            "{site} has gone back to guarding the gauge on bare truthiness. That guard is the \
             #48 bug itself: it renders an unreadable pane and an unsampled pane identically, \
             as nothing."
        );
    }

    // A tone the stylesheet never heard of renders as unstyled text — visible, so
    // not the silent failure, but not the distinct one either.
    let css = read(STYLES);
    for rule in [".pane__gauge--absent", ".tab__gauge--absent"] {
        assert!(css.contains(rule), "{STYLES} must style `{rule}`, or the two states look alike");
    }
}

/// **The probe cannot fake a pass** (#48, C63).
///
/// The assertions above look for strings in markup, and the cheapest way for that to
/// become vacuous is for the strings to originate in the probe rather than in the
/// component. So: the probe's source must not contain the rendered text it is
/// checked for. It passes readings — `{ kind: "unavailable" }` — and the words come
/// out of `gaugeView` or they do not come out at all.
#[test]
fn the_probe_supplies_readings_and_never_the_words() {
    let probe = read(PROBE);
    for phrase in ["ctx unavailable", "could not be read", "not sampled yet", "≈42%"] {
        assert!(
            !probe.contains(phrase),
            "{PROBE} contains `{phrase}`, so a test asserting on it would pass on the probe's \
             own text rather than on what the component rendered"
        );
    }
}
