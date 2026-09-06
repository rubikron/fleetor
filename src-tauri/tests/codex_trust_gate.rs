//! **Checkpoint 14's second half: a seeded codex pane renders no first-run gate**
//! (WP-25 phase 2, #30; C17, C21, C34, C47; D-034, Tier 1.5).
//!
//! C17 renamed checkpoint 14 from "project-key canonicalization" to "project
//! identity **and trust seeding**" because the key's shape is only half the
//! problem: every harness parks a fresh pane on a first-run trust dialog, and a
//! pane sitting on one looks completely healthy while every `fleet send` into it
//! comes back `accepted` (L1). So the assertion is two-part, and only one part is
//! an artifact assertion.
//!
//!  - **Both keys written canonically** — `placement::codex`'s own unit tests,
//!    which read the seeded `config.toml`.
//!  - **A pane booted in a *subdirectory* of its worktree renders no gate** —
//!    this file, against the real binary. Nothing readable in the seed proves the
//!    vendor honoured it, which is the whole reason the vendor tier exists.
//!
//! ## Why the subdirectory is the case that matters
//!
//! C34 measured that codex resolves trust by a **two-candidate exact lookup** —
//! the canonicalized cwd, or the git root that cwd resolves to — with no ancestor
//! walk. For a *linked worktree*, which is what every FLEETOR worker gets, the
//! resolved root is the **main repository**, so:
//!
//!  - a key for the worktree trusts the worktree root and **not one directory
//!    below it**;
//!  - a key for the main repository trusts the worktree and everything under it.
//!
//! `git rev-parse --show-toplevel` inside a worktree returns the *worktree*, so
//! the obvious way to compute the key produces the one that does not cover
//! subdirectories. That mistake is invisible at the worktree root and invisible in
//! the file. **The control arm is what makes this file worth running:** the same
//! pane, in the same directory, seeded with `--show-toplevel`'s answer instead,
//! must gate. Without it a passing fix arm would also pass against a vendor that
//! had stopped gating at all.
//!
//! ## Three states, not two
//!
//! A pane that renders **neither** marker is [`Verdict::Inconclusive`] and fails.
//! Treating "no gate appeared" as "trusted" would let a pane that crashed before
//! painting pass every arm silently — the class of lie D-034 exists to refuse, and
//! the same discipline `trust_probe.py` runs under.
//!
//! ## Both binaries (C47)
//!
//! The `codex` on this machine is a cmux shim that injects flags, and #31 found a
//! verdict that differs absolutely between it and the real binary. #25 ran its
//! whole trust matrix both ways and got identical verdicts, so trust is expected
//! to be shim-independent — expected is not measured, and this file takes both
//! readings and names which binary each came from.
//!
//! ## Zero tokens
//!
//! No arm types anything into a pane, and every arm's provider is a loopback port
//! with nothing listening. No turn is ever completed, and every assertion holds on
//! a machine with no network. #38 is the only ticket authorized to spend money.
//!
//! ## This file now runs the production configuration (#47)
//!
//! It did not, and said so. The wake #42 added presses `\r` until the pane goes
//! quiet, `1. Yes, continue` is the gate's **pre-selected** option, so #30 spawned
//! every pane here under `BringUp::AtOnce` to keep its control arm from being
//! answered out from under it — and named the discrepancy rather than letting it
//! pass, because it meant codex's own bring-up was the untested one.
//!
//! #47 measured what that keypress actually does and it was worse than assumed:
//! the wake's second `\r` dismissed the dialog, the pane reached its composer
//! looking perfectly healthy, and the vendor **persisted** `trust_level =
//! "trusted"` for the directory into the pane's own `config.toml`. So the wake now
//! refuses a gate instead of pressing through it, and these arms run under
//! `CODEX_SPEC` — the spec a real pane is placed with.
//!
//! **The control got stronger rather than weaker.** Under `AtOnce` it could only
//! say "the gate was painted". Under `AfterWaking` it says three things: the gate
//! was painted, `spawn` **refused** and named the directory, and the pane wrote no
//! trust row it was not seeded with. That last is the actual property #47 is about
//! — a pane whose seeding failed is not silently trusted — and it was unassertable
//! from here before.
//!
//! ## Cost, stated (D-081)
//!
//! About 25 s on a machine with `codex` installed: four real panes, two of which
//! are deliberately allowed to gate. The skip is loud for C13's reason — a tier
//! that skips quietly rots into decoration.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD, Engine};
use fleetor_core::pane::PaneId;
use fleetor_shell::placement::codex::{codex, CODEX_SPEC, OPERATOR_DIR};
use fleetor_shell::placement::Seed;
use fleetor_shell::pty::{out_channel, Emit, PaneRegistry};
use portable_pty::CommandBuilder;
use toml_edit::{DocumentMut, Item, Table, Value};

