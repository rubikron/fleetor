//! **A codex pane is not announced until it can receive** (WP-25 phase 2, #42;
//! C6, C22, C26, C37, D-034; Tier 1.4, Tier 1.5).
//!
//! `codex` opens on an animated splash that ends on a keypress rather than on a
//! timer — #24 left one animating for 75 s — and it **discards** what is written
//! to it until that keypress. Every codex pane gets a freshly seeded `CODEX_HOME`
//! under C6, so a first message delivered into that window is silently gone while
//! the pane looks perfectly healthy, and `fleet send` answers `accepted`. That is
//! the lie Tier 1.5 and D-034 exist to prevent.
//!
//! ## Why this file drives `PaneRegistry` rather than a pty of its own
//!
//! #24 measured the splash **inside its own probe**, which forks its own pty,
//! sets its own window size and answers the vendor's terminal queries itself. A
//! finding that only reproduces inside the instrument is a finding about the
//! instrument. So the subject here is `PaneRegistry::spawn` — the same call the
//! shell makes for every pane, through `portable-pty`, with the same pump, the
//! same coalescer and the same writer lock — and the message is delivered with
//! `PaneRegistry::write_paste`, which is the exact call `deliver` makes when a
//! `fleet send` arrives. **Nothing here reimplements the path it is measuring.**
//!
//! It reproduced. The splash is not an artifact of the probe, and it is not an
//! artifact of an *empty* `CODEX_HOME` either: a directory seeded by codex's own
//! `seed_config_dir` animates identically.
//!
//! ## The control is the whole measurement
//!
//! The fix arm alone proves nothing. A message that arrives on the wire in a
//! woken pane would read exactly the same as one that would have arrived anyway,
//! and the arm would pass against a `wake` that returned immediately. So the same
//! pane is spawned a second time under a spec that differs in **one field** —
//! [`BringUp::AtOnce`] instead of [`BringUp::AfterWaking`] — and the same message
//! is delivered at the same moment. It must be swallowed. That is the bug, run on
//! purpose, and it is what makes the other arm mean something.
//!
//! ## Zero tokens
//!
//! Both arms answer a fabricated provider on loopback with `data: [DONE]`,
//! the instrument C13 established and `probe.py` already uses. No completion is
//! ever requested of a real endpoint, and every assertion holds on a machine with
//! no network. #38 is the only ticket authorized to spend money.
//!
//! ## Cost, stated (D-081)
//!
//! About 40 s on a machine with `codex` installed: two real interactive panes,
//! one of which is deliberately allowed to fail. The skip is loud for C13's
//! reason — a tier that skips quietly rots into decoration.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use fleetor_core::pane::PaneId;
use fleetor_shell::placement::codex::CODEX_SPEC;
use fleetor_shell::placement::harness::BringUp;
use fleetor_shell::placement::HarnessSpec;
use fleetor_shell::pty::PaneRegistry;
use portable_pty::CommandBuilder;

/// The pane, the capture server and the paint reader are
/// `tests/codex_pane/mod.rs`'s — shared with `tests/codex_typing.rs` (#32) so
/// one file's control cannot vouch for the other file's arm through a second,
/// drifted copy of the instrument.
mod codex_pane;
use codex_pane::{
    announce, on_path, scratch, CodexPane, Painted, RECORDED_BUILD, VENDOR_BIN,
};

/// How long a delivered message is given to reach the wire.
///
/// **A budget for the measurement, not a timeout on anything the product does.**
/// Nothing in `src/` waits on this; it is how long this test is willing to sit
/// before it concludes a message never arrived, and the swallowed arm is the one
/// that spends it in full.
const ON_THE_WIRE: Duration = Duration::from_secs(25);

/// The same spec codex ships with, one field changed: this pane is announced the
/// moment it has a pty, the way every pane was before #42.
///
/// **Built from `..CODEX_SPEC` rather than written out**, so the control cannot
/// drift into being a different harness. If it ever differs in a second field,
/// the arm below stops being a control and nobody would be told.
static ANNOUNCED_AT_ONCE: HarnessSpec = HarnessSpec { bring_up: BringUp::AtOnce, ..CODEX_SPEC };

