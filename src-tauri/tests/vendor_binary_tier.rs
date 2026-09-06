//! **The vendor-binary tier** (WP-25 phase 1, issue #17; C13, C20).
//!
//! The conformance suite has two tiers already: artifact assertions below this
//! one (`tests/harness_conformance_*.rs`, `tests/placement.rs` — what `place`
//! wrote and what it will run) and recorded prototypes above it. This file is
//! the third, and it sits between them: it runs the **real vendor binary** and
//! spends **zero tokens**.
//!
//! It exists because the brief carrier and the socket lever are precisely the
//! checkpoints that **fail by looking healthy**. An artifact assertion proves
//! that FLEETOR wrote a config key; nothing it can observe proves the vendor
//! honoured it. Reading the literal request body off the wire proves it, and
//! `examples/codex-spike/probe.py` already does exactly that — eight arms in one
//! command, exit code equal to the number of failed arms, against a local
//! capture server that answers every request with `data: [DONE]` so nothing ever
//! reaches a real endpoint. That probe is suite infrastructure per C13, not a
//! throwaway. This file wires it in and gates on it; it does not reimplement it.
//!
//! ## The skip discipline is the whole ballgame
//!
//! C13 states the cost outright: a machine without the vendor binary runs a
//! strictly weaker suite. That is accepted, and it is why the skip must be
//! **loud** — a tier that skips quietly rots into decoration (`building.md` §6).
//!
//! Loud is harder than it sounds. `libtest` captures `println!` *and*
//! `eprintln!` on a passing test, so the obvious announcement is invisible in a
//! plain `cargo test` run. [`announce`] therefore writes to file descriptor 2
//! directly, which the capture does not intercept — verified, not assumed. The
//! banner shows up in the middle of the suite's own output with no
//! `--nocapture`.
//!
//! Three things are deliberately **not** skips, because none of them is "the
//! vendor binary is absent":
//!
//! - a missing probe — that is a *deleted tier*, and it fails;
//! - a probe that ran and printed no arms — that is a silent pass, and it fails;
//! - a probe that printed its own `SKIP:` while this file believed the binary
//!   was present — the two disagree about the machine, and it fails.
//!
//! ## What this tier is not
//!
//! **It registers no harness.** Phase 1's exit condition is a green suite with
//! exactly one registered harness, and this tier does not add a second. The
//! vendor binary is named here as the subject of a measurement, the same way
//! `tests/write_guardrail.rs` names `sh`. Nothing in `src/` learns the name.
//!
//! **It changes no production code**, adds no crate and adds no entry point.
//!
//! ## Cost, stated (D-081)
//!
//! On a machine with the vendor binary this file adds roughly **47 s** to
//! `cargo test`: two non-interactive turns, three sandboxed commands and two
//! real pty sessions, all bounded by the probe's own per-subprocess timeouts.
//! It runs by default rather than behind an opt-in flag, because an opt-in flag
//! is a quiet skip with extra steps.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The vendor binary the probe drives.
///
/// This names the subject of a measurement, not a registered harness — see the
/// header. `grep -rn "codex" src-tauri/src/` is expected to find nothing.
const VENDOR_BIN: &str = "codex";

/// The interpreter the probe is written in.
const INTERPRETER: &str = "python3";

/// The probe, relative to the repository root. One command, eight arms.
const PROBE: &str = "examples/codex-spike/probe.py";

/// The vendor build every arm was recorded against, spelled the way both the
/// probe and the spike notes spell it.
const RECORDED_BUILD: &str = "codex-cli 0.153.4";

/// The notes that carry the measurements, version-stamped to [`RECORDED_BUILD`].
const NOTES: &str = "docs/notes/codex-spike-notes.md";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("src-tauri has a parent").to_path_buf()
}

/// Say something the test harness cannot swallow.
///
/// `libtest` captures the print macros on a passing test, which is exactly the
/// condition a skip is in. Writing to fd 2 directly goes around the capture, so
/// the banner reaches the operator's terminal during an ordinary `cargo test`.
/// The descriptor is borrowed, never owned: closing it would take stderr with
/// it for the rest of the binary.
fn announce(lines: &[String]) {
    let banner = format!(
        "\n\
         ==============================================================================\n\
         {}\n\
         ==============================================================================\n",
        lines.join("\n")
    );
    #[cfg(unix)]
    {
        use std::os::unix::io::FromRawFd;
        let mut stderr = std::mem::ManuallyDrop::new(unsafe { fs::File::from_raw_fd(2) });
        let _ = stderr.write_all(banner.as_bytes());
        let _ = stderr.flush();
    }
    #[cfg(not(unix))]
    {
        // No fd trick off unix; the print macro at least reaches `--nocapture`.
        eprint!("{banner}");
    }
}

/// Resolve a program on `PATH` the way a shell would: first executable file
/// wins. No crate for this — it is eight lines and this file has no deps.
fn on_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|dir| dir.join(program)).find(|candidate| {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::metadata(candidate)
                .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        }
        #[cfg(not(unix))]
        {
            candidate.is_file()
        }
    })
}

// --- the tier ---------------------------------------------------------------