/// The vendor binary, and the build every verdict here was recorded against.
const VENDOR_BIN: &str = "codex";
const VENDOR_ABSOLUTE: &str = "/opt/homebrew/bin/codex";
const RECORDED_BUILD: &str = "codex-cli 0.153.4";

/// **The gate's own words**, the literal `trust_probe.py` detects and the one
/// thing that makes this measurable with no human at the keyboard. It reaches the
/// pty in about three seconds.
const GATE: &str = "Do you trust the contents of this directory";

/// The pane's window. A 0x0 terminal is not a terminal — codex paints nothing at
/// all into one, which cost #25 a whole matrix of inconclusive arms.
const ROWS: u16 = 40;
const COLS: u16 = 120;

/// How long a pane is given to paint a marker. A gated pane spends about three
/// seconds of it; this is the budget before "neither marker" becomes a verdict.
const TO_A_VERDICT: Duration = Duration::from_secs(20);

/// The `[projects."…"]` keys a seeded `CODEX_HOME` carries right now.
///
/// Read **after** a pane has run, because the question #47 asks is whether the pane
/// added one: codex persists the answer to its trust dialog into this same file, so
/// a key that was not seeded is a directory FLEETOR trusted on the operator's
/// behalf. A line scan rather than a TOML parse — the file is the vendor's to
/// shape, and a malformed one is a thing to see rather than to fail parsing.
fn trust_keys_in(seeded: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(seeded.join("config.toml")) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            Some(line.trim().strip_prefix("[projects.")?.strip_suffix(']')?.to_string())
        })
        .collect()
}

// --- the instrument -----------------------------------------------------------

/// What one pane painted, assembled from the registry's own emit callback — the
/// same bytes the operator's terminal renders.
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

    /// What the pane painted, escapes removed and **all** whitespace collapsed.
    ///
    /// Codex repaints character by character with cursor moves between, so a word
    /// never survives as a word with its spacing intact. Collapsing is what makes
    /// a substring test work at all. Lifted from `codex_bringup.rs`, which is
    /// `probe_clear.py`'s `screen()` in Rust.
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
                Some(']') => {
                    for c in chars.by_ref() {
                        if c == '\u{7}' || c == '\u{1b}' {
                            break;
                        }
                    }
                }
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

/// What one pane did with its first-run gate. **Three states**, because "neither"
/// is a failure and not a pass — see the header.
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    /// The composer, resolving this pane's own directory. No gate was drawn.
    Trusted,
    /// The gate's literal reached the pty.
    Gated,
    /// Neither marker inside [`TO_A_VERDICT`]. Never a pass.
    Inconclusive,
}

// --- the fixture ---------------------------------------------------------------

/// A repository, a **linked worktree** of it, and a subdirectory of that worktree
/// — the exact shape a FLEETOR worker runs in.
///
/// **Rooted under `/tmp` rather than the platform temp dir, and deliberately.**
/// Codex's header elides a path past roughly 44 characters, and the marker this
/// file looks for is the *unelided* one; `/var/folders/…/T/` blows that budget
/// before the fixture even starts. The short root also exercises C34's asymmetry
/// for free: the cwd is handed over spelled `/tmp/…` and the keys are written
/// `/private/tmp/…`, which is the direction codex honours.
struct Fixture {
    root: PathBuf,
    repo: PathBuf,
    worktree: PathBuf,
    deep: PathBuf,
    operator_home: PathBuf,
    pane_home: PathBuf,
}

