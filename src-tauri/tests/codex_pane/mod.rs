//! **One real codex pane on a real pty, brought up the way the shell brings one
//! up** — the shared instrument behind `tests/codex_bringup.rs` (#42) and
//! `tests/codex_typing.rs` (#32).
//!
//! Both files ask a question about bytes reaching a live vendor pane, and both
//! need the same four things: a `CODEX_HOME` written by codex's *own* seeder
//! (C6), a fabricated provider on loopback that records every request body and
//! completes no turn (C13), the pane spawned through `PaneRegistry::spawn` rather
//! than through a pty of the file's own, and the delivery made with the exact
//! call `deliver` makes when a `fleet send` arrives.
//!
//! **Nothing here reimplements the path it is measuring.** That is #42's rule and
//! it is why this is a shared module rather than a second copy: a finding that
//! only reproduces inside the instrument is a finding about the instrument, and
//! two instruments that drifted apart would let one file's control vouch for the
//! other file's arm.
//!
//! ## Zero tokens
//!
//! [`Capture`] answers `data: [DONE]`, so no completion is ever requested of a
//! real endpoint and every assertion above holds on a machine with no network.
//! #38 is the only ticket authorized to spend money.
//!
//! ## Which `codex` this resolves, and why that is enough here (C47)
//!
//! [`on_path`] takes the first `codex` on `PATH`, which on this machine is a cmux
//! shim that injects flags. C47's rule — resolve the vendor binary absolutely and
//! report both readings — binds an arm that measures **flags or config
//! precedence**, because a shim that injects a flag is measuring the shim's
//! opinion instead of the vendor's. Neither caller measures a flag: what a
//! bracketed paste and a carriage return do to a composer is a property of the
//! TUI's input handling, which every spelling of the binary shares.
//!
//! **Both readings were taken rather than that being left as reasoning** (#32):
//! `tests/codex_typing.rs` passes identically through the shim and through
//! `/opt/homebrew/bin/codex` resolved absolutely, in the same 7.6 s. So `on_path`
//! stays, because a typing arm that pinned an absolute path would stop running on
//! a machine that installs codex somewhere else, and the skip is the loud kind.
//! Both callers record the build they were measured against and say so on a skip.

#![allow(dead_code)] // each caller uses a subset; the instrument is one piece

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD, Engine};
use fleetor_core::pane::PaneId;
use fleetor_shell::placement::codex::{codex, OPERATOR_DIR};
use fleetor_shell::placement::{HarnessSpec, Seed};
use fleetor_shell::pty::{out_channel, Emit, PaneRegistry};
use portable_pty::CommandBuilder;

/// The vendor binary, and the build every number in either caller was measured
/// against.
pub const VENDOR_BIN: &str = "codex";
pub const RECORDED_BUILD: &str = "codex-cli 0.153.4";

/// The pane's window. A 0x0 terminal is not a terminal — codex paints empty
/// frames into one — and this is the size the shell's own terminals report.
pub const ROWS: u16 = 40;
pub const COLS: u16 = 120;

/// The pane a caller's message is delivered into.
pub const PANE: PaneId = PaneId::Worker(1);

// --- the instruments ----------------------------------------------------------

/// Everything a pane has painted, assembled from the registry's own emit
/// callback — the same bytes the operator's terminal renders.
#[derive(Default)]
pub struct Painted {
    channel: String,
    bytes: Mutex<Vec<u8>>,
}

impl Painted {
    pub fn watching(pane: PaneId) -> Arc<Self> {
        Arc::new(Self { channel: out_channel(pane), bytes: Mutex::new(Vec::new()) })
    }

    pub fn emitter(self: &Arc<Self>) -> Emit {
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

    pub fn painted(&self) -> usize {
        self.bytes.lock().expect("the paint lock").len()
    }

    /// What the pane painted, escapes removed and **all** whitespace collapsed.
    ///
    /// Codex repaints character by character with cursor moves between, so a word
    /// never survives as a word with its spacing intact. Collapsing is what makes
    /// a substring test work at all — this is a readiness signal, not a
    /// rendering, and it is `probe_clear.py`'s `screen()` in Rust.
    pub fn screen(&self) -> String {
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
/// its callers drive the registry rather than a subprocess.
pub struct Capture {
    port: u16,
    bodies: Arc<Mutex<Vec<String>>>,
}

impl Capture {
    pub fn start() -> Self {
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

    pub fn carrying(&self, needle: &str) -> bool {
        self.bodies.lock().expect("the capture lock").iter().any(|b| b.contains(needle))
    }

    pub fn requests(&self) -> usize {
        self.bodies.lock().expect("the capture lock").len()
    }

    /// The first request body carrying `needle`.
    ///
    /// What makes "one paste, one turn" assertable: a body is one request, so a
    /// multi-line message that arrived as N turns puts its lines in N bodies and
    /// the first of them carries only the first line.
    pub fn body_carrying(&self, needle: &str) -> Option<String> {
        self.bodies
            .lock()
            .expect("the capture lock")
            .iter()
            .find(|b| b.contains(needle))
            .cloned()
    }
}

/// `libtest` captures `println!` and `eprintln!` on a passing test, so a skip
/// announced with either is invisible in a plain `cargo test`. fd 2 is not
/// intercepted. Lifted from `vendor_binary_tier.rs`, whose header argues it.
pub fn announce(lines: &[String]) {
    #[cfg(unix)]
    {
        use std::os::unix::io::FromRawFd;
        let mut fd2 = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(2) });
        let _ = writeln!(fd2, "\n{}\n", lines.join("\n"));
    }
}

pub fn on_path(program: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(program))
            .find(|candidate| candidate.is_file())
    })
}

