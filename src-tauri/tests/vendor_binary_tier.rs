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
//! Since #43 it gates its **sibling** the same way — `probe_clear.py`, five arms,
//! the `/clear` survival measurement C37 recorded and left ungated. Both probes
//! are run in sequence by the one gate below, and both exit codes count.
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
//! On a machine with the vendor binary the two probes add roughly **85 s** to
//! `cargo test` — `probe.py`'s ~46 s (two non-interactive turns, three sandboxed
//! commands, two real pty sessions) plus `probe_clear.py`'s ~39 s (two more pty
//! sessions, each driven through a turn, a `/clear` and another turn), all
//! bounded by the probes' own per-subprocess timeouts. That is up from the ~47 s
//! D-081 stated, and the increase is the whole of #43's second half: the
//! alternative to paying it is a measurement nothing re-checks.
//!
//! It runs by default rather than behind an opt-in flag, because an opt-in flag
//! is a quiet skip with extra steps.
//!
//! ## Concurrency (#43)
//!
//! The probes admit **one run at a time**, on a lock they take themselves, and
//! each run gets its own scratch root and an OS-assigned port. Before that, two
//! simultaneous runs shared a fixed root and a fixed port, and the second wiped
//! the first's installation mid-pty — observed as a red `typing-fast-does-not-submit`
//! that was contention wearing a regression's clothes. A tier whose red means
//! "maybe someone else was running" is worse than one that skips loudly, so the
//! second run now waits, and if it waits out the probe's own limit it prints
//! `BUSY:` and this file announces a skip instead of a failure.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The vendor binary the probe drives.
///
/// This names the subject of a measurement, not a registered harness — see the
/// header. `grep -rn "codex" src-tauri/src/` is expected to find nothing.
const VENDOR_BIN: &str = "codex";

/// **The vendor binary, resolved absolutely** (C47).
///
/// The `codex` on PATH here is a cmux shim that injects flags, and #31 found the
/// case where the two verdicts differ absolutely. So any arm measuring hooks, flags
/// or config precedence takes both readings, and this is the one that can be
/// reasoned about. Absent on a machine installed elsewhere, in which case the PATH
/// reading is the only one and the arm says so.
const VENDOR_ABSOLUTE: &str = "/opt/homebrew/bin/codex";

/// The interpreter the probe is written in.
const INTERPRETER: &str = "python3";

/// The probe, relative to the repository root. One command, eight arms.
const PROBE: &str = "examples/codex-spike/probe.py";

/// **Its sibling**, relative to the repository root. One command, five arms: the
/// `/clear` survival measurement behind C37, gated here since #43.
///
/// C37 landed it runnable but ungated and said so as a stated cost, which is the
/// decoration C13 warns about — an ungated probe stops being re-checkable and
/// becomes folklore dated to one build. It is a sibling rather than a ninth arm
/// of [`PROBE`] because it needs a capture server that numbers every request.
const CLEAR_PROBE: &str = "examples/codex-spike/probe_clear.py";

/// The two probes this tier gates on, in the order it runs them.
const PROBES: [&str; 2] = [PROBE, CLEAR_PROBE];

/// The vendor build every arm was recorded against, spelled the way both the
/// probe and the spike notes spell it.
const RECORDED_BUILD: &str = "codex-cli 0.153.4";

/// The notes that carry the measurements, version-stamped to [`RECORDED_BUILD`].
const NOTES: &str = "docs/notes/codex-spike-notes.md";

/// The notes that carry [`CLEAR_PROBE`]'s measurement (C37), stamped the same way.
const CLEAR_NOTES: &str = "docs/notes/codex-clear-notes.md";

/// This file, relative to the repository root — it reads its own source, because
/// since #27 it stands up a loopback provider of its own.
const THIS_FILE: &str = "src-tauri/tests/vendor_binary_tier.rs";

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

/// **The binary an arm runs when it wants one reading** (C47, #43).
///
/// Not the `codex` on PATH, which on this machine is the cmux shim. Two reasons,
/// and the second is why this landed with the concurrency work:
///
///  - the shim injects six `-c hooks.*` flags and a hook-trust bypass, so an arm
///    that runs it measures the vendor **plus six injected hooks** — a confound
///    in every arm that did not ask for one, and it fires the operator's desktop
///    notifications on every pane a probe opens;
///  - its path is a per-session temporary under `TMPDIR`, and it can be swept
///    while a run is in flight. That was observed here as
///    `could not run …/cmux-cli-shims/…/codex: No such file or directory` —
///    a red arm with the vendor's name on it and nothing to do with the vendor.
///
/// The two arms that take **both** readings on purpose — the write-guardrail arm
/// and the gate probe — resolve their own binaries and do not call this.
fn the_vendor_binary() -> Option<PathBuf> {
    let absolute = Path::new(VENDOR_ABSOLUTE);
    if absolute.is_file() {
        return Some(absolute.to_path_buf());
    }
    on_path(VENDOR_BIN)
}

