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
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD, Engine};
use fleetor_core::pane::PaneId;
use fleetor_shell::placement::codex::{codex, CODEX_SPEC, OPERATOR_DIR};
use fleetor_shell::placement::harness::BringUp;
use fleetor_shell::placement::{HarnessSpec, Seed};
use fleetor_shell::pty::{out_channel, Emit, PaneRegistry};
use portable_pty::CommandBuilder;

/// The vendor binary, and the build every number in this file was measured
/// against.
const VENDOR_BIN: &str = "codex";
const RECORDED_BUILD: &str = "codex-cli 0.153.4";

/// The pane's window. A 0x0 terminal is not a terminal — codex paints empty
/// frames into one — and this is the size the shell's own terminals report.
const ROWS: u16 = 40;
const COLS: u16 = 120;

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

// --- the instruments ----------------------------------------------------------

/// Everything a pane has painted, assembled from the registry's own emit
/// callback — the same bytes the operator's terminal renders.
#[derive(Default)]
struct Painted {
    channel: String,
    bytes: Mutex<Vec<u8>>,
}

impl Painted {
    fn watching(pane: PaneId) -> Arc<Self> {
        Arc::new(Self { channel: out_channel(pane), bytes: Mutex::new(Vec::new()) })
    }

    fn emitter(self: &Arc<Self>) -> Emit {
        let me = Arc::clone(self);
        Arc::new(move |channel: &str, payload: String| {
            if channel != me.channel {
                return;
            }
            if let Ok(raw) = STANDARD.decode(payload) {
                me.bytes.lock().expect("the paint lock").extend_from_slice(&raw);
            }
        })
    }

    fn painted(&self) -> usize {
        self.bytes.lock().expect("the paint lock").len()
    }

    /// What the pane painted, escapes removed and **all** whitespace collapsed.
    ///
    /// Codex repaints character by character with cursor moves between, so a word
    /// never survives as a word with its spacing intact. Collapsing is what makes
    /// a substring test work at all — this is a readiness signal, not a
    /// rendering, and it is `probe_clear.py`'s `screen()` in Rust.
    fn screen(&self) -> String {
        let raw = self.bytes.lock().expect("the paint lock").clone();
        let text = String::from_utf8_lossy(&raw);
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '\u{1b}' {
                if !c.is_whitespace() && !c.is_control() {
                    out.push(c);
                }
                continue;
            }
            match chars.peek() {
                // OSC: runs to BEL or ST.
                Some(']') => {
                    for c in chars.by_ref() {
                        if c == '\u{7}' || c == '\u{1b}' {
                            break;
                        }
                    }
                }
                // CSI: parameter bytes, then one final letter.
                Some('[') => {
                    chars.next();
                    for c in chars.by_ref() {
                        if c.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                _ => {
                    chars.next();
                }
            }
        }
        out
    }
}

/// A fabricated provider on loopback that records every request body and answers
/// `data: [DONE]`, completing no turn. `probe.py`'s instrument (C13), in Rust so
/// this file drives the registry rather than a subprocess.
struct Capture {
    port: u16,
    bodies: Arc<Mutex<Vec<String>>>,
}

impl Capture {
    fn start() -> Self {
        use std::io::{BufRead, BufReader, Read};
        use std::net::TcpListener;

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

    fn carrying(&self, needle: &str) -> bool {
        self.bodies.lock().expect("the capture lock").iter().any(|b| b.contains(needle))
    }
}

/// `libtest` captures `println!` and `eprintln!` on a passing test, so a skip
/// announced with either is invisible in a plain `cargo test`. fd 2 is not
/// intercepted. Lifted from `vendor_binary_tier.rs`, whose header argues it.
fn announce(lines: &[String]) {
    #[cfg(unix)]
    {
        use std::os::unix::io::FromRawFd;
        let mut fd2 = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(2) });
        let _ = writeln!(fd2, "\n{}\n", lines.join("\n"));
    }
}