pub fn scratch(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "fleetor-codex-pane-{tag}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("a clock")
            .as_nanos()
    ))
}

// --- one pane -------------------------------------------------------------------

/// A live codex pane, its capture server, and everything it has painted.
///
/// Constructed by [`Self::brought_up`], which returns **the instant `spawn`
/// returns** — no settling, no sleep, no readiness check of the caller's own.
/// That is what `deliver` sees the moment a `fleet send` lands, and the whole
/// claim of both callers is about what the pane can take at that instant.
pub struct CodexPane {
    pub painted: Arc<Painted>,
    pub capture: Capture,
    registry: PaneRegistry,
    root: PathBuf,
}

impl CodexPane {
    /// Seed a `CODEX_HOME`, start a capture server, and bring one pane up under
    /// `spec`.
    ///
    /// **The seeder, not a hand-written directory.** The `CODEX_HOME` under test
    /// has to be the one a pane actually gets (C6), or neither caller is
    /// measuring the pane a fleet places.
    pub fn brought_up(vendor: &Path, spec: &'static HarnessSpec, tag: &str) -> Self {
        let root = scratch(tag);
        let operator_home = root.join("operator");
        let pane_home = root.join("pane-home");
        let cwd = root.join("work");
        let seeded = root.join("seeded");
        std::fs::create_dir_all(operator_home.join(OPERATOR_DIR))
            .expect("a fabricated installation");
        std::fs::create_dir_all(&pane_home).expect("the Fence's private HOME");
        std::fs::create_dir_all(&cwd).expect("a pane cwd");

        let brief = "You are a FLEETOR worker pane.";
        codex()
            .seed_config_dir(&Seed::new(&seeded, &cwd, Some(&operator_home)).with_brief(brief))
            .expect("seeding a pane's CODEX_HOME");

        let capture = Capture::start();
        let over = |key: &str, value: &str| ["-c".to_string(), format!("{key}={value}")];
        let base_url = format!("\"http://127.0.0.1:{}\"", capture.port);

        let mut cmd = CommandBuilder::new(vendor);
        // The shell's own terminal is not an alt-screen one, and the probe
        // measured through the same flag.
        cmd.arg("--no-alt-screen");
        // The provider is overridden on argv rather than rewritten into the
        // seeded file, so these arms stay independent of how the seed spells its
        // own provider — that is #29's to change and this is not a test of it.
        for arg in over("model_provider", "probe")
            .into_iter()
            .chain(over("model_providers.probe.name", "\"probe\""))
            .chain(over("model_providers.probe.base_url", &base_url))
            .chain(over("model_providers.probe.wire_api", "\"responses\""))
            .chain(over("model_providers.probe.experimental_bearer_token", "\"sk-probe\""))
            // Provider-level, not top-level: set at the top level they do
            // nothing, and each turn becomes a dozen identical requests (C37).
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

        let painted = Painted::watching(PANE);
        let registry = PaneRegistry::new(painted.emitter(), root.join("panes.pids"));
        registry.spawn(PANE, cmd, spec, ROWS, COLS).expect("a codex pane");

        Self { painted, capture, registry, root }
    }

    /// Deliver one message, with **the exact call `deliver` makes** when a
    /// `fleet send` arrives — framing, submit byte and gap all read off the
    /// pane's own spec.
    pub fn deliver(&self, message: &str) -> Result<(), String> {
        self.registry.write_paste(PANE, message)
    }

    /// Type one slash command, with the exact call `deliver` makes for a
    /// `fleet cmd` — the spelling looked up in the same spec (checkpoint 10).
    pub fn command(&self, command: &str) -> Result<(), String> {
        self.registry.write_command(PANE, command)
    }

    /// Poll until `needle` shows up in a request body, or `within` elapses.
    ///
    /// **A budget for the measurement, not a timeout on anything the product
    /// does.** Nothing in `src/` waits on this; it is how long a caller is
    /// willing to sit before it concludes a message never arrived.
    pub fn reached_the_wire(&self, needle: &str, within: Duration) -> bool {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            if self.capture.carrying(needle) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        false
    }

    /// Poll until the pane has painted `needle`, or `within` elapses.
    ///
    /// The screen is [`Painted::screen`]'s collapsed reading, so `needle` must be
    /// whitespace-free.
    pub fn painted_on_screen(&self, needle: &str, within: Duration) -> bool {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            if self.painted.screen().contains(needle) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }

    pub fn screen(&self) -> String {
        self.painted.screen()
    }

    pub fn bytes_painted(&self) -> usize {
        self.painted.painted()
    }
}

impl Drop for CodexPane {
    fn drop(&mut self) {
        self.registry.kill_all();
        std::fs::remove_dir_all(&self.root).ok();
    }
}