/// A scratch name no concurrent run can also choose, and no longer than that.
///
/// **Unique**, because the wall clock alone is not: two runs of this tier started
/// together take the same millisecond, land on the same root, and the second
/// one's `fleet.sock` fails to bind with `AddrInUse` — contention wearing a
/// regression's clothes, which is the whole of #43. The pid is what distinguishes
/// them; the clock distinguishes two runs from the same shell.
///
/// **Short**, because one of these roots holds an `AF_UNIX` socket and those
/// paths cap at `SUN_LEN` — about 104 bytes, of which macOS's `TMPDIR` already
/// spends 49. Spelling the clock in full nanoseconds fits the budget on Linux and
/// blows it here, which is `path must be shorter than SUN_LEN` rather than
/// anything about the vendor. The low digits carry all the uniqueness two runs
/// need, so the name stays around 30 bytes.
fn scratch_name(what: &str) -> String {
    let ticks = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("a clock")
        .as_nanos()
        % 100_000_000;
    format!("fleetor-codex-{what}-{}-{ticks}", std::process::id())
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
fn the_vendor_binary_tier_runs_the_probes_or_announces_their_absence() {
    let root = repo_root();

    // A missing probe is a deleted tier, not an absent vendor. C13 makes the
    // probes suite infrastructure; losing one must be as loud as any other test
    // failure, and it must not wear a skip's clothes.
    for probe_rel in PROBES {
        assert!(
            root.join(probe_rel).is_file(),
            "{probe_rel} is part of the vendor-binary tier. It is missing, which is a deleted \
             tier rather than an absent vendor binary (C13). Restore it; do not delete this test."
        );
    }

    let Some(interpreter) = on_path(INTERPRETER) else {
        announce(&[
            format!("SKIPPED: the vendor-binary tier — no {INTERPRETER} on PATH."),
            format!("  {PROBE} and {CLEAR_PROBE} cannot run, so no vendor behaviour"),
            "  was measured.".into(),
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
            "    typing-fast-does-not-submit, typing-tuned-submits,".into(),
            "    clear-brief-before, clear-actually-cleared, clear-brief-survives,".into(),
            "    clear-brief-identical, clear-brief-survives-via-flag".into(),
            "  The artifact tier still proves FLEETOR wrote its keys. It cannot prove".into(),
            "  the vendor honoured them — that is what these arms are for.".into(),
            "  This machine is running a strictly weaker conformance suite (C13).".into(),
        ]);
        return;
    };

    // Sequentially, and deliberately: both probes drive a real pty and both take
    // the probes' own one-run-at-a-time lock, so running them concurrently would
    // buy nothing and cost the timing the pty arms measure.
    for probe_rel in PROBES {
        gate_on(&interpreter, &root, probe_rel, &vendor);
    }
}

/// Run one probe and gate on its exit code, or announce loudly why it measured
/// nothing.
///
/// Every outcome is a passing assertion, a panic carrying the probe's whole
/// transcript, or a banner on fd 2. The one outcome that is neither pass nor
/// fail is `BUSY:` — another run held the probes' lock for the whole wait, which
/// is a machine that was never quiet rather than a vendor that changed (#43).
fn gate_on(interpreter: &Path, root: &Path, probe_rel: &str, vendor: &Path) {
    let probe = root.join(probe_rel);
    let out = Command::new(interpreter)
        .arg(&probe)
        .current_dir(root)
        .output()
        .unwrap_or_else(|e| panic!("could not run {}: {e}", probe.display()));

    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let transcript = format!("--- probe stdout ---\n{stdout}\n--- probe stderr ---\n{stderr}");

    // Contention, announced as contention. The alternative this replaces is the
    // expensive one: a red arm that means "someone else was running", which
    // spends an operator's afternoon on a vendor change that never happened.
    if stdout.contains("BUSY:") {
        announce(&[
            format!("SKIPPED: {probe_rel} — another codex-spike run held the lock throughout."),
            "  Nothing was measured, and nothing here is evidence of a regression.".into(),
            "  Re-run this tier on a quiet machine before trusting its silence (C13).".into(),
        ]);
        return;
    }

    // The probe skips on the same condition this file just ruled out. If it
    // skipped anyway, the two disagree about the machine — which is the one
    // way a skip could become silent, so it is a failure rather than a skip.
    assert!(
        !stdout.contains("SKIP:"),
        "{probe_rel} skipped although `{VENDOR_BIN}` resolved to {}. The tier and the probe \
         disagree about this machine, so nothing was measured and nothing announced it.\n{transcript}",
        vendor.display()
    );

    // No silent pass: a probe that printed no arms exits 0 too.
    let arms = stdout.matches("  PASS  ").count() + stdout.matches("  FAIL  ").count();
    assert!(
        arms > 0,
        "{probe_rel} exited without reporting a single arm. Exit 0 with nothing measured is \
         the failure this tier was built to make impossible.\n{transcript}"
    );
    assert!(
        stdout.contains(" failed"),
        "{probe_rel} did not reach its own verdict line, so its exit code is not a count of \
         failed arms and gating on it would be guesswork.\n{transcript}"
    );

    // The exit code is the number of failed arms — gate on it.
    assert_eq!(
        out.status.code(),
        Some(0),
        "the vendor-binary tier is red: {arms} arms ran in {probe_rel} and it reports failures. \
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

    // The sibling takes the same stamp from the same place — it imports `BUILD`
    // rather than spelling it again, which is the only way two files cannot
    // drift apart. Assert the import, not a second copy of the string.
    let clear = fs::read_to_string(root.join(CLEAR_PROBE)).expect("the sibling is committed too");
    assert!(
        clear.contains("from probe import") && clear.contains("BUILD"),
        "{CLEAR_PROBE} must take its vendor build id from {PROBE} rather than spelling it \
         again, or the two can report different builds for the same run"
    );

    let notes = fs::read_to_string(root.join(NOTES)).expect("the spike notes are committed");
    assert!(
        notes.contains(RECORDED_BUILD),
        "{NOTES} must carry the same vendor build id as {PROBE} ({RECORDED_BUILD}), or a \
         failing arm cannot be traced to the measurement it contradicts"
    );

    let clear_notes = fs::read_to_string(root.join(CLEAR_NOTES)).expect("C37's notes are committed");
    assert!(
        clear_notes.contains(RECORDED_BUILD),
        "{CLEAR_NOTES} must carry the same vendor build id as {CLEAR_PROBE} ({RECORDED_BUILD}), \
         or a failing `/clear` arm cannot be traced to the measurement it contradicts"
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

    // **This file too, since #27.** The brief-carrier arm stands up its own
    // loopback provider in Rust rather than shelling out to the probe, so the
    // property has to be checked where the URL now lives as well.
    let here = fs::read_to_string(root.join(THIS_FILE)).expect("this tier reads its own source");

    // The sibling addresses nothing at all: it borrows `probe.build_scratch`'s
    // provider block wholesale, so the property is inherited rather than
    // restated. Both halves are checked — that it names no host of its own, and
    // that it still gets its provider from the file that does.
    let clear = fs::read_to_string(root.join(CLEAR_PROBE)).expect("the sibling is committed too");
    assert!(
        clear.contains("probe.build_scratch"),
        "{CLEAR_PROBE} no longer takes its provider block from {PROBE}, so nothing guarantees \
         the model provider it points the vendor at is a local capture server"
    );

    let offenders: Vec<&str> = src
        .split("://")
        .skip(1)
        .chain(clear.split("://").skip(1))
        // The scanner's own `"://"` literal is the one occurrence in this file
        // that is not an address, and it is recognisable because what follows it
        // is the end of that string literal rather than a host.
        .chain(here.split("://").skip(1).filter(|rest| !rest.starts_with('"')))
        .filter(|rest| !rest.starts_with("127.0.0.1"))
        .map(|rest| rest.split_whitespace().next().unwrap_or(rest))
        .collect();

    assert!(
        offenders.is_empty(),
        "{PROBE} and {THIS_FILE} must address nothing but 127.0.0.1 — the zero-token property \
         of this whole tier (C13) is that the model provider is a local capture server. \
         Found: {offenders:?}"
    );
    assert!(
        src.contains("127.0.0.1"),
        "{PROBE} no longer stands up a loopback capture server, so nothing guarantees a run \
         of this tier spends nothing"
    );
    assert!(
        here.contains("127.0.0.1"),
        "{THIS_FILE} no longer stands up a loopback capture server of its own, so the \
         brief-carrier arm is either gone or pointed somewhere that can charge for it"
    );
}

/// **The seeded `CODEX_HOME` loads in the real binary, and the trap it defuses
/// reproduces** (WP-25 phase 2, #26; C6).
///
/// The in-crate tests in `placement::codex` assert what FLEETOR *wrote*. Nothing
/// they can observe proves the vendor accepts it — which is the whole reason this
/// tier exists, and it is doubly the reason here, because the failure being
/// prevented is a pane that dies at spawn with a message about a file the operator
/// never named.
///
/// Two arms, and the negative control is the load-bearing one:
///
///  - **verbatim** — the operator's config copied as-is into a pane's
///    `CODEX_HOME`, with a fabricated `HOME` the way the Fence gives one. The
///    binary refuses, and the path it names is inside the *pane's* private home.
///    That is C6's trap, reproduced rather than quoted.
///  - **seeded** — the identical operator installation through
///    `Harness::seed_config_dir`. The binary loads it and runs the command.
///
/// `codex sandbox <cmd>` is the vehicle: it loads the configuration and runs a
/// command under the real seatbelt, with **no model and no network** — the same
/// zero-token instrument arm 2 of the probe uses.
///
/// Nothing here reads or writes the operator's real `~/.codex`. The operator
/// installation is fabricated under the scratch root, which is possible because
/// `Seed::operator_home` arrives as a value rather than being read from the
/// process.
#[test]
fn the_seeded_codex_home_loads_in_the_real_binary_and_the_trap_reproduces() {
    use fleetor_shell::placement::codex::{codex, OPERATOR_DIR};
    use fleetor_shell::placement::Seed;

    let Some(vendor) = the_vendor_binary() else {
        announce(&[
            format!("SKIPPED: the codex config-seeding arm — `{VENDOR_BIN}` is not on PATH."),
            format!("  Recorded against {RECORDED_BUILD}. Nothing below was measured:"),
            "    seeded-config-loads, tilde-trap-reproduces (#26, C6)".into(),
            "  The in-crate tests still prove what FLEETOR wrote. They cannot prove".into(),
            "  the vendor accepts it — that is what this arm is for.".into(),
        ]);
        return;
    };

    let root = std::env::temp_dir().join(scratch_name("vendor"));
    let operator_home = root.join("operator");
    let operator_dir = operator_home.join(OPERATOR_DIR);
    let pane_home = root.join("pane-home");
    let cwd = root.join("work");
    fs::create_dir_all(&operator_dir).expect("a fabricated operator installation");
    fs::create_dir_all(&pane_home).expect("the Fence's private HOME");
    fs::create_dir_all(&cwd).expect("a pane cwd");

    // The trap, in the shape the operator's own config carries it: a tilde inside
    // a value naming a file inside the operator's own installation.
    let operator_config = "model_instructions_file = \"~/.codex/brief.md\"\n";
    fs::write(operator_dir.join("config.toml"), operator_config).expect("operator config");
    fs::write(operator_dir.join("brief.md"), "a fifty-character sentinel brief\n").expect("brief");

    let run = |codex_home: &Path| {
        Command::new(&vendor)
            .args(["sandbox", "/bin/echo", "ok"])
            .env("HOME", &pane_home)
            .env("CODEX_HOME", codex_home)
            .current_dir(&cwd)
            .output()
            .unwrap_or_else(|e| panic!("could not run {}: {e}", vendor.display()))
    };

    // Arm 1 — verbatim. The refusal names a path inside the pane's private HOME,
    // which is the whole of C6's measurement.
    let verbatim = root.join("verbatim");
    fs::create_dir_all(&verbatim).expect("a verbatim CODEX_HOME");
    fs::write(verbatim.join("config.toml"), operator_config).expect("verbatim config");
    let refused = run(&verbatim);
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&refused.stdout),
        String::from_utf8_lossy(&refused.stderr)
    );
    assert_ne!(
        refused.status.code(),
        Some(0),
        "a `~` copied verbatim into a pane's CODEX_HOME no longer kills the pane. Either the \
         vendor changed how it resolves one, or this arm stopped testing anything — check \
         which before relaxing the seeder.\n{said}"
    );
    assert!(
        said.contains(&pane_home.display().to_string()),
        "the refusal was expected to name a path inside the pane's private HOME ({}), which \
         is what makes this the trap C6 measured rather than some other failure.\n{said}",
        pane_home.display()
    );

    // Arm 2 — seeded. The same installation, through the harness.
    //
    // The brief is spelled out because #27 made an absent one a refusal rather
    // than a quiet seed. Note that `model_instructions_file` is now a key FLEETOR
    // *owns*, so in the seeded arm its tilde is replaced outright rather than
    // relocated — arm 1, the verbatim copy, is where the trap still lives, and the
    // relocation itself is pinned in-crate by
    // `no_seeded_value_resolves_against_the_panes_private_home`.
    let seeded = root.join("seeded");
    codex()
        .seed_config_dir(
            &Seed::new(&seeded, &cwd, Some(&operator_home))
                .with_brief("a fifty-character sentinel brief for the pane"),
        )
        .expect("seeding a pane's CODEX_HOME");
    let accepted = run(&seeded);
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&accepted.stdout),
        String::from_utf8_lossy(&accepted.stderr)
    );
    assert_eq!(
        accepted.status.code(),
        Some(0),
        "the real binary refused a configuration this repository seeded. Whatever the seeder \
         writes, the vendor has to accept — an artifact assertion cannot see this.\n{said}"
    );
    assert!(said.contains("ok"), "the sandboxed command did not run:\n{said}");

    // And the operator's installation is exactly as it was.
    assert_eq!(
        fs::read_to_string(operator_dir.join("config.toml")).expect("still there"),
        operator_config,
        "seeding wrote back into the operator's own installation",
    );

    fs::remove_dir_all(&root).ok();
}

// --- checkpoint 2, against the wire (#27, C3, C37) ---------------------------

/// A loopback capture server: a model provider that records the literal request
/// body and completes the stream without ever answering.
///
/// **This is the zero-token property, as a mechanism rather than a promise.** The
/// provider a codex pane is pointed at for the length of this arm is a
/// `TcpListener` on `127.0.0.1`, so the request the vendor builds is readable in
/// full and nothing reaches an endpoint that could charge for it.
///
/// Hand-rolled rather than pulled in, for this file's stated reason: it has no
/// dependencies, and an integration test cannot reach the crate's own anyway.
struct Capture {
    port: u16,
    bodies: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl Capture {
    fn start() -> Self {
        use std::io::{BufRead, BufReader, Read};
        use std::net::TcpListener;
        use std::sync::{Arc, Mutex};

        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let port = listener.local_addr().expect("its address").port();
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&bodies);

        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(&stream);
                let mut length = 0usize;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(n) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = n.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0u8; length];
                if reader.read_exact(&mut body).is_ok() {
                    recorded
                        .lock()
                        .expect("the capture lock")
                        .push(String::from_utf8_lossy(&body).into_owned());
                }
                let mut out = &stream;
                let _ = Write::write_all(
                    &mut out,
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                      Connection: close\r\n\r\ndata: [DONE]\n\n",
                );
                let _ = Write::flush(&mut out);
            }
        });

        Self { port, bodies }
    }

    /// The first request body this server saw, or `None` if the vendor never
    /// spoke to it.
    fn first(&self) -> Option<String> {
        self.bodies.lock().expect("the capture lock").first().cloned()
    }
}

/// One JSON string literal's worth of escaping — enough to look for a rendered
/// brief inside a captured body without parsing the body.
///
/// The brief is markdown: newlines, quotes and backslashes are the only three
/// things the vendor's serializer will have changed, and a needle that survives
/// all three is a needle that proves the whole document arrived.
fn as_json_text(raw: &str) -> String {
    raw.chars()
        .map(|c| match c {
            '"' => "\\\"".to_string(),
            '\\' => "\\\\".to_string(),
            '\n' => "\\n".to_string(),
            '\r' => "\\r".to_string(),
            '\t' => "\\t".to_string(),
            other => other.to_string(),
        })
        .collect()
}