// --- the arms -----------------------------------------------------------------

/// One pane, brought up under `spec`, sent `message` the instant `spawn` returns.
///
/// Returns whether the message reached the wire, and what the pane painted.
fn deliver_into_a_fresh_pane(
    vendor: &std::path::Path,
    spec: &'static HarnessSpec,
    message: &str,
) -> (bool, Arc<Painted>) {
    let tag = match spec.bring_up {
        BringUp::AfterWaking => "woken",
        BringUp::AtOnce => "control",
    };
    let pane = CodexPane::brought_up(vendor, spec, tag);

    // **The instant it is announced.** No settling, no sleep, no readiness check
    // of its own — this is what `deliver` does the moment a `fleet send` lands,
    // and the whole claim is that the pane can take it.
    pane.deliver(message).expect("the delivery");

    let arrived = pane.reached_the_wire(message, ON_THE_WIRE);
    (arrived, Arc::clone(&pane.painted))
}
/// **The ticket, measured against the registry the shell actually uses** (#42).
///
/// A message delivered the instant `spawn` returns reaches the model. Run against
/// a `CODEX_HOME` that codex's own seeder just wrote, which is the case that
/// fails.
#[test]
fn a_message_delivered_the_instant_a_codex_pane_is_announced_is_submitted() {
    let Some(vendor) = on_path(VENDOR_BIN) else {
        announce(&[
            format!("SKIPPED: codex bring-up (#42) — `{VENDOR_BIN}` is not on PATH."),
            format!("  Recorded against {RECORDED_BUILD}. Nothing below was measured:"),
            "    a first message survives the splash a fresh CODEX_HOME opens on".into(),
            "    the same message is swallowed when the pane is announced at once".into(),
            "  The in-crate tests still prove the registry holds an unwoken pane out of".into(),
            "  the map delivery reads. They cannot prove the vendor's splash is what".into(),
            "  swallows a message, and a swallowed message looks exactly like a".into(),
            "  delivered one from this side of the pty.".into(),
        ]);
        return;
    };

    let woken = "BRINGUP-SENTINEL-WOKEN";
    let (arrived, painted) = deliver_into_a_fresh_pane(&vendor, &CODEX_SPEC, woken);
    let screen = painted.screen();

    assert!(
        arrived,
        "a message delivered the instant the pane was announced never reached the model. \
         The pane was woken before `spawn` returned, so either the wake stopped dismissing \
         the splash or it is now returning before the pane can receive — which is the \
         silent-loss failure #42 exists to close, back again. The pane painted {} bytes.",
        painted.painted(),
    );
    assert!(
        !screen.to_lowercase().contains("trustthecontents"),
        "the pane put up the trust dialog, so the keypress that woke it may have been \
         answering *that* rather than dismissing a splash — and this arm would pass for \
         the wrong reason. Checkpoint 14 seeds the trust record precisely so a pane never \
         meets this question (C6, C17)."
    );
}

/// **The control: the same pane, announced at once, swallows the same message.**
///
/// Without this the arm above is vacuous — it would pass against a `wake` that
/// did nothing at all. One field differs between the two specs.
#[test]
fn the_same_message_is_swallowed_when_the_pane_is_announced_before_it_is_woken() {
    let Some(vendor) = on_path(VENDOR_BIN) else {
        return; // the arm above owns the announcement; two banners would be noise
    };

    let control = "BRINGUP-SENTINEL-CONTROL";
    let (arrived, painted) = deliver_into_a_fresh_pane(&vendor, &ANNOUNCED_AT_ONCE, control);

    assert!(
        !arrived,
        "the control arm's message reached the model *without* the pane being woken, so \
         the splash is no longer swallowing anything and the other arm proves nothing. \
         Either the vendor changed — re-measure against {RECORDED_BUILD} — or this file \
         has stopped measuring what it claims to."
    );
    assert!(
        painted.painted() > 0,
        "the control pane painted nothing at all, so it never started and the swallow \
         above is an unspawned process rather than a splash. That is not a control."
    );
}