/// **The gate.** Run the probe against the real vendor binary, or announce
/// loudly that this machine is running the weaker suite.
///
/// The probe's exit code *is* the number of failed arms, so this asserts on it
/// directly rather than parsing its output for a verdict. The output is parsed
/// for one thing only: evidence that arms actually ran, so a probe that silently
/// did nothing cannot pass as a green tier.
#[test]
fn the_vendor_binary_tier_runs_the_probe_or_announces_its_absence() {
    let root = repo_root();
    let probe = root.join(PROBE);

    // A missing probe is a deleted tier, not an absent vendor. C13 makes the
    // probe suite infrastructure; losing it must be as loud as any other test
    // failure, and it must not wear a skip's clothes.
    assert!(
        probe.is_file(),
        "{PROBE} is the vendor-binary tier. It is missing, which is a deleted tier rather \
         than an absent vendor binary (C13). Restore it; do not delete this test."
    );

    let Some(interpreter) = on_path(INTERPRETER) else {
        announce(&[
            format!("SKIPPED: the vendor-binary tier — no {INTERPRETER} on PATH."),
            format!("  {PROBE} cannot run, so no vendor behaviour was measured."),
            "  This machine is running a strictly weaker conformance suite (C13).".into(),
        ]);
        return;
    };

    let Some(vendor) = on_path(VENDOR_BIN) else {
        announce(&[
            format!("SKIPPED: the vendor-binary tier — `{VENDOR_BIN}` is not on PATH."),
            format!("  Recorded against {RECORDED_BUILD}. Nothing below was measured:"),
            "    brief-replaces, brief-rejected, agents-md, fence,".into(),
            "    socket-refused-by-default, socket-lifted-by-network-access,".into(),
            "    typing-fast-does-not-submit, typing-tuned-submits".into(),
            "  The artifact tier still proves FLEETOR wrote its keys. It cannot prove".into(),
            "  the vendor honoured them — that is what these arms are for.".into(),
            "  This machine is running a strictly weaker conformance suite (C13).".into(),
        ]);
        return;
    };

    let out = Command::new(&interpreter)
        .arg(&probe)
        .current_dir(&root)
        .output()
        .unwrap_or_else(|e| panic!("could not run {}: {e}", probe.display()));

    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let transcript = format!("--- probe stdout ---\n{stdout}\n--- probe stderr ---\n{stderr}");

    // The probe skips on the same condition this file just ruled out. If it
    // skipped anyway, the two disagree about the machine — which is the one
    // way a skip could become silent, so it is a failure rather than a skip.
    assert!(
        !stdout.contains("SKIP:"),
        "the probe skipped although `{VENDOR_BIN}` resolved to {}. The tier and the probe \
         disagree about this machine, so nothing was measured and nothing announced it.\n{transcript}",
        vendor.display()
    );

    // No silent pass: a probe that printed no arms exits 0 too.
    let arms = stdout.matches("  PASS  ").count() + stdout.matches("  FAIL  ").count();
    assert!(
        arms > 0,
        "the probe exited without reporting a single arm. Exit 0 with nothing measured is \
         the failure this tier was built to make impossible.\n{transcript}"
    );
    assert!(
        stdout.contains(" failed"),
        "the probe did not reach its own verdict line, so its exit code is not a count of \
         failed arms and gating on it would be guesswork.\n{transcript}"
    );

    // The exit code is the number of failed arms — gate on it.
    assert_eq!(
        out.status.code(),
        Some(0),
        "the vendor-binary tier is red: {arms} arms ran and the probe reports failures. \
         A failing arm is either a regression in what FLEETOR writes or drift in the vendor \
         build — the probe says which. Re-record rather than patching around drift.\n{transcript}"
    );

    // Captured unless `--nocapture`, which is fine: a green tier is an ordinary
    // passing test. Only the skip has to shout.
    println!("{stdout}");
}

/// **The probe is committed infrastructure, and it is version-stamped.**
///
/// C13's tier is only as honest as the build id it was recorded against, and the
/// roadmap's exit checklist requires the notes to carry that id too. If the two
/// spellings drift apart, a reader cannot tell which measurement a failing arm
/// belongs to. This pins them to each other rather than to a hard-coded date.
#[test]
fn the_probe_is_committed_infrastructure_stamped_with_its_vendor_build() {
    let root = repo_root();
    let src = fs::read_to_string(root.join(PROBE)).expect("the probe is committed, not pasted");

    assert!(
        src.contains(&format!("BUILD = \"{RECORDED_BUILD}\"")),
        "{PROBE} must name the vendor build it was recorded against, as `BUILD`"
    );

    let notes = fs::read_to_string(root.join(NOTES)).expect("the spike notes are committed");
    assert!(
        notes.contains(RECORDED_BUILD),
        "{NOTES} must carry the same vendor build id as {PROBE} ({RECORDED_BUILD}), or a \
         failing arm cannot be traced to the measurement it contradicts"
    );
}

/// **The tier can never reach a real model endpoint** — the zero-token property,
/// as a property of the probe's source rather than a promise about it.
///
/// The whole tier rests on the model provider being a local capture server. One
/// edit pointing it somewhere else would turn a suite run into recurring spend,
/// silently and on every machine. So: every URL in the probe is loopback.
///
/// This is strict on purpose. A documentation link in a comment will trip it —
/// write the link in prose without a scheme, or put it in the notes instead.
#[test]
fn the_probe_addresses_nothing_but_loopback() {
    let root = repo_root();
    let src = fs::read_to_string(root.join(PROBE)).expect("the probe is committed, not pasted");

    let offenders: Vec<&str> = src
        .split("://")
        .skip(1)
        .filter(|rest| !rest.starts_with("127.0.0.1"))
        .map(|rest| rest.split_whitespace().next().unwrap_or(rest))
        .collect();

    assert!(
        offenders.is_empty(),
        "{PROBE} must address nothing but 127.0.0.1 — the zero-token property of this whole \
         tier (C13) is that the model provider is a local capture server. Found: {offenders:?}"
    );
    assert!(
        src.contains("127.0.0.1"),
        "{PROBE} no longer stands up a loopback capture server, so nothing guarantees a run \
         of this tier spends nothing"
    );
}