/// **Checkpoint 2, proven rather than asserted: the brief FLEETOR seeds actually
/// replaces the vendor's own system prompt** (#27, C3, C37).
///
/// This is the arm the whole tier exists for. `placement::codex`'s in-crate tests
/// prove FLEETOR wrote `model_instructions_file` and wrote a file for it to name;
/// **nothing they can observe proves the vendor honoured either**, and this is a
/// checkpoint that fails by looking healthy — a pane that never got its brief
/// still renders a prompt, still accepts a paste and still answers, having never
/// been told it is part of a fleet.
///
/// So the pane's `CODEX_HOME` is seeded through `Harness::seed_config_dir` with a
/// **real rendered worker brief**, the real binary is run against it, and the
/// request body it builds is read off a loopback socket.
///
/// Three assertions, and the third is the one that makes the first two mean
/// anything:
///
///  1. **The brief is in the request**, whole, and it arrives ahead of the first
///     `user` message — so it is the pane's instructions and not the in-band
///     `AGENTS.md` shape C3 measured and rejected.
///  2. **`You are Codex` is absent from the entire body.** That is *replace*
///     rather than append (D-043), which is the property the fleet's brief depends
///     on.
///  3. **The negative control**: the identical run against an *un-seeded*
///     `CODEX_HOME` carries the vendor's own prompt and not the brief. Without it
///     a vendor that had stopped sending a system prompt at all would read as a
///     pass.
///
/// **What did not fit the recorded shape, stated plainly.** C3 and C37 quote
/// `instructions` — a top-level field — because both drove a *cloned catalog
/// entry* (`probe.py`'s `probe-model`). Against the build's own default model the
/// prompt travels instead as the first `developer` message in `input`. The
/// carrier's behaviour is unchanged and so is the decision; what changed is which
/// wire slot the prompt occupies, which is why this arm asserts on the body rather
/// than on a field name.
///
/// **Zero tokens**, and nothing here reads or writes the operator's real
/// `~/.codex`: the operator installation is fabricated under a scratch root and
/// handed down on `Seed::operator_home`.
#[test]
fn the_seeded_brief_replaces_the_vendor_prompt_on_the_wire() {
    use fleetor_core::pane::{PaneId, WORKER_SLOTS};
    use fleetor_shell::placement::codex::{codex, OPERATOR_DIR};
    use fleetor_shell::placement::Seed;

    let Some(vendor) = the_vendor_binary() else {
        announce(&[
            format!("SKIPPED: the codex brief-carrier arm — `{VENDOR_BIN}` is not on PATH."),
            format!("  Recorded against {RECORDED_BUILD}. Nothing below was measured:"),
            "    seeded-brief-replaces-the-vendor-prompt (#27, C3)".into(),
            "  The in-crate tests still prove FLEETOR wrote the carrier key and the file".into(),
            "  it names. They cannot prove the vendor honoured it, and a pane that never".into(),
            "  got its brief looks exactly like one that did.".into(),
        ]);
        return;
    };

    let root = std::env::temp_dir().join(scratch_name("brief"));
    let operator_home = root.join("operator");
    let pane_home = root.join("pane-home");
    let cwd = root.join("work");
    let bare = root.join("bare");
    fs::create_dir_all(operator_home.join(OPERATOR_DIR)).expect("a fabricated installation");
    fs::create_dir_all(&pane_home).expect("the Fence's private HOME");
    fs::create_dir_all(&cwd).expect("a pane cwd");
    fs::create_dir_all(&bare).expect("an un-seeded CODEX_HOME");

    // The real thing: what `place_worker` renders and hands to `Seed::with_brief`,
    // fragments composed in and all.
    let brief = fleetor_core::brief::worker_brief(
        PaneId::Worker(1),
        &PaneId::roster(&WORKER_SLOTS),
        &cwd.to_string_lossy(),
    );
    let seeded = root.join("seeded");
    codex()
        .seed_config_dir(&Seed::new(&seeded, &cwd, Some(&operator_home)).with_brief(&brief))
        .expect("seeding a pane's CODEX_HOME");

    // One turn against a loopback provider, for each of the two homes. The exit
    // status is deliberately not gated on: the capture server completes no
    // response, so the vendor exits non-zero having already sent the request that
    // is the whole measurement.
    let turn = |codex_home: &Path| {
        let capture = Capture::start();
        let base_url = format!("http://127.0.0.1:{}", capture.port);
        let over = |key: &str, value: &str| ["-c".to_string(), format!("{key}={value}")];
        let args: Vec<String> = ["exec", "--skip-git-repo-check"]
            .iter()
            .map(|a| (*a).to_string())
            .chain(over("model_provider", "probe"))
            .chain(over("model_providers.probe.name", "\"probe\""))
            .chain(over("model_providers.probe.base_url", &format!("\"{base_url}\"")))
            .chain(over("model_providers.probe.wire_api", "\"responses\""))
            .chain(over("model_providers.probe.experimental_bearer_token", "\"sk-probe\""))
            .chain(over("model_providers.probe.request_max_retries", "0"))
            .chain(over("model_providers.probe.stream_max_retries", "0"))
            .collect();

        let mut child = Command::new(&vendor)
            .args(&args)
            .env("HOME", &pane_home)
            .env("CODEX_HOME", codex_home)
            .current_dir(&cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("could not run {}: {e}", vendor.display()));
        Write::write_all(child.stdin.as_mut().expect("a stdin pipe"), b"hi\n").expect("the turn");
        drop(child.stdin.take());
        let out = child.wait_with_output().expect("the vendor exits");
        capture.first().unwrap_or_else(|| {
            panic!(
                "no request reached the loopback provider, so nothing was measured:\n{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            )
        })
    };

    // 1 and 2 — the seeded pane.
    let body = turn(&seeded);
    // `trim_end`, because the vendor drops the file's trailing newline and a
    // needle that carried one would fail on a whole brief that arrived intact.
    let needle = as_json_text(brief.trim_end());
    let at = body.find(&needle).unwrap_or_else(|| {
        panic!(
            "the brief FLEETOR seeded never reached the request. `model_instructions_file` is \
             written and the file it names exists — the in-crate tests prove that — so either \
             the vendor stopped honouring the key or the seeder stopped pointing at the file. \
             Body was {} bytes.",
            body.len()
        )
    });
    assert!(
        !body.contains("You are Codex"),
        "the vendor's own system prompt came back alongside the brief. The carrier is supposed \
         to *replace* it (D-043, C3); a brief underneath the vendor's instructions is a \
         different product, and every pane in the fleet would be running it."
    );
    let first_user = body.find("\"role\": \"user\"").or_else(|| body.find("\"role\":\"user\""));
    assert!(
        first_user.is_none_or(|user| at < user),
        "the brief arrived at or after the first `user` message — which is the shape of the \
         `AGENTS.md` carrier C3 measured and rejected: in-band, spending the pane's own \
         context, reading as though the operator typed it."
    );

    // The two fragments, on the wire rather than in a rendered string (§4).
    for clause in ["did **not** deliver", "Never reply to a broadcast unless it names you"] {
        assert!(
            body.contains(&as_json_text(clause)),
            "the brief reached the model without `{clause}` — the delivery contract and the \
             broadcast rule are the two clauses the fleet cannot run without",
        );
    }

    // 3 — the control. Same command, same provider, an un-seeded CODEX_HOME.
    let control = turn(&bare);
    assert!(
        control.contains("You are Codex"),
        "the negative control did not carry the vendor's own prompt either, so the run above \
         proves nothing: a vendor that had stopped sending a system prompt at all would read \
         exactly like a brief that replaced one."
    );
    assert!(
        !control.contains(&needle),
        "an un-seeded CODEX_HOME carried FLEETOR's brief, so the brief is arriving from \
         somewhere other than the seed and this arm is measuring the wrong thing."
    );

    fs::remove_dir_all(&root).ok();
}

/// **The fence is real, the socket is reachable through it, and the seeded file
/// is what the vendor resolves its posture from** (WP-25 phase 2, #28; C5, C7,
/// C21).
///
/// This is the arm the ticket exists for. Everything `placement::codex`'s own
/// tests can see is what FLEETOR *wrote*; a checkpoint that fails by looking
/// healthy needs the vendor's own answer, and `codex` gives two zero-token
/// instruments that between them supply it:
///
///  - **`codex sandbox <cmd>`** runs a command under the **real seatbelt** with no
///    model and no network, so what the trio's values actually permit and refuse
///    is measured rather than inferred.
///  - **`codex doctor --json`** resolves the configuration in a `CODEX_HOME` and
///    reports the posture it arrived at — `sandbox.helpers` for the fence,
///    `config.load`'s enabled-feature list for C21's four.
///
/// **Two instruments rather than one, because neither is sufficient and the
/// reason is a measurement.** `codex sandbox` takes its sandbox from `-c`
/// overrides and **ignores `sandbox_mode` in `config.toml` entirely** — a file
/// saying `danger-full-access` still runs the command read-only. So it can prove
/// what the trio's *values* do and can prove nothing about the *file*. `doctor`
/// is the other way round: it reads the file and names the resolved posture, and
/// runs nothing under a seatbelt. Together they close the loop. Apart, either one
/// is the artifact assertion this tier exists to replace.
///
/// **The inside-the-workspace arm is the load-bearing one.** A fence that refuses
/// every write passes a "cannot write outside the worktree" assertion perfectly,
/// and is exactly the failure C7 named when it rejected the newer permission-profile
/// generation: *"falling back to read-only"* — a worker that spawns clean and
/// silently cannot work. Asserting the refusal without asserting the permission
/// measures nothing.
///
/// Zero tokens. No completion is requested at any point; `doctor` makes a
/// provider *reachability* probe that this test reads nothing from, and every
/// assertion below still holds on a machine with no network.
#[test]
fn the_sandbox_trio_fences_a_real_pane_and_the_fleet_socket_still_reaches_it() {
    use fleetor_shell::placement::codex::{codex, CODEX_SPEC, OPERATOR_DIR};
    use fleetor_shell::placement::Seed;
    use std::io::Read;
    use std::os::unix::net::UnixListener;

    const FOUR: [&str; 4] =
        ["multi_agent", "browser_use", "computer_use", "in_app_local_automation"];

    let Some(vendor) = the_vendor_binary() else {
        announce(&[
            format!("SKIPPED: the codex containment arm — `{VENDOR_BIN}` is not on PATH."),
            format!("  Recorded against {RECORDED_BUILD}. Nothing below was measured:"),
            "    fence-refuses-outside, fence-permits-inside, socket-reachable,".into(),
            "    socket-refused-without-the-lever, seeded-posture-resolves,".into(),
            "    four-features-off-on-a-worker, orchestrator-keeps-them (#28, C5, C7, C21)".into(),
            "  The in-crate tests still prove what FLEETOR wrote. Only the real".into(),
            "  seatbelt can prove the vendor honoured it, and this checkpoint is".into(),
            "  precisely one that fails by looking healthy.".into(),
        ]);
        return;
    };

    let root = std::env::temp_dir().join(scratch_name("fence"));
    let operator_home = root.join("operator");
    let operator_dir = operator_home.join(OPERATOR_DIR);
    let pane_home = root.join("pane-home");
    let cwd = root.join("worktree");
    fs::create_dir_all(&operator_dir).expect("a fabricated operator installation");
    fs::create_dir_all(&pane_home).expect("the Fence's private HOME");
    fs::create_dir_all(&cwd).expect("a pane worktree");

    // **`$TMPDIR` is one of `workspace-write`'s default writable roots**, measured
    // here rather than assumed: with the ambient `TMPDIR` left alone, a scratch
    // root under `std::env::temp_dir()` is *inside* the fence and the outside-write
    // arm below passes for the wrong reason. Production is not arranged that way —
    // the Fence's private `HOME` is `~/.fleetor/_shell/homes/worker-N` and the
    // fleet's socket is under `~/.fleetor` — so the child is handed a `TMPDIR`
    // inside its own worktree, which reproduces the real layout rather than
    // relaxing the posture under test. Nothing about the trio changes.
    let pane_tmp = cwd.join("tmp");
    fs::create_dir_all(&pane_tmp).expect("the pane's own TMPDIR");

    // **A hostile operator configuration**, which is the case the two FLEETOR-owned
    // keys exist for: someone who turned the sandbox off months ago for unrelated
    // reasons, and narrowed its network access. Inherited verbatim this is an
    // unfenced pane that cannot talk.
    fs::write(
        operator_dir.join("config.toml"),
        "sandbox_mode = \"danger-full-access\"\n\
         approval_policy = \"on-request\"\n\
         [sandbox_workspace_write]\n\
         network_access = false\n",
    )
    .expect("operator config");

    // --- part one: what the trio's values actually do, under the real seatbelt ---

    // The rows are read off the spec rather than spelled here, so a row that
    // changed to something that does not fence goes red here and not in review.
    let trio: Vec<String> = CODEX_SPEC
        .posture
        .sandbox_keys
        .iter()
        .chain(CODEX_SPEC.outbound.reachability_keys)
        .map(|(key, value)| format!("{key}={value}"))
        .collect();
    assert_eq!(trio.len(), 3, "the trio was measured as three rows together: {trio:?}");

    let under_seatbelt = |overrides: &[String], argv: &[&str]| {
        let mut command = Command::new(&vendor);
        command.arg("sandbox");
        for over in overrides {
            command.args(["-c", over]);
        }
        command
            .args(argv)
            .env("HOME", &pane_home)
            .env("TMPDIR", &pane_tmp)
            .env("CODEX_HOME", &operator_dir)
            .current_dir(&cwd)
            .output()
            .unwrap_or_else(|e| panic!("could not run {}: {e}", vendor.display()))
    };
    let said = |out: &std::process::Output| {
        format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
    };

    // Arm 1 — a write outside the worktree is refused, and no file is created.
    let escaped = pane_home.join("probe");
    let outside = under_seatbelt(&trio, &["sh", "-c", "echo x > \"$HOME/probe\""]);
    assert_ne!(
        outside.status.code(),
        Some(0),
        "a codex pane wrote outside its worktree under the trio FLEETOR seeds.\n{}",
        said(&outside),
    );
    assert!(
        !escaped.exists(),
        "the command failed but the file is there — a refusal after the write is not a fence",
    );

    // Arm 2 — **and a write inside it succeeds.** Without this, a read-only
    // fallback passes arm 1 and ships a worker that cannot do any work (C7).
    let inside = under_seatbelt(&trio, &["sh", "-c", "echo ok > ./inside-the-worktree"]);
    assert_eq!(
        inside.status.code(),
        Some(0),
        "a codex worker cannot write inside its own worktree. That is the \
         `falling back to read-only` failure C7 rejected the newer permission-profile \
         generation over, and it looks perfectly healthy at spawn.\n{}",
        said(&inside),
    );
    assert!(cwd.join("inside-the-worktree").is_file(), "exit 0 but nothing was written");

    // Arms 3 and 4 — the socket, and the lever that makes it reachable. Skipped
    // together rather than faked, since a probe needs an interpreter to be one.
    match on_path("python3") {
        None => announce(&[
            "SKIPPED: the codex socket arms — no `python3` to connect with.".into(),
            "  socket-reachable and socket-refused-without-the-lever were not measured.".into(),
        ]),
        Some(python) => {
            const PROBE: &str = "import socket, sys\n\
                 s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)\n\
                 s.connect(sys.argv[1])\n\
                 s.sendall(b'HELLO-FLEET')\n\
                 print('CONNECTED')\n";
            // Outside the worktree, the way the fleet's own socket is: this is a
            // network permission rather than a filesystem one, and a socket that
            // happened to sit inside the writable root would not show that.
            let socket = root.join("fleet.sock");
            let listener = UnixListener::bind(&socket).expect("the fleet's own socket");
            let heard = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().expect("a pane connected");
                let mut body = Vec::new();
                stream.read_to_end(&mut body).expect("what it sent");
                body
            });

            let probe = [python.to_string_lossy().into_owned(), "-c".into(), PROBE.into(), socket
                .to_string_lossy()
                .into_owned()];
            let argv: Vec<&str> = probe.iter().map(String::as_str).collect();

            let connected = under_seatbelt(&trio, &argv);
            assert!(
                said(&connected).contains("CONNECTED"),
                "a fenced codex pane could not reach the fleet socket. Every `fleet send` from \
                 it would fail, and the pane would look perfectly alive.\n{}",
                said(&connected),
            );
            assert_eq!(
                heard.join().expect("the listener thread"),
                b"HELLO-FLEET",
                "the connect succeeded but the listener got nothing — connecting is not talking",
            );

            // The negative control, and it is what makes the arm above mean
            // anything: with the one lever off, the identical probe is refused.
            // This is C5's measurement reproduced, and the reason no bridge exists
            // on the message path.
            let without: Vec<String> = trio
                .iter()
                .map(|row| row.replace("network_access=true", "network_access=false"))
                .collect();
            assert_ne!(
                without, trio,
                "the negative control changed nothing, so it controls for nothing",
            );
            let refused = under_seatbelt(&without, &argv);
            assert_ne!(
                refused.status.code(),
                Some(0),
                "the socket was reachable with the network lever off, so arm 3 was not \
                 measuring the lever.\n{}",
                said(&refused),
            );
        }
    }

    // --- part two: the seeded file is what the vendor resolves its posture from ---

    let seeded = |dir: &Path, operators_own: bool| {
        let seed = Seed::new(dir, &cwd, Some(&operator_home)).with_brief("a fenced pane's brief");
        let seed = if operators_own { seed.for_the_operator() } else { seed };
        codex().seed_config_dir(&seed).expect("seeding a pane's CODEX_HOME");
    };
    let worker = root.join("worker-1");
    let orch = root.join("orch");
    seeded(&worker, false);
    seeded(&orch, true);

    let report = |codex_home: &Path| -> serde_json::Value {
        let out = Command::new(&vendor)
            .args(["doctor", "--json"])
            .env("HOME", &pane_home)
            .env("CODEX_HOME", codex_home)
            .current_dir(&cwd)
            .output()
            .unwrap_or_else(|e| panic!("could not run {}: {e}", vendor.display()));
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
            panic!("`{VENDOR_BIN} doctor --json` stopped emitting JSON ({e}):\n{}", said(&out))
        })
    };

    // Arm 5 — the vendor's own reading of the seeded document. `restricted` and
    // `Never` are what the operator's `danger-full-access` / `on-request` would
    // *not* have produced, which is what makes this an assertion about FLEETOR's
    // keys winning rather than about codex's defaults.
    let worker_report = report(&worker);
    let helpers = &worker_report["checks"]["sandbox.helpers"]["details"];
    assert_eq!(
        helpers["filesystem sandbox"].as_str(),
        Some("restricted"),
        "the vendor resolved the seeded config to an unfenced pane: {helpers}",
    );
    assert_eq!(
        helpers["approval policy"].as_str(),
        Some("Never"),
        "a worker has no human to answer an approval prompt, so it would park looking \
         perfectly healthy: {helpers}",
    );

    // Arms 6 and 7 — C21's asymmetry, read off the vendor's own resolved list.
    let enabled = |report: &serde_json::Value| -> Vec<String> {
        report["checks"]["config.load"]["details"]["enabled feature flags"]
            .as_str()
            .unwrap_or_else(|| panic!("`doctor` stopped reporting the resolved feature flags"))
            .split(',')
            .map(|name| name.trim().to_string())
            .collect()
    };
    let on_the_worker = enabled(&worker_report);
    let on_the_orchestrator = enabled(&report(&orch));
    for feature in FOUR {
        assert!(
            !on_the_worker.contains(&feature.to_string()),
            "`{feature}` is still on for a codex worker. The sandbox bounds the filesystem \
             and the network; it does not bound a pane driving a browser or a desktop, or one \
             fanning out into threads the run manifest never sees (C21).",
        );
        assert!(
            on_the_orchestrator.contains(&feature.to_string()),
            "`{feature}` was turned off on the orchestrator. That is the operator's own pane \
             and it inherits their flags untouched — the asymmetry is the product (C21).",
        );
    }

    fs::remove_dir_all(&root).ok();
}

