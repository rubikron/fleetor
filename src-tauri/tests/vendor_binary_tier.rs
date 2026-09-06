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

    // **This file too, since #27.** The brief-carrier arm stands up its own
    // loopback provider in Rust rather than shelling out to the probe, so the
    // property has to be checked where the URL now lives as well.
    let here = fs::read_to_string(root.join(THIS_FILE)).expect("this tier reads its own source");

    let offenders: Vec<&str> = src
        .split("://")
        .skip(1)
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

    let Some(vendor) = on_path(VENDOR_BIN) else {
        announce(&[
            format!("SKIPPED: the codex config-seeding arm — `{VENDOR_BIN}` is not on PATH."),
            format!("  Recorded against {RECORDED_BUILD}. Nothing below was measured:"),
            "    seeded-config-loads, tilde-trap-reproduces (#26, C6)".into(),
            "  The in-crate tests still prove what FLEETOR wrote. They cannot prove".into(),
            "  the vendor accepts it — that is what this arm is for.".into(),
        ]);
        return;
    };

    let root = std::env::temp_dir().join(format!(
        "fleetor-codex-vendor-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("a clock")
            .as_millis()
    ));
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

    let Some(vendor) = on_path(VENDOR_BIN) else {
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

    let root = std::env::temp_dir().join(format!(
        "fleetor-codex-brief-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("a clock")
            .as_millis()
    ));
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

    let Some(vendor) = on_path(VENDOR_BIN) else {
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

    let root = std::env::temp_dir().join(format!(
        "fleetor-codex-fence-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("a clock")
            .as_millis()
    ));
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