impl Fixture {
    fn build() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("a clock")
            .as_nanos()
            % 1_000_000_000;
        let root = PathBuf::from(format!("/tmp/ftr-{stamp}"));
        let repo = root.join("repo");
        let worktree = root.join("wt");
        let deep = worktree.join("sub").join("deep");
        let operator_home = root.join("op");
        let pane_home = root.join("home");

        init_repo(&repo);
        git_in(&repo, &["worktree", "add", "-q", "-b", "fleet/trust", &worktree.to_string_lossy()]);
        for dir in [&deep, &operator_home.join(OPERATOR_DIR), &pane_home] {
            std::fs::create_dir_all(dir).expect("the fixture");
        }
        Self { root, repo, worktree, deep, operator_home, pane_home }
    }

    /// A pane's `CODEX_HOME`, seeded by **codex's own seeder** — not a
    /// hand-written directory. The `CODEX_HOME` under test has to be the one a
    /// pane actually gets (C6), or this measures something FLEETOR does not do.
    fn seed(&self, name: &str) -> PathBuf {
        let seeded = self.root.join(name);
        codex()
            .seed_config_dir(
                &Seed::new(&seeded, &self.deep, Some(&self.operator_home))
                    .with_brief("You are a FLEETOR worker pane."),
            )
            .expect("seeding a pane's CODEX_HOME");
        seeded
    }

    /// **The control**: the same seed with the same everything, except that its
    /// `projects` table holds `--show-toplevel`'s answer — the worktree root —
    /// and nothing else.
    ///
    /// Written by editing the seeded document rather than by hand, so the arm
    /// differs from the fix arm in one table and cannot drift into being a
    /// different pane.
    fn seed_the_show_toplevel_mistake(&self, name: &str) -> PathBuf {
        let seeded = self.seed(name);
        let file = seeded.join("config.toml");
        let mut doc: DocumentMut =
            std::fs::read_to_string(&file).expect("the seed").parse().expect("valid TOML");

        let mut record = Table::new();
        record.insert("trust_level", Item::Value(Value::from("trusted")));
        let mut projects = Table::new();
        projects.insert(&codex().project_key(&self.worktree), Item::Table(record));
        doc.as_table_mut().insert("projects", Item::Table(projects));

        std::fs::write(&file, doc.to_string()).expect("the control's seed");
        seeded
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

/// One git command, with identity, signing and hooks pinned per command so a
/// result never depends on this machine's own git configuration.
fn git_in(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.email=scratch@fleetor.test"])
        .args(["-c", "user.name=Scratch"])
        .args(["-c", "commit.gpgsign=false"])
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(args)
        .output()
        .expect("git must be on PATH for these tests");
    assert!(
        out.status.success(),
        "git {args:?} in {} failed: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr),
    );
}

fn init_repo(dir: &Path) {
    std::fs::create_dir_all(dir).expect("a repository root");
    std::fs::write(dir.join("README.md"), "scratch\n").expect("something to commit");
    git_in(dir, &["init", "-q", "-b", "main"]);
    git_in(dir, &["add", "-A"]);
    git_in(dir, &["commit", "-q", "-m", "first"]);
}

// --- the arm -------------------------------------------------------------------

/// What one pane's bring-up produced: which side of the gate it ended on, what
/// `PaneRegistry::spawn` answered, and what it painted.
struct Reading {
    verdict: Verdict,
    /// **The production bring-up's own answer** (#47). `Ok` is a pane that woke and
    /// was announced; `Err` is the wake refusing to press through a trust dialog,
    /// and the string is what the operator reads in the pane itself.
    spawned: Result<(), String>,
    screen: String,
}

/// Boot one real codex pane on a real pty, **through codex's own bring-up**, and
/// watch it until it says which side of the gate it is on.
///
/// The only thing ever written to it is `BringUp::AfterWaking`'s wake key, by the
/// registry, on the bring-up path — which is the configuration a placed pane runs
/// and the one #30 could not use. Nothing is typed into the composer, and no arm
/// completes a turn.
fn verdict(vendor: &Path, seeded: &Path, fixture: &Fixture) -> Reading {
    // A loopback port with nothing listening. The pane never completes a turn —
    // it never starts one — and this is the belt to that brace.
    let dead_port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let port = listener.local_addr().expect("its address").port();
        drop(listener);
        port
    };

    let mut cmd = CommandBuilder::new(vendor);
    // The shell's own terminal is not an alt-screen one, and #25 measured through
    // the same flag.
    cmd.arg("--no-alt-screen");
    for (key, value) in [
        ("model_provider", "probe".to_string()),
        ("model_providers.probe.name", "\"probe\"".to_string()),
        ("model_providers.probe.base_url", format!("\"http://127.0.0.1:{dead_port}\"")),
        ("model_providers.probe.wire_api", "\"responses\"".to_string()),
        ("model_providers.probe.experimental_bearer_token", "\"sk-probe\"".to_string()),
        ("model_providers.probe.request_max_retries", "0".to_string()),
        ("model_providers.probe.stream_max_retries", "0".to_string()),
    ] {
        cmd.arg("-c");
        cmd.arg(format!("{key}={value}"));
    }
    cmd.cwd(&fixture.deep);
    cmd.env("CODEX_HOME", seeded);
    cmd.env("HOME", &fixture.pane_home);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");

    // The composer resolves this pane's own directory, in the header and again in
    // the status line. Both spellings, because the header elides a long path and
    // the status line does not.
    let real = std::fs::canonicalize(&fixture.deep).expect("the cwd resolves");
    let real = real.to_string_lossy();
    let trusted = [format!("directory:{real}"), format!("·{real}")];
    let gate: String = GATE.chars().filter(|c| !c.is_whitespace()).collect();

    let pane = PaneId::Worker(1);
    let painted = Painted::watching(pane);
    let registry = PaneRegistry::new(painted.emitter(), fixture.root.join("panes.pids"));
    // Under `AfterWaking` this blocks for the length of the bring-up: about two
    // seconds for a pane that settles, about three for one that meets the gate and
    // is refused. Both are the pane's own pace, not a budget.
    let spawned = registry.spawn(pane, cmd, &CODEX_SPEC, ROWS, COLS);

    let deadline = Instant::now() + TO_A_VERDICT;
    let mut verdict = Verdict::Inconclusive;
    while Instant::now() < deadline {
        let screen = painted.screen();
        if screen.contains(&gate) {
            verdict = Verdict::Gated;
            break;
        }
        if trusted.iter().any(|marker| screen.contains(marker.as_str())) {
            verdict = Verdict::Trusted;
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let screen = painted.screen();
    registry.kill_all();
    Reading { verdict, spawned, screen }
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

/// Every binary worth driving on this machine, named (C47).
fn readings() -> Vec<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    if let Some(shim) = on_path(VENDOR_BIN) {
        found.push((format!("PATH `{VENDOR_BIN}` ({})", shim.display()), shim));
    }
    let absolute = PathBuf::from(VENDOR_ABSOLUTE);
    if absolute.is_file() && !found.iter().any(|(_, at)| *at == absolute) {
        found.push((format!("the vendor binary at {VENDOR_ABSOLUTE}"), absolute));
    }
    found
}

/// **The ticket, measured** (#30, C34).
///
/// A pane seeded by `seed_config_dir` and booted in a *subdirectory* of its
/// worktree reaches its composer. The same pane seeded with `--show-toplevel`'s
/// answer gates — that arm is the control, and it is what says the fix arm means
/// something.
#[test]
fn a_codex_pane_booted_below_its_worktree_is_trusted_and_show_toplevel_alone_gates() {
    let readings = readings();
    if readings.is_empty() {
        announce(&[
            format!("SKIP: no `{VENDOR_BIN}` on this machine ({RECORDED_BUILD} expected)."),
            "  Checkpoint 14's artifact half still runs — `placement::codex`'s unit tests read".into(),
            "  both keys out of the seeded config.toml. What is skipped is the half that".into(),
            "  matters most: that the vendor honours them and a pane below its worktree root".into(),
            "  renders no first-run trust gate. This machine runs the weaker suite (C13).".into(),
        ]);
        return;
    }

    for (name, vendor) in readings {
        let fixture = Fixture::build();

        let seeded = fixture.seed("pane-config");
        // The two halves of checkpoint 14, in the same breath: this is the key the
        // pane below is about to be trusted *by*, so a green arm can never be read
        // as "the vendor stopped gating".
        let written = std::fs::read_to_string(seeded.join("config.toml")).expect("the seed");
        let repo_key = codex().project_key(&fixture.repo);
        assert!(
            written.contains(&format!("[projects.{repo_key:?}]")),
            "the seeder wrote no main-repository key for {}:\n{written}",
            fixture.repo.display(),
        );

        let fix = verdict(&vendor, &seeded, &fixture);
        assert_eq!(
            fix.verdict,
            Verdict::Trusted,
            "{name}: a pane seeded by FLEETOR and booted in {} did not reach its composer. \
             Either the main-repository key is missing — check it is the parent of \
             `--git-common-dir` and not `--show-toplevel` — or the vendor changed; re-measure \
             against {RECORDED_BUILD} with `python3 examples/codex-spike/trust_probe.py`.\n\
             What it painted:\n{}",
            fixture.deep.display(),
            fix.screen,
        );
        // **The normal path is unaffected** (#47). A correctly seeded pane meets no
        // gate, so the wake settles it and it is announced exactly as before.
        assert!(
            fix.spawned.is_ok(),
            "{name}: a correctly seeded pane was refused by its own bring-up: {:?}. #47 may \
             only refuse a pane that is actually sitting on the dialog — refusing a healthy \
             one is a fleet with no codex in it.",
            fix.spawned,
        );

        let control_seed = fixture.seed_the_show_toplevel_mistake("pane-config-control");
        let before = trust_keys_in(&control_seed);
        let control = verdict(&vendor, &control_seed, &fixture);
        assert_eq!(
            control.verdict,
            Verdict::Gated,
            "{name}: the control did not gate. A key for the worktree root is supposed to \
             leave {} untrusted (C34, row 11); if it no longer does, the arm above proves \
             nothing and this file's whole claim needs re-measuring.\nWhat it painted:\n{}",
            fixture.deep.display(),
            control.screen,
        );

        // **The ticket, in the production configuration** (#47). The wake met the
        // dialog and refused it by name rather than pressing the pre-selected
        // `1. Yes, continue`.
        let refusal = control.spawned.clone().expect_err(&format!(
            "{name}: a pane sitting on its trust dialog was brought up successfully, which \
             means the wake answered it. That is FLEETOR trusting {} on the operator's \
             behalf — measured, and the vendor writes it to disk.",
            fixture.deep.display(),
        ));
        assert!(
            refusal.contains(&fixture.deep.display().to_string())
                || refusal.contains(
                    &std::fs::canonicalize(&fixture.deep).expect("the cwd resolves").display().to_string()
                ),
            "{name}: the refusal did not name the directory that was not trusted: {refusal}",
        );

        // **And it wrote nothing.** This is the half that was unassertable while
        // these arms ran under `AtOnce`: codex persists the answer to its own
        // dialog, so an unchanged projects table is the evidence that no answer was
        // given.
        assert_eq!(
            trust_keys_in(&control_seed),
            before,
            "{name}: the control pane's config.toml gained a trust key it was not seeded \
             with, so something answered the dialog and the vendor persisted it (#47)",
        );

        announce(&[format!(
            "codex trust gate, {name}: seeded pane below its worktree = trusted and announced, \
             --show-toplevel alone = gated, refused by name, and no trust written."
        )]);
    }
}