/// **The vendor resolves a worker to FLEETOR's provider, and the operator's own
/// key cannot satisfy it** (WP-25 phase 2, #29; C2, C9, D-062).
///
/// Everything `placement::codex`'s own tests can see is what FLEETOR *wrote*.
/// Checkpoint 5 is the checkpoint most able to fail while looking healthy — a pane
/// carrying the wrong credential boots, reaches its prompt and answers — so the
/// arm that matters is the vendor's own reading of the seeded document.
/// `codex doctor --json` supplies it: `checks["auth.credentials"]` names **which
/// variable the active provider authenticates through**, and whether it is there.
///
/// Four arms, and the middle one is the load-bearing one:
///
///  1. A fenced seat with the fleet's key present resolves to FLEETOR's entry and
///     the variable placement sets.
///  2. **The same seat with the fleet's key absent and the operator's
///     `CODEX_API_KEY` present still fails.** That is the property worth having:
///     the fleet's provider entry cannot be satisfied by an operator's own
///     credential sitting in a shell profile, by construction rather than by the
///     scrub — which makes `Credentials::scrubbed_env` the second line of defence
///     rather than the only one. Without this arm, arm 1 passes just as well on a
///     seeder that quietly fell back to whatever the environment had.
///  3. The operator's own seat is **not** on FLEETOR's provider — C2 as C9 amended
///     it, read off the vendor rather than off what the seeder wrote.
///  4. The entry the seeder wrote is a document the vendor accepts.
///
/// Zero tokens. `doctor` requests no completion; its provider *reachability* probe
/// is read from nowhere here, and every assertion holds on a machine with no
/// network.
#[test]
fn the_vendor_resolves_a_worker_to_fleetors_provider_and_refuses_the_operators_key() {
    use fleetor_shell::placement::codex::{codex, CODEX_SPEC, OPERATOR_DIR};
    use fleetor_shell::placement::Seed;

    // The operator's own installation, in the shape the spike measured: a
    // third-party provider with its own bearer token, selected by `model_provider`.
    const OPERATORS_PROVIDER: &str = "operators-own";
    const OPERATORS_TOKEN: &str = "sk-operator-CREDENTIAL-SENTINEL";

    let fleet_key_env = CODEX_SPEC
        .credentials
        .token_env
        .expect("codex carries the fleet's key in a variable its provider entry names");

    let Some(vendor) = the_vendor_binary() else {
        announce(&[
            format!("SKIPPED: the codex credential arm — `{VENDOR_BIN}` is not on PATH."),
            format!("  Recorded against {RECORDED_BUILD}. Nothing below was measured:"),
            "    worker-resolves-to-fleetor, operators-key-cannot-satisfy-it,".into(),
            "    orchestrator-keeps-its-own, the-entry-is-a-document-the-vendor-accepts".into(),
            "  The in-crate tests still prove what FLEETOR wrote into the seed and".into(),
            "  that no credential of the operator's survives into a fenced pane.".into(),
            "  Only the vendor can say which provider it actually resolved (#29, C9).".into(),
        ]);
        return;
    };

    let root = std::env::temp_dir().join(scratch_name("cred"));
    let operator_home = root.join("operator");
    let operator_dir = operator_home.join(OPERATOR_DIR);
    let pane_home = root.join("pane-home");
    let cwd = root.join("worktree");
    fs::create_dir_all(&operator_dir).expect("a fabricated operator installation");
    fs::create_dir_all(&pane_home).expect("the Fence's private HOME");
    fs::create_dir_all(&cwd).expect("a pane worktree");
    fs::write(
        operator_dir.join("config.toml"),
        format!(
            // The fabricated operator's endpoint is **loopback**, and not because
            // this arm dials it: `the_probe_addresses_nothing_but_loopback` reads
            // this file's own source and refuses any other host, which is the
            // property that keeps the whole tier free (C13). What this arm is
            // about is *which provider a seat resolves to*, and that is decided by
            // `model_provider`, never by the URL under it.
            "model_provider = \"{OPERATORS_PROVIDER}\"\n\
             [model_providers.{OPERATORS_PROVIDER}]\n\
             name = \"the operator's own\"\n\
             base_url = \"http://127.0.0.1:9/\"\n\
             wire_api = \"responses\"\n\
             experimental_bearer_token = \"{OPERATORS_TOKEN}\"\n",
        ),
    )
    .expect("operator config");

    let seeded = |dir: &Path, operators_own: bool| {
        let seed = Seed::new(dir, &cwd, Some(&operator_home)).with_brief("a pane's brief");
        let seed = if operators_own { seed.for_the_operator() } else { seed };
        codex().seed_config_dir(&seed).expect("seeding a pane's CODEX_HOME");
    };
    let worker = root.join("worker-1");
    let orch = root.join("orch");
    seeded(&worker, false);
    seeded(&orch, true);

    let said = |out: &std::process::Output| {
        format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
    };
    // `doctor` with a controlled environment: the two names checkpoint 5 scrubs are
    // removed unless an arm puts one back, so nothing this machine happens to
    // export can decide the result.
    let report = |codex_home: &Path, env: &[(&str, &str)]| -> serde_json::Value {
        let mut command = Command::new(&vendor);
        command
            .args(["doctor", "--json"])
            .env("HOME", &pane_home)
            .env("CODEX_HOME", codex_home)
            .current_dir(&cwd);
        for name in CODEX_SPEC.credentials.scrubbed_env {
            command.env_remove(name);
        }
        command.env_remove(fleet_key_env);
        for (name, value) in env {
            command.env(name, value);
        }
        let out = command.output().unwrap_or_else(|e| panic!("could not run {}: {e}", vendor.display()));
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
            panic!("`{VENDOR_BIN} doctor --json` stopped emitting JSON ({e}):\n{}", said(&out))
        })
    };
    let auth = |report: &serde_json::Value| -> serde_json::Value {
        report["checks"]["auth.credentials"].clone()
    };

    // Arm 1 — the fenced seat, with the fleet's key where placement puts it.
    let with_the_fleets_key = auth(&report(&worker, &[(fleet_key_env, "sk-the-fleets-own-key")]));
    assert_eq!(
        with_the_fleets_key["details"]["provider auth env var"].as_str(),
        Some(format!("{fleet_key_env} (present)").as_str()),
        "the vendor did not resolve a worker to FLEETOR's provider entry: {with_the_fleets_key}",
    );
    assert_eq!(
        with_the_fleets_key["status"].as_str(),
        Some("ok"),
        "the entry resolved to a name and then would not authenticate: {with_the_fleets_key}",
    );

    // Arm 2 — the same seat, the fleet's key gone and the *operator's* in its
    // place. This is the arm that proves arm 1 measured the entry rather than the
    // environment.
    let with_the_operators_key = auth(&report(&worker, &[("CODEX_API_KEY", "sk-the-operators-own")]));
    assert_eq!(
        with_the_operators_key["status"].as_str(),
        Some("fail"),
        "an operator's own key in the environment authenticated a fenced pane. A worker holds \
         the fleet's credential and never the operator's (D-062): {with_the_operators_key}",
    );
    assert_eq!(
        with_the_operators_key["details"]["provider auth env var"].as_str(),
        Some(format!("{fleet_key_env} (missing)").as_str()),
        "the fleet's entry stopped naming the one variable it authenticates through: \
         {with_the_operators_key}",
    );
    assert_eq!(
        with_the_operators_key["details"]["auth env vars present"].as_str(),
        Some("CODEX_API_KEY"),
        "the operator's key was not in the environment, so arm 2 refused for the wrong \
         reason: {with_the_operators_key}",
    );

    // Arm 3 — the operator's own seat, which keeps the provider it inherited
    // (D-030, D-052). Asserted against the vendor's reading: a seat resolved to
    // FLEETOR's entry would name the fleet's variable here, and this one does not.
    let operators_seat = auth(&report(&orch, &[]));
    assert!(
        operators_seat["details"]["provider auth env var"].is_null(),
        "the orchestrator was switched onto the fleet's provider. That seat runs the \
         operator's own login and inherited provider, and there is no picker anywhere \
         (C2 as amended by C9): {operators_seat}",
    );
    let inherited = fs::read_to_string(orch.join("config.toml")).expect("the orchestrator's seed");
    assert!(
        inherited.contains(OPERATORS_PROVIDER),
        "the orchestrator lost the provider it inherited:\n{inherited}",
    );

    // Arm 4 — the entry is a document the vendor accepts. A provider table with a
    // malformed row is a pane that dies at configuration load, which every other
    // arm here would report as some other failure.
    let loaded = report(&worker, &[(fleet_key_env, "sk-the-fleets-own-key")]);
    assert_eq!(
        loaded["checks"]["config.load"]["details"]["config.toml parse"].as_str(),
        Some("ok"),
        "the vendor would not parse the seeded document: {}",
        loaded["checks"]["config.load"],
    );

    fs::remove_dir_all(&root).ok();
}