// --- what holds with no vendor binary on the machine ---------------------------

/// **An unwoken pane is not addressable, and a woken one is** — asserted against
/// the registry with `/bin/cat`, so it runs on every machine.
///
/// This is the structural half of #42: `AfterWaking` holds a pane out of the map
/// delivery reads until it settles, and `AtOnce` does not. `cat` paints only what
/// it is sent, so pressing the wake key is what makes it paint and then fall
/// silent — which is exactly the shape the detector is looking for, with no
/// vendor involved.
#[test]
fn a_pane_that_is_announced_at_once_is_addressable_and_the_registry_still_reaps_both() {
    let root = scratch("cat");
    std::fs::create_dir_all(&root).expect("a scratch dir");
    let pane = PaneId::Worker(2);
    let painted = Painted::watching(pane);
    let registry = PaneRegistry::new(painted.emitter(), root.join("panes.pids"));

    let mut cmd = CommandBuilder::new("/bin/cat");
    cmd.env("TERM", "dumb");
    registry.spawn(pane, cmd, &ANNOUNCED_AT_ONCE, 24, 80).expect("spawn");

    assert!(
        registry.write_paste(pane, "addressable").is_ok(),
        "a pane whose harness is ready at once must be writable the moment spawn returns \
         — that is every pane FLEETOR has ever run, and #42 may not change it"
    );
    assert!(registry.any_pane(), "a running pane is a pane");
    registry.kill_all();
    std::fs::remove_dir_all(&root).ok();
}