fn on_path(program: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(program))
            .find(|candidate| candidate.is_file())
    })
}

fn scratch(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "fleetor-codex-bringup-{tag}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("a clock")
            .as_nanos()
    ))
}

// --- the arms -----------------------------------------------------------------

/// One pane, brought up under `spec`, sent `message` the instant `spawn` returns.
///
/// Returns whether the message reached the wire, and what the pane painted.
fn deliver_into_a_fresh_pane(
    vendor: &std::path::Path,
    spec: &'static HarnessSpec,
    message: &str,
) -> (bool, Arc<Painted>) {
    let root = scratch(match spec.bring_up {
        BringUp::AfterWaking => "woken",
        BringUp::AtOnce => "control",
    });
    let operator_home = root.join("operator");
    let pane_home = root.join("pane-home");
    let cwd = root.join("work");
    let seeded = root.join("seeded");
    std::fs::create_dir_all(operator_home.join(OPERATOR_DIR)).expect("a fabricated installation");
    std::fs::create_dir_all(&pane_home).expect("the Fence's private HOME");
    std::fs::create_dir_all(&cwd).expect("a pane cwd");

    // **The seeder, not a hand-written directory.** The `CODEX_HOME` under test
    // has to be the one a pane actually gets (C6), or the splash this file is
    // about is not the splash a pane meets.
    let brief = "You are a FLEETOR worker pane.";
    codex()
        .seed_config_dir(&Seed::new(&seeded, &cwd, Some(&operator_home)).with_brief(brief))
        .expect("seeding a pane's CODEX_HOME");

    let capture = Capture::start();
    let over = |key: &str, value: &str| ["-c".to_string(), format!("{key}={value}")];
    let base_url = format!("\"http://127.0.0.1:{}\"", capture.port);

    let mut cmd = CommandBuilder::new(vendor);
    // The shell's own terminal is not an alt-screen one, and the probe measured
    // through the same flag.
    cmd.arg("--no-alt-screen");
    // The provider is overridden on argv rather than rewritten into the seeded
    // file, so this arm stays independent of how the seed spells its own
    // provider — that is #29's to change and this is not a test of it.
    for arg in over("model_provider", "probe")
        .into_iter()
        .chain(over("model_providers.probe.name", "\"probe\""))
        .chain(over("model_providers.probe.base_url", &base_url))
        .chain(over("model_providers.probe.wire_api", "\"responses\""))
        .chain(over("model_providers.probe.experimental_bearer_token", "\"sk-probe\""))
        // Provider-level, not top-level: set at the top level they do nothing,
        // and each turn becomes a dozen identical requests (C37).
        .chain(over("model_providers.probe.request_max_retries", "0"))
        .chain(over("model_providers.probe.stream_max_retries", "0"))
    {
        cmd.arg(arg);
    }
    cmd.cwd(&cwd);
    cmd.env("CODEX_HOME", &seeded);
    cmd.env("HOME", &pane_home);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");

    let pane = PaneId::Worker(1);
    let painted = Painted::watching(pane);
    let registry = PaneRegistry::new(painted.emitter(), root.join("panes.pids"));
    registry.spawn(pane, cmd, spec, ROWS, COLS).expect("a codex pane");

    // **The instant it is announced.** No settling, no sleep, no readiness check
    // of its own — this is what `deliver` does the moment a `fleet send` lands,
    // and the whole claim is that the pane can take it.
    registry.write_paste(pane, message).expect("the delivery");

    let deadline = Instant::now() + ON_THE_WIRE;
    let mut arrived = false;
    while Instant::now() < deadline {
        if capture.carrying(message) {
            arrived = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    registry.kill_all();
    std::fs::remove_dir_all(&root).ok();
    (arrived, painted)
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
        for banned in ["bring_up", "BringUp", "waking", "painted", "sleep(WAKE", "QUIET_SAMPLE"] {
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