/// **The operator's own seat is logged in, and the vendor is what says so** (WP-25
/// phase 2, #44; C6, C9, C14, C43).
///
/// The arm above proves a *worker* runs on FLEETOR's provider. This one proves the
/// other half of the same asymmetry, and it is the half that was missing: a codex
/// login lives **inside the directory this seeder replaces** (C6 — `CODEX_HOME`
/// alone determines configuration and login), so striking the credential on every
/// seat handed the orchestrator a pane with no login at all. That failure is the
/// arc's signature shape — a configuration that loads, a pane that starts, and a
/// seat that cannot authenticate — so it is asserted against the vendor's own
/// reading rather than inferred from `auth.json` being on disk.
///
/// **All three auth shapes C14 names, because the copy has to survive all three**
/// (measured on `codex-cli 0.153.4`, each one a distinct `auth.credentials`
/// reading):
///
/// | shape | where the credential lives | what `doctor` says on the operator's seat |
/// |---|---|---|
/// | subscription plan | `auth.json` ChatGPT tokens | `stored auth mode = chatgpt` |
/// | API key | `auth.json` API key | `stored auth mode = api_key` |
/// | named custom provider | `config.toml` `env_key` / bearer token | `provider auth env var … (present)` |
///
/// The third is the operator's own installation on the machine this was written
/// on, which is why a copy that handled only the first two would pass here and
/// fail there.
///
/// **The negative control is the load-bearing arm.** The same seeded orchestrator
/// with its `auth.json` removed reports `fail` — *"no Codex credentials were
/// found"* — which is exactly what #26 shipped and what this ticket fixes. Without
/// it, every assertion above passes just as well on a machine whose ambient
/// environment happened to be logged in.
///
/// Zero tokens, and zero non-loopback traffic: each fabricated provider is
/// `127.0.0.1` and carries `requires_openai_auth`, so the shapes that consult
/// `auth.json` do so without the vendor's default endpoint being dialled at all.
#[test]
fn the_operators_own_seat_authenticates_and_a_fenced_seat_holds_nothing_of_the_operators() {
    use fleetor_shell::placement::codex::{codex, CODEX_SPEC, OPERATOR_DIR};
    use fleetor_shell::placement::Seed;

    /// One string, in every shape's credential, so a worker's pane directory can be
    /// scanned for the operator's key as bytes rather than as a key name.
    const SENTINEL: &str = "sk-operator-CREDENTIAL-SENTINEL";
    /// The variable the operator's *own* shell exports their key in — scrubbed on a
    /// fenced seat, and not on theirs.
    const OPERATORS_SHELL_VAR: &str = "OPERATORS_SHELL_KEY";

    let fleet_key_env = CODEX_SPEC
        .credentials
        .token_env
        .expect("codex carries the fleet's key in a variable its provider entry names");

    let Some(vendor) = the_vendor_binary() else {
        announce(&[
            format!("SKIPPED: the codex orchestrator-login arm — `{VENDOR_BIN}` is not on PATH."),
            format!("  Recorded against {RECORDED_BUILD}. Nothing below was measured:"),
            "    plan-shape-survives, api-key-shape-survives, provider-shape-survives,".into(),
            "    a-seat-without-the-copy-has-no-login (#44, C14, C43)".into(),
            "  The in-crate tests still prove the operator's `auth.json` and provider".into(),
            "  credential reach their own seat and no other. Only the vendor can say".into(),
            "  whether what reached it counts as being logged in.".into(),
        ]);
        return;
    };

    let root = std::env::temp_dir().join(scratch_name("seat"));
    let pane_home = root.join("pane-home");
    let cwd = root.join("worktree");
    fs::create_dir_all(&pane_home).expect("the Fence's private HOME");
    fs::create_dir_all(&cwd).expect("a pane worktree");

    // A fabricated operator installation. Never `~/.codex`: `Seed::operator_home`
    // arrives as a value, which is the mechanism that makes the operator's real
    // installation unreachable from here even by accident.
    let installation = |name: &str, config: &str, auth: Option<&str>| -> PathBuf {
        let home = root.join(name);
        let dir = home.join(OPERATOR_DIR);
        fs::create_dir_all(&dir).expect("a fabricated operator installation");
        fs::write(dir.join("config.toml"), config).expect("operator config");
        if let Some(body) = auth {
            fs::write(dir.join("auth.json"), body).expect("operator credential");
        }
        home
    };

    let seed_into = |dir: &Path, operator_home: &Path, operators_own: bool| {
        let seed = Seed::new(dir, &cwd, Some(operator_home)).with_brief("a pane's brief");
        let seed = if operators_own { seed.for_the_operator() } else { seed };
        codex().seed_config_dir(&seed).expect("seeding a pane's CODEX_HOME");
    };

    let said = |out: &std::process::Output| {
        format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
    };
    // `doctor`, with a controlled environment: everything checkpoint 5 scrubs, the
    // fleet's variable and the operator's own shell variable are all removed unless
    // an arm puts one back, so nothing this machine exports can decide a result.
    let auth = |codex_home: &Path, env: &[(&str, &str)]| -> serde_json::Value {
        let mut command = Command::new(&vendor);
        command
            .args(["doctor", "--json"])
            .env("HOME", &pane_home)
            .env("CODEX_HOME", codex_home)
            .current_dir(&cwd);
        for name in CODEX_SPEC.credentials.scrubbed_env {
            command.env_remove(name);
        }
        command.env_remove(fleet_key_env);
        command.env_remove(OPERATORS_SHELL_VAR);
        for (name, value) in env {
            command.env(name, value);
        }
        let out =
            command.output().unwrap_or_else(|e| panic!("could not run {}: {e}", vendor.display()));
        let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
            panic!("`{VENDOR_BIN} doctor --json` stopped emitting JSON ({e}):\n{}", said(&out))
        });
        report["checks"]["auth.credentials"].clone()
    };

    // A provider whose endpoint is loopback and which still requires the vendor's
    // own auth — the shape that lets the two `auth.json` arms be read without the
    // default endpoint being dialled.
    let plan_provider = "[model_providers.operators-plan]\n\
                         name = \"the operator's plan\"\n\
                         base_url = \"http://127.0.0.1:9/v1\"\n\
                         wire_api = \"responses\"\n\
                         requires_openai_auth = true\n";
    let selects_plan = format!("model_provider = \"operators-plan\"\n{plan_provider}");

    // --- shape 1: a subscription plan ---------------------------------------
    //
    // The ChatGPT token set, in the vendor's own `auth.json` shape. The id token
    // is a syntactically real JWT carrying a plan type — the vendor parses it, and
    // a malformed one is reported as `stored credentials could not be read`, which
    // would pass an "is it logged in" assertion written less carefully.
    let plan_auth = format!(
        "{{\"auth_mode\":\"chatgpt\",\"OPENAI_API_KEY\":null,\"tokens\":{{\
           \"id_token\":\"eyJhbGciOiAibm9uZSIsICJ0eXAiOiAiSldUIn0.\
           eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOiB7ImNoYXRncHRfcGxhbl90eXBlIjogInBybyIsICJj\
           aGF0Z3B0X2FjY291bnRfaWQiOiAiYWNjdC0xMjMiLCAiY2hhdGdwdF91c2VyX2lkIjogInVzZXItMSJ9LCAi\
           ZW1haWwiOiAib3BlcmF0b3JAZXhhbXBsZS5jb20ifQ.c2ln\",\
           \"access_token\":\"{SENTINEL}\",\"refresh_token\":\"{SENTINEL}-refresh\",\
           \"account_id\":\"acct-123\"}},\"last_refresh\":\"2026-09-05T00:00:00Z\"}}"
    );
    let plan = installation("operator-plan", &selects_plan, Some(&plan_auth));
    let plan_orch = root.join("plan-orch");
    let plan_worker = root.join("plan-worker-1");
    seed_into(&plan_orch, &plan, true);
    seed_into(&plan_worker, &plan, false);

    let logged_in = auth(&plan_orch, &[]);
    assert_eq!(
        logged_in["status"].as_str(),
        Some("ok"),
        "the operator's own seat is not logged in. A codex login lives inside the directory \
         this seeder replaces (C6), so the seat that does not carry it in has none at all — \
         which is #44: {logged_in}",
    );
    assert_eq!(
        logged_in["details"]["stored auth mode"].as_str(),
        Some("chatgpt"),
        "a subscription plan did not survive the copy: {logged_in}",
    );
    assert_eq!(
        logged_in["details"]["stored ChatGPT tokens"].as_str(),
        Some("true"),
        "the plan's token set did not survive the copy: {logged_in}",
    );

    // The negative control, and the reason the assertion above measures anything:
    // the identical seat with the copied credential taken back out is what #26
    // shipped, and the vendor calls it what it is.
    let without = root.join("plan-orch-no-login");
    seed_into(&without, &plan, true);
    fs::remove_file(without.join("auth.json")).expect("the copied credential is a file");
    let not_logged_in = auth(&without, &[]);
    assert_eq!(
        not_logged_in["status"].as_str(),
        Some("fail"),
        "an orchestrator with no credential file reported healthy auth, so the arm above \
         measured the machine rather than the copy: {not_logged_in}",
    );
    assert!(
        not_logged_in["summary"].as_str().unwrap_or_default().contains("no Codex credentials"),
        "the vendor stopped naming a missing login as one, so the negative control no longer \
         reproduces the failure #44 fixes: {not_logged_in}",
    );

    // --- shape 2: an API key -------------------------------------------------
    let key_auth = format!("{{\"auth_mode\":\"apikey\",\"OPENAI_API_KEY\":\"{SENTINEL}\"}}");
    let keyed = installation("operator-key", &selects_plan, Some(&key_auth));
    let key_orch = root.join("key-orch");
    let key_worker = root.join("key-worker-1");
    seed_into(&key_orch, &keyed, true);
    seed_into(&key_worker, &keyed, false);

    let with_a_key = auth(&key_orch, &[]);
    assert_eq!(
        with_a_key["status"].as_str(),
        Some("ok"),
        "an API-key login did not survive the copy: {with_a_key}",
    );
    assert_eq!(
        with_a_key["details"]["stored auth mode"].as_str(),
        Some("api_key"),
        "the stored key arrived in a shape the vendor does not read as a login: {with_a_key}",
    );

    // --- shape 3: a named custom provider ------------------------------------
    //
    // The operator's own installation is this shape, and it is the one that lives
    // in `config.toml` rather than in `auth.json` — so it is the strike, not the
    // snapshot allowlist, that decides whether it survives.
    let custom = format!(
        "model_provider = \"operators-own\"\n\
         [model_providers.operators-own]\n\
         name = \"the operator's own\"\n\
         base_url = \"http://127.0.0.1:9/\"\n\
         wire_api = \"responses\"\n\
         env_key = \"{OPERATORS_SHELL_VAR}\"\n\
         experimental_bearer_token = \"{SENTINEL}\"\n"
    );
    let third_party = installation("operator-custom", &custom, None);
    let custom_orch = root.join("custom-orch");
    let custom_worker = root.join("custom-worker-1");
    seed_into(&custom_orch, &third_party, true);
    seed_into(&custom_worker, &third_party, false);

    let inherited = auth(&custom_orch, &[(OPERATORS_SHELL_VAR, "sk-the-operators-own")]);
    assert_eq!(
        inherited["details"]["provider auth env var"].as_str(),
        Some(format!("{OPERATORS_SHELL_VAR} (present)").as_str()),
        "the orchestrator's inherited provider lost the variable it authenticates through. \
         That seat is not scrubbed, so the operator's own shell variable is the one the \
         vendor is supposed to follow there (C2 as amended by C9): {inherited}",
    );
    assert_eq!(
        inherited["status"].as_str(),
        Some("ok"),
        "the inherited provider resolved to a name and then would not authenticate: \
         {inherited}",
    );
    // The bearer token is the other spelling of the same shape, and `doctor` has no
    // reading for it — measured: a table with one and a table with none produce the
    // identical `auth.credentials`. So this half is asserted off the document the
    // vendor loaded, and it is stated as what it is rather than dressed up as a
    // vendor reading.
    let orch_document =
        fs::read_to_string(custom_orch.join("config.toml")).expect("the orchestrator's seed");
    assert!(
        orch_document.contains(SENTINEL),
        "the orchestrator's inherited provider was struck of its bearer token:\n{orch_document}",
    );

    // --- and the fence, on all three -----------------------------------------
    //
    // #29's byte scan, at this tier: whatever shape the operator's credential took,
    // none of it is anywhere under a fenced pane's directory.
    for worker in [&plan_worker, &key_worker, &custom_worker] {
        assert!(
            !worker.join("auth.json").exists(),
            "a fenced pane holds the fleet's credential and never the operator's (D-062): {}",
            worker.display(),
        );
        let mut stack = vec![worker.clone()];
        while let Some(at) = stack.pop() {
            for entry in fs::read_dir(&at).expect("a seeded pane directory").flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let bytes = fs::read(&path).unwrap_or_default();
                assert!(
                    !String::from_utf8_lossy(&bytes).contains(SENTINEL),
                    "the operator's credential reached a fenced pane through {}",
                    path.display(),
                );
            }
        }
    }

    fs::remove_dir_all(&root).ok();
}