/// A woken pane is reaped by `kill_all` even though it is not in the map
/// delivery reads.
///
/// A window closed while a pane was coming up would otherwise leave a live agent
/// process with nobody watching it — the money bug `kill_all` exists to prevent.
/// `cat` never settles here: it paints only in answer to the wake key, so the
/// bring-up loop keeps pressing, which is what makes it a pane caught mid-wake.
#[test]
fn a_pane_still_being_woken_is_still_reaped() {
    let root = scratch("reap");
    std::fs::create_dir_all(&root).expect("a scratch dir");
    let pane = PaneId::Worker(3);
    let painted = Painted::watching(pane);
    let registry = Arc::new(PaneRegistry::new(painted.emitter(), root.join("panes.pids")));

    // `yes` never stops painting, so it is never announced — a pane permanently
    // mid-bring-up, which is the state this test needs to exist in.
    let mut cmd = CommandBuilder::new("/usr/bin/yes");
    cmd.env("TERM", "dumb");
    let spawning = Arc::clone(&registry);
    let bringing_up = std::thread::spawn(move || {
        let _ = spawning.spawn(pane, cmd, &CODEX_SPEC, 24, 80);
    });

    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && painted.painted() == 0 {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(painted.painted() > 0, "the pane never painted, so it never started");
    assert!(
        registry.any_pane(),
        "a pane being woken is a pane this session has brought up — the target guard has \
         to see it, because its worktree and its config dir are already committed"
    );
    let roster: Vec<PaneId> = registry.roster().into_iter().map(|e| e.pane).collect();
    assert!(
        !roster.contains(&pane),
        "a pane that cannot yet receive appeared on the fleet's roster, which is the list \
         `fleet broadcast` fans out to — every broadcast would write into a pane that \
         throws the message away"
    );
    assert!(
        registry.write_paste(pane, "too early").is_err(),
        "a pane that has not been woken took a delivery. It must be refused for want of a \
         pane, which the hub answers `accepted: false` — the honest answer a swallowed \
         message never gets (Tier 1.5)"
    );

    registry.kill_all();
    let _ = bringing_up.join();
    std::fs::remove_dir_all(&root).ok();
}

// --- the wake meets a trust gate (#47) ------------------------------------------

/// One fake pane that paints `line` and then sits there, with a `CODEX_HOME`
/// beside it whose only project key covers a directory this pane is not in.
///
/// **A shell rather than the vendor, so both arms below run on every machine.**
/// What is under test is not what codex paints — `trust_probe.py` and
/// `codex_trust_gate.rs` measure that against the real binary — it is what the
/// **wake** does once that text reaches the pty. A pane that paints once and falls
/// silent is the cleanest statement of it: with no gate check it settles in about a
/// second and is announced, so the two arms below differ by one word of paint.
struct BroughtUp {
    registry: PaneRegistry,
    pane: PaneId,
    cwd: std::path::PathBuf,
    /// What `spawn` answered. The `Err` is the refusal under test, and its text is
    /// the sentence `TerminalPane.tsx` writes into the pane itself.
    outcome: Result<(), String>,
}

fn paint_and_bring_up(root: &std::path::Path, line: &str) -> BroughtUp {
    let cwd = root.join("work");
    let home = root.join("codex-home");
    std::fs::create_dir_all(&cwd).expect("a pane cwd");
    std::fs::create_dir_all(&home).expect("a CODEX_HOME");
    std::fs::write(
        home.join("config.toml"),
        "[projects.\"/somewhere/else\"]\ntrust_level = \"trusted\"\n",
    )
    .expect("a seed that covers the wrong directory");

    let pane = PaneId::Worker(1);
    let painted = Painted::watching(pane);
    let registry = PaneRegistry::new(painted.emitter(), root.join("panes.pids"));

    // Painted word by word with a cursor move before each, the way the vendor
    // repaints — so the arm exercises the scanner rather than a substring search,
    // which is exactly what would not have worked against the real dialog.
    let mut cmd = CommandBuilder::new("/bin/sh");
    cmd.arg("-c");
    cmd.arg(format!(
        "printf '\\033[2J\\033[H'; i=10; for w in {line}; do \
         printf '\\033[%d;1H%s ' \"$i\" \"$w\"; i=$((i+1)); done; \
         printf '\\033[30;1H> 1. Yes, continue   2. No, quit'; sleep 30"
    ));
    cmd.cwd(&cwd);
    cmd.env("CODEX_HOME", &home);
    cmd.env("TERM", "xterm-256color");

    let outcome = registry.spawn(pane, cmd, &CODEX_SPEC, 40, 120);
    BroughtUp { registry, pane, cwd, outcome }
}

/// **The ticket** (#47). A pane parked on the trust gate is refused, not answered.
///
/// Measured first, under both binaries, and the reading is why this exists: the
/// wake's second `\r` *did* dismiss the dialog, the pane reached its composer
/// looking healthy, and the vendor persisted `trust_level = "trusted"` for the
/// directory into the pane's own `config.toml`. That is a loud failure — a parked
/// pane, which #25's probe detects in about three seconds — converted into a silent
/// one that also grants trust.
#[test]
fn a_pane_that_paints_the_trust_gate_is_refused_rather_than_answered() {
    let root = scratch("gate");
    let up = paint_and_bring_up(&root, "Do you trust the contents of this directory?");
    let refusal = up.outcome.clone().expect_err(
        "the wake settled a pane that was sitting on its trust dialog. `1. Yes, continue` is \
         pre-selected, so the press that settled it answered the dialog — FLEETOR trusting a \
         directory on the operator's behalf, which the vendor then writes to disk (#47).",
    );

    assert!(
        refusal.contains(&up.cwd.display().to_string()),
        "the refusal has to name the directory that was not trusted — it is the one thing the \
         operator needs in order to fix the seeding: {refusal}",
    );
    assert!(
        refusal.contains("/somewhere/else") && refusal.contains("config.toml"),
        "…and the keys the seed did write, and the file they are in, so there is something to \
         compare that directory against: {refusal}",
    );

    // **Not announced**, which is the other half of the ticket: a refused pane must
    // not become addressable by some other route.
    assert!(
        up.registry.write_paste(up.pane, "too early").is_err(),
        "a pane parked on an unanswered trust dialog took a delivery. It has to be refused for \
         want of a pane, which the hub answers `accepted: false` (Tier 1.5)",
    );
    let roster: Vec<PaneId> = up.registry.roster().into_iter().map(|e| e.pane).collect();
    assert!(!roster.contains(&up.pane), "a refused pane is not on the fleet's roster");

    up.registry.kill_all();
    std::fs::remove_dir_all(&root).ok();
}

/// **The control.** The same fake pane, one word different, is woken and announced.
///
/// Without it the arm above would pass against a `wake` that refused every pane it
/// was ever handed — and a codex pane that can never come up is a fleet with no
/// codex in it.
#[test]
fn a_pane_that_paints_something_that_is_not_the_gate_is_announced() {
    let root = scratch("not-gate");
    let up = paint_and_bring_up(&root, "Do you trust the contents of this repository?");
    up.outcome.clone().expect(
        "one word differs from the gate's own words and this pane was refused anyway, so the \
         detector is matching something looser than the dialog itself",
    );
    assert!(
        up.registry.write_paste(up.pane, "addressable").is_ok(),
        "a pane that painted no gate and then fell silent is a woken pane, and #47 may not \
         change that",
    );
    up.registry.kill_all();
    std::fs::remove_dir_all(&root).ok();
}

/// The registry's own map is the only thing that decides addressability, and a
/// spec's bring-up answer never reaches a delivery.
///
/// A source-reading tripwire, for the same reason `harness_literals.rs` is one: a
/// readiness check inside `writable` would behave identically to holding the pane
/// out of the map, and no test that *runs* the code could tell them apart. This
/// is the Tier 1.4 half of #42.
#[test]
fn nothing_on_the_delivery_path_reads_a_pane_s_bring_up() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/pty.rs"),
    )
    .expect("pty.rs");

    let delivery = ["fn writable", "fn write_paste", "fn write_command", "fn type_framed"];
    let mut offenders: Vec<(&str, HashMap<&str, ()>)> = Vec::new();
    for name in delivery {
        let start = source.find(name).unwrap_or_else(|| panic!("{name} has been renamed"));
        let body = &source[start..];
        // The function's own body: up to whatever comes next at the same
        // indentation — another method, a doc comment, or the end of the `impl`.
        // Every marker, not just `fn`: stopping only at `fn` reads a method's
        // body plus everything after it, which is how this tripwire first fired
        // on a mention three functions away.
        let end = ["\n    fn ", "\n    pub fn ", "\n    ///", "\n}"]
            .iter()
            .filter_map(|marker| body[1..].find(marker).map(|i| i + 1))
            .min()
            .unwrap_or(body.len());
        let body = &body[..end];
        let mut found = HashMap::new();
        // `TRUST_GATE` and `GateScanner` joined the list with #47. The gate check
        // is readiness of exactly the kind this tripwire exists to keep off the
        // message path: a `writable` that refused a pane sitting on a dialog would
        // behave identically to holding it out of the map, and only one of them is
        // legal. It belongs to `wake` and to the pump, and to nothing else.
        for banned in [
            "bring_up",
            "BringUp",
            "waking",
            "painted",
            "sleep(WAKE",
            "QUIET_SAMPLE",
            "TRUST_GATE",
            "GateScanner",
        ] {
            if body.contains(banned) {
                found.insert(banned, ());
            }
        }
        if !found.is_empty() {
            offenders.push((name, found));
        }
    }
    assert!(
        offenders.is_empty(),
        "the delivery path has learned about bring-up: {offenders:?}. Readiness on the way \
         *to* a pty is a Tier 1.4 violation — it can only grow into a wait, a guard or a \
         retry between `fleet send` and a pane, which building.md §9 records as argued and \
         lost twice. Readiness belongs on the bring-up path, before the pane is announced."
    );
}