/// **The write guardrail actually refuses a real codex tool call, in a pane
/// FLEETOR seeded** (#46, C49; D-065, WP-17).
///
/// This is the arm the whole of #31, #45 and #46 exists to make possible. Every
/// piece of it except the model comes from production code:
///
///  - the pane's `CODEX_HOME` is written by `Harness::seed_config_dir`;
///  - the hook, its command line and the ownership of the `hooks` table are
///    written by `Harness::install_guardrail`, from roots this test computes the
///    way `placement::guardrail_notices` does;
///  - the argv — **including the hook-trust bypass** — is whatever
///    `Harness::command_args` composes for a seat FLEETOR drives. If that flag ever
///    stops being emitted, this arm goes red rather than the assertion being
///    edited.
///
/// **The model is `examples/codex-spike/hook_probe.py`** (C13: the probe is suite
/// infrastructure, not a throwaway). It answers the first request of the turn with
/// a canned `exec_command` writing outside the worktree, so a real tool call really
/// happens and the filesystem is the witness. Zero tokens, loopback only.
///
/// **Both binaries, per C47.** This arm measures hooks, flags and config
/// precedence, which is exactly the class where the PATH `codex` and the vendor
/// binary have been observed to disagree absolutely — so it resolves the vendor
/// path and reports both readings, and both must refuse.
///
/// **Two witnesses, because neither alone is enough.** The absent file says the
/// write did not happen; the journal line says *the guardrail* is why. A seatbelt
/// refusal would produce the first and not the second, which is why the probe
/// relaxes `sandbox_mode` for this arm alone — C7's fence has its own.
#[test]
fn a_fleet_seeded_codex_worker_is_refused_a_write_outside_its_worktree() {
    use fleetor_shell::guardrail::GuardrailPlacement;
    use fleetor_shell::placement::codex::{codex, OPERATOR_DIR};
    use fleetor_shell::placement::Seed;
    use fleetor_shell::placement::harness::Seat;
    const HOOK_PROBE: &str = "examples/codex-spike/hook_probe.py";
    const BYPASS: &str = "--dangerously-bypass-hook-trust";

    let root = repo_root();
    let probe = root.join(HOOK_PROBE);
    assert!(probe.is_file(), "the instrument is committed infrastructure: {}", probe.display());

    // **The two readings C47 requires**, resolved rather than assumed: whatever
    // `codex` means on PATH, and the vendor binary behind it. On a machine where
    // they are the same file this measures it twice and says so.
    let vendor_abs = Path::new(VENDOR_ABSOLUTE).to_path_buf();
    // `true` marks the reading taken through the resolved vendor binary — the one
    // the negative control below can attribute a refusal on. A wrapper cannot be
    // attributed on, and C48 measured why.
    let mut binaries: Vec<(PathBuf, bool)> = Vec::new();
    if vendor_abs.is_file() {
        binaries.push((vendor_abs.clone(), true));
    }
    if let Some(on) = on_path(VENDOR_BIN) {
        if !binaries.iter().any(|(p, _)| *p == on) {
            let is_vendor = binaries.is_empty();
            binaries.push((on, is_vendor));
        }
    }
    if binaries.is_empty() {
        announce(&[
            format!("SKIPPED: the codex write-guardrail arm — no `{VENDOR_BIN}` to drive."),
            format!("  Recorded against {RECORDED_BUILD}. Nothing below was measured:"),
            "    the-guardrail-refuses-a-real-tool-call, and-files-the-journal-line,".into(),
            "    under-both-the-vendor-binary-and-the-PATH-codex (#46, C47, C49).".into(),
            "  The in-crate tests still prove FLEETOR owns the pane's hooks table and".into(),
            "  emits the bypass. Only the vendor can say whether the hook then fires.".into(),
        ]);
        return;
    }
    let Some(python) = on_path(INTERPRETER) else {
        announce(&["SKIPPED: the codex write-guardrail arm — no `python3` to be the model.".into()]);
        return;
    };

    for (binary, is_vendor) in binaries {
        let scratch = std::env::temp_dir().join(scratch_name("guardrail"));
        let operator_home = scratch.join("operator");
        let cwd = scratch.join("worktree");
        let shell = scratch.join("_shell");
        let seeded = scratch.join("pane-config").join("worker-1");
        let outside = scratch.join("outside").join("loot.txt");
        for dir in [&operator_home.join(OPERATOR_DIR), &cwd, &shell, &seeded] {
            fs::create_dir_all(dir).expect("the scratch layout");
        }
        fs::create_dir_all(outside.parent().expect("a parent")).expect("a target outside");

        // **The operator brings a hook of their own**, which is the case C49 turns
        // on: a merge would have carried it in, and the bypass would then have run
        // it unverified inside a fenced worker. It must not be in the seeded
        // document at all.
        let theirs = scratch.join("theirs-ran");
        fs::write(
            operator_home.join(OPERATOR_DIR).join("config.toml"),
            format!(
                "[[hooks.PreToolUse]]\n\
                 [[hooks.PreToolUse.hooks]]\n\
                 type = \"command\"\n\
                 command = \"/usr/bin/touch {}\"\n",
                theirs.display(),
            ),
        )
        .expect("the operator's own config");

        let seed = Seed::new(&seeded, &cwd, Some(&operator_home)).with_brief("do the work");
        codex().seed_config_dir(&seed).expect("seeding a worker's CODEX_HOME");

        let journal = fleetor_shell::guardrail::journal_path(&shell);
        let roots = fleetor_shell::guardrail::roots_for(&cwd, &shell, &[]);
        codex()
            .install_guardrail(&GuardrailPlacement {
                operators_own_seat: false,
                pane: fleetor_core::pane::PaneId::Worker(1),
                config_dir: &seeded,
                roots: &roots,
                policy: &fleetor_shell::guardrail::policy_dir(&shell),
                journal: &journal,
            })
            .expect("installing a worker's guardrail");

        // The argv is production's, and the flag being in it is the half that makes
        // the hook run at all.
        let argv = codex().command_args(
            &Seat::new("do the work").with_permission_mode("never"),
        );
        assert!(
            argv.iter().any(|a| a == BYPASS),
            "a fenced codex seat's argv carries the hook-trust bypass: {argv:?}",
        );

        let drive = |args: &[String]| {
            let mut command = Command::new(&python);
            command.arg(&probe).arg("--fleet-seeded");
            command.args(["--home", &seeded.to_string_lossy()]);
            command.args(["--cwd", &cwd.to_string_lossy()]);
            command.args(["--outside", &outside.to_string_lossy()]);
            command.args(["--binary", &binary.to_string_lossy()]);
            for arg in args {
                command.args(["--arg", arg]);
            }
            let out = command
                .current_dir(&root)
                .output()
                .unwrap_or_else(|e| panic!("could not run the probe: {e}"));
            let said = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr),
            );
            let reading =
                String::from_utf8_lossy(&out.stdout).lines().last().unwrap_or_default().to_string();
            (reading, said)
        };

        let (reading, said) = drive(&argv);

        // The probe has to have been the model, or the arm proved nothing: a turn
        // that never asked for a tool call cannot be refused one.
        assert!(
            reading.contains("\"model_was_asked\": true"),
            "the loopback model was never reached, so no tool call happened under {}:\n{said}",
            binary.display(),
        );

        // **Witness one: the write did not land.**
        assert!(
            !outside.exists(),
            "a fleet-spawned codex worker wrote outside its worktree under {}.\n{said}",
            binary.display(),
        );
        assert!(
            reading.contains("\"wrote_outside\": false"),
            "the probe's own reading disagrees under {}:\n{said}",
            binary.display(),
        );

        // **Witness two: the guardrail is why**, and it reached the Activity feed.
        // Without this a seatbelt refusal would pass witness one.
        let lines = fs::read_to_string(&journal).unwrap_or_default();
        let refused = lines.lines().find(|l| l.contains("\"pane\"")).unwrap_or_else(|| {
            panic!(
                "no refusal reached the journal under {} — the write did not land, but \
                 nothing says the guardrail is why.\n{said}",
                binary.display(),
            )
        });
        assert!(refused.contains("worker-1"), "the journal line names the pane: {refused}");

        // **And the operator's own hook did not run**, because it is not in the
        // document the bypass trusted. This is C49's whole claim, at the vendor.
        assert!(
            !theirs.exists(),
            "an operator's own hook executed inside a fenced worker under {} — the \
             bypass widened the pane instead of narrowing it, which is the outcome \
             C49 says must not happen",
            binary.display(),
        );

        // --- the negative control, which is the load-bearing arm -----------------
        //
        // **Everything above passes on a machine where the tool call never
        // happened.** So the identical pane is driven once more with the bypass
        // taken out of the argv and nothing else changed: the hook is then
        // untrusted, codex skips it silently (C48), and the same write must land.
        // Without this, "the file is not there" is not evidence about the
        // guardrail — it is evidence about nothing in particular.
        let without: Vec<String> = argv.iter().filter(|a| *a != BYPASS).cloned().collect();
        let (control, control_said) = drive(&without);
        let control_wrote = control.contains("\"wrote_outside\": true") && outside.exists();
        let attribution = if is_vendor {
            assert!(
                control_wrote,
                "the negative control did not write either, so this arm is measuring the \
                 instrument rather than the guardrail, under {}:\n{control_said}",
                binary.display(),
            );
            format!("negative control without {BYPASS}: the same write landed")
        } else if control_wrote {
            format!("negative control without {BYPASS}: the same write landed")
        } else {
            // **C47, and it is the case that rule exists for.** This binary is a
            // wrapper, and C48 read the wrapper's own arg stream: it injects
            // `--dangerously-bypass-hook-trust` alongside its `-c hooks.*` flags. So
            // removing *our* copy changes nothing here and the control cannot
            // discriminate. The refusal above is real and is reported; it is
            // attributed on the vendor binary, which is the other reading.
            format!(
                "negative control inconclusive: this binary is a wrapper that injects \
                 {BYPASS} of its own (C47, C48), so the refusal is attributed on {VENDOR_ABSOLUTE}"
            )
        };
        fs::remove_file(&outside).ok();

        announce(&[format!(
            "codex write guardrail: REFUSED an out-of-worktree write under {}\n  ({attribution})",
            binary.display(),
        )]);
        fs::remove_dir_all(&scratch).ok();
    }
}

/// **The gate's own probe, run against the real binary — three auth shapes, a live
/// model list and the posture codex resolved** (WP-25 phase 3, #34; C8, C14, C47).
///
/// The arm above proves the vendor *reports* three distinct credential shapes. This
/// one proves the code the operator's gate actually runs reads all three as a
/// login, and it runs `placement::codex::diagnose` itself rather than a hand-typed
/// `doctor` invocation — so a probe that stopped parsing one of the vendor's
/// readings fails here instead of turning into a silently narrower gate.
///
/// **Nothing here touches the operator's installation.** Each shape is a fabricated
/// `~/.codex` seeded into a scratch `CODEX_HOME` through
/// `Harness::seed_config_dir`, and `Installation` is how the identical production
/// function is pointed at it — the same move `Layout::under` makes for a placement.
///
/// **The negative control is `env_key`-shaped on purpose.** #29 measured that a
/// selected provider naming an absent `env_key` fails *even with `CODEX_API_KEY`
/// present in the environment* — there is no fallback from a named variable to an
/// ambient one. A control built on a missing `auth.json` instead would pass on a
/// developer's machine and fail on a CI runner that happens to export a key, which
/// is a control that measures the machine rather than the code.
///
/// **Both readings, per C47**: the probe is handed the `codex` on PATH — a wrapper
/// that injects flags on the machine this was written on — and resolves the vendor
/// binary out of the report's own `runtime.provenance`. This is a measurement of
/// resolved configuration, which is exactly the class where #31 found the two
/// disagreeing absolutely.
///
/// Zero tokens, and zero non-loopback traffic: every fabricated provider is
/// `127.0.0.1`, so `doctor`'s reachability probe dials nothing real. About 4 s.
#[test]
fn the_gates_probe_reads_all_three_auth_shapes_the_vendor_reports() {
    use fleetor_shell::placement::codex::{codex, diagnose, Installation, OPERATOR_DIR};
    use fleetor_shell::placement::harness::{AccountShape, LoginState};
    use fleetor_shell::placement::Seed;

    let Some(vendor) = on_path(VENDOR_BIN) else {
        announce(&[
            format!("SKIPPED: the codex gate-probe arm — `{VENDOR_BIN}` is not on PATH."),
            format!("  Recorded against {RECORDED_BUILD}. Nothing below was measured:"),
            "    plan-reads-as-a-login, api-key-reads-as-a-login,".into(),
            "    custom-provider-reads-as-a-login, no-credential-is-the-one-refusal,".into(),
            "    the-model-list-is-the-vendors-own, the-posture-is-read-back (#34, C8, C14)".into(),
            "  The in-crate tests still prove the probe reads each recorded reading as".into(),
            "  the shape it is. Only the vendor can say it still emits them.".into(),
        ]);
        return;
    };

    let root = std::env::temp_dir().join(scratch_name("gate"));
    let pane_home = root.join("pane-home");
    let cwd = root.join("worktree");
    fs::create_dir_all(&pane_home).expect("a private HOME");
    fs::create_dir_all(&cwd).expect("a pane worktree");

    // A fabricated operator installation, seeded into an orchestrator's own
    // `CODEX_HOME` — the seat C9 says carries the operator's login.
    let seat = |name: &str, config: &str, auth: Option<&str>| -> PathBuf {
        let operator = root.join(format!("{name}-operator"));
        let dir = operator.join(OPERATOR_DIR);
        fs::create_dir_all(&dir).expect("a fabricated operator installation");
        fs::write(dir.join("config.toml"), config).expect("operator config");
        if let Some(body) = auth {
            fs::write(dir.join("auth.json"), body).expect("operator credential");
        }
        let seeded = root.join(name);
        codex()
            .seed_config_dir(
                &Seed::new(&seeded, &cwd, Some(&operator))
                    .with_brief("a pane's brief")
                    .for_the_operator(),
            )
            .expect("seeding an orchestrator's CODEX_HOME");
        seeded
    };
    let probe = |config_dir: &Path| {
        diagnose(&Installation {
            binary: Some(&vendor),
            config_dir: Some(config_dir),
            home: Some(&pane_home),
            cwd: Some(&cwd),
        })
    };

    // A loopback provider that still requires the vendor's own auth, so the two
    // `auth.json` shapes are read without the real endpoint being dialled.
    let plan_provider = "model_provider = \"operators-plan\"\n\
                         [model_providers.operators-plan]\n\
                         name = \"the operator's plan\"\n\
                         base_url = \"http://127.0.0.1:9/v1\"\n\
                         wire_api = \"responses\"\n\
                         requires_openai_auth = true\n";

    // --- shape 1: a subscription plan ---------------------------------------
    let plan_auth = "{\"auth_mode\":\"chatgpt\",\"OPENAI_API_KEY\":null,\"tokens\":{\
                       \"id_token\":\"eyJhbGciOiAibm9uZSIsICJ0eXAiOiAiSldUIn0.\
                       eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOiB7ImNoYXRncHRfcGxhbl90eXBlIjogInBybyJ9fQ.c2ln\",\
                       \"access_token\":\"sk-plan\",\"refresh_token\":\"sk-plan-refresh\",\
                       \"account_id\":\"acct-123\"},\"last_refresh\":\"2026-09-05T00:00:00Z\"}";
    let plan = probe(&seat("plan", plan_provider, Some(plan_auth)));
    assert!(
        matches!(plan.login, LoginState::LoggedIn(AccountShape::SubscriptionPlan { .. })),
        "a subscription plan must count as logged in (C14). The orchestrator is the only \
         codex seat that spends the operator's credential (C9), and refusing a plan there \
         is story 9 — a supported feature looking unimplemented — at the gate: {:?}",
        plan.login,
    );

    // --- shape 2: an API key -------------------------------------------------
    let key_auth = "{\"auth_mode\":\"apikey\",\"OPENAI_API_KEY\":\"sk-stored\"}";
    let keyed = probe(&seat("key", plan_provider, Some(key_auth)));
    assert_eq!(
        keyed.login,
        LoginState::LoggedIn(AccountShape::ApiKey),
        "a stored API key must count as logged in (C14)",
    );

    // --- shape 3: a named custom provider ------------------------------------
    //
    // The operator's own installation is this shape, which is why a probe handling
    // only the first two would pass everywhere except on their machine.
    // **`PATH` is the variable the entry names, and it is a deliberate choice.**
    // The reading under test is the vendor's `provider auth env var = "X (present)"`,
    // which needs a variable that really is in the environment the probe inherits —
    // and that environment is the orchestrator seat's own shape, unscrubbed,
    // because the operator's shell variable is the one the vendor is supposed to
    // follow there (C9). Setting a variable of this test's own would mean mutating
    // the process environment while `cargo test` runs other tests on other threads,
    // which is unsound rather than merely untidy. `PATH` is present on every machine
    // that can run this file at all, and codex treats a named `env_key` as opaque —
    // #29 measured that it never falls back to an ambient auth variable, so nothing
    // about the *name* changes what is being measured.
    const OPERATORS_VAR: &str = "PATH";
    /// The same entry naming a variable that is absent — the refusal control.
    const ABSENT_VAR: &str = "FLEETOR_GATE_PROBE_KEY_34";
    let custom_config = |env_key: &str| {
        format!(
            "model_provider = \"operators-own\"\n\
             [model_providers.operators-own]\n\
             name = \"the operator's own\"\n\
             base_url = \"http://127.0.0.1:9/\"\n\
             wire_api = \"responses\"\n\
             env_key = \"{env_key}\"\n"
        )
    };
    let named = probe(&seat("custom", &custom_config(OPERATORS_VAR), None));
    assert_eq!(
        named.login,
        LoginState::LoggedIn(AccountShape::CustomProvider {
            name: "the operator's own".to_string(),
            env_var: Some(OPERATORS_VAR.to_string()),
        }),
        "a named custom provider must count as logged in and be named (C2, C9, C14)",
    );
    assert_eq!(
        named.provider.as_deref(),
        Some("the operator's own"),
        "the provider is a fact displayed beside the model (C2): {:?}",
        named.provider,
    );

    // --- the one refusal -----------------------------------------------------
    //
    // The same entry with its variable absent. #29 measured that a selected
    // `env_key` has no fallback to an ambient auth variable, which is what makes
    // this control independent of whatever the machine running it exports.
    let refused = probe(&seat("refused", &custom_config(ABSENT_VAR), None));
    assert!(
        matches!(refused.login, LoginState::NoCredential { .. }),
        "codex resolved no usable credential and the gate did not refuse. That is the \
         one refusal C14 leaves, and without it the three arms above would pass on a \
         probe that answered `logged in` unconditionally: {:?}",
        refused.login,
    );
    assert_eq!(refused.login.caveat(), None, "nothing to caveat about a refusal");

    // --- what the gate shows beside the shape --------------------------------
    assert!(
        !named.models.is_empty(),
        "the model list is the vendor's own catalog resolution and it came back empty, \
         so a picker would offer nothing (C2)",
    );
    assert!(
        named.models.iter().all(|m| !m.slug.is_empty() && !m.display_name.is_empty()),
        "every option needs the slug the argv takes and the name a human reads: {:?}",
        named.models,
    );
    assert_eq!(
        named.posture.filesystem.as_deref(),
        Some("restricted"),
        "the containment the vendor *resolved*, not the keys FLEETOR wrote (C8): {:?}",
        named.posture,
    );
    assert!(named.posture.network.is_some() && named.posture.approval.is_some(), "{:?}", named.posture);
    assert!(
        named.version.as_deref().is_some_and(|v| RECORDED_BUILD.ends_with(v)),
        "the build every reading here was recorded against is {RECORDED_BUILD}; the probe \
         read {:?}. Re-measure before moving the constant.",
        named.version,
    );

    // --- both readings, per C47 ----------------------------------------------
    let resolved = named.resolved.clone().unwrap_or_else(|| {
        panic!(
            "the probe did not resolve a vendor binary behind `{}`. C47's rule is that an \
             arm reading resolved configuration reports both readings, and this one now \
             has only the wrapper's.",
            named.invoked,
        )
    });
    assert!(resolved.is_file(), "{} is not a file", resolved.display());
    assert!(
        named.readings_agree,
        "the `{}` on PATH and the vendor binary at {} report different configurations. \
         That is C47's case and the operator is told about it — but it also means every \
         reading above describes the wrapper rather than what a pane runs under, so the \
         arm is measuring the wrong binary.",
        named.invoked,
        resolved.display(),
    );

    // --- and the caveat reaches the operator ---------------------------------
    let lines = named.notices();
    assert!(
        lines.iter().any(|(level, text)| {
            *level == fleetor_core::event::NoticeLevel::Warn
                && text.contains(fleetor_shell::placement::harness::REACHABILITY_NOT_AUTHORIZATION)
        }),
        "the reachability-not-authorization caveat did not reach the feed. `doctor` \
         returned HTTP 401 against a live provider and still counted as reachable, so a \
         revoked key clears this gate and fails on turn one — the operator has to be told \
         that, not have it recorded in a decision file: {lines:#?}",
    );

    fs::remove_dir_all(&root).ok();
}
