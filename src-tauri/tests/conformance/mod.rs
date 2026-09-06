//! The conformance driver: **one pass per registered harness**, over the same
//! assertions (WP-25, M23, C17, C19, C20).
//!
//! This module is the shape, not the assertions. It places a real fleet's worth
//! of panes through [`placement::place`] against a scratch layout, once for every
//! entry in the harness registry, and hands each checkpoint file a [`Pass`] to
//! assert against. The checkpoints themselves live beside it, split by number:
//!
//!  - `tests/harness_conformance_1_7.rs` — program and base arguments, brief
//!    carrier, model flag and posture, config dir and its seeding, credential
//!    wiring and the scrub, config and credential isolation, write-guardrail
//!    install.
//!  - `tests/harness_conformance_8_14.rs` — outbound reachability, typing
//!    profile, command channel, gauge, orphan names, transcript, project identity
//!    and trust.
//!
//! **How to add checkpoints 8–14 without editing the file next door.** Write a
//! new top-level test binary, `mod conformance;`, and one `#[test]` per
//! checkpoint whose whole body is [`for_each_registered`]. Everything a
//! checkpoint can look at is on [`Pass`]; if a checkpoint needs something the
//! pass does not carry, add it *here* — a field, or a method like
//! [`Pass::place_worker_against`] — rather than placing panes of your own, so
//! that both files keep asserting against the same bring-up. The one thing that
//! must not move is the rule below.
//!
//! ## The rule every checkpoint here obeys
//!
//! **Assert end to end through [`placement::place`], never against the spec
//! constant.** A test that reads `CLAUDE_CODE_SPEC` and agrees with it tests the
//! constant; a test that drives `place` and finds the spec's answer in the
//! command, in the seeded files, or in the notices tests the seam. The spec is an
//! *input* to every assertion below — it says what to look for — and the
//! observable output of `place` is what is looked at. That is what makes this
//! suite the safety net for the migrate batches: they move the literals onto the
//! spec underneath a `place` whose caller-observable behaviour does not change,
//! so a batch that breaks behaviour fails here rather than passing quietly.
//!
//! **A harness that stubs a checkpoint fails rather than compiling quietly.**
//! Each checkpoint below refuses the empty answer — an empty program name, an
//! empty seed-key list, a scrub list that removes nothing an attended seat is
//! actually given — because an assertion that would pass against a stub is worse
//! than no assertion: it reads as coverage.
//!
//! ## Nothing here touches the operator's installation
//!
//! `tests/placement.rs`'s discipline exactly: the layout and the host arrive as
//! values, every path is under a scratch root this module makes and removes, and
//! **nothing here sets an environment variable**. The credential, the model, the
//! endpoint and the posture are all distinctive scratch values, so finding one on
//! a command proves it travelled from the caller rather than coinciding with a
//! default.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD, Engine};
use fleetor_core::pane::PaneId;
use fleetor_shell::placement::harness::{registered, Harness};
use fleetor_shell::placement::{self, HarnessSpec, Host, Layout, PaneSpec, Placed};
use fleetor_shell::prompts::PaneContext;
use fleetor_shell::pty::{Emit, PaneRegistry};
use fleetor_shell::runs;

// --- the values a pass hands in ------------------------------------------------

/// The marker the orchestrator's brief carries. Distinctive, so finding it proves
/// the brief travelled rather than that some default happened to match.
pub const ORCH_BRIEF_MARK: &str = "FLEETOR-CONFORMANCE-ORCH-BRIEF";

/// The marker a worker's brief carries. Different from the orchestrator's, so a
/// harness that hands every seat the same brief fails rather than passing on one
/// of them.
pub const WORKER_BRIEF_MARK: &str = "FLEETOR-CONFORMANCE-WORKER-BRIEF";

/// The fleet's own credential, as this pass's host holds it.
pub const WORKER_KEY: &str = "sk-fleetor-conformance-worker-key";

/// The model the caller names for unattended seats.
pub const WORKER_MODEL: &str = "fleetor-conformance-worker-model";

/// The endpoint the caller names for unattended seats.
pub const WORKER_BASE_URL: &str = "https://workers.conformance.invalid/harness";

/// The permission posture the caller names for unattended seats. Deliberately not
/// a value any harness would default to.
pub const WORKER_POSTURE: &str = "fleetor-conformance-posture";

/// The slot the pass's worker is placed in.
pub const WORKER_SLOT: u8 = 1;

// --- one pass ------------------------------------------------------------------

/// One registered harness, and the panes `place` actually produced for it.
///
/// Two seats, because between them they carry every asymmetry checkpoints 1–7 are
/// about: [`Pass::orch`] is the operator's own, attended, on their account, and
/// [`Pass::worker`] is fenced, unattended, on the fleet's credential. A checkpoint
/// asserted on only one of them cannot tell "this harness wires the credential"
/// from "this harness wires the credential everywhere", which is the failure
/// D-062 exists to prevent.
pub struct Pass {
    /// The registry entry this pass is for.
    pub harness: &'static dyn Harness,
    /// Its fourteen answers — the *input* to every assertion, never the thing
    /// asserted against itself.
    pub spec: &'static HarnessSpec,

    /// The scratch root. The layout and the target are both inside it, and it is
    /// removed when the pass drops.
    pub root: PathBuf,
    /// Where this fleet lives, for this pass.
    pub layout: Layout,
    /// The repository the fleet was pointed at — a real git repo, so the worker
    /// gets its own worktree rather than the announced fallback.
    pub target: PathBuf,
    /// What the caller handed placement: briefs carrying the two marks above, and
    /// launch values distinctive enough to prove they travelled.
    pub context: PaneContext,
    /// A machine with a `fleet` binary and the fleet's key on it, and **nothing
    /// else** — no operator home, no toolchain, no stand-in pane program. The
    /// machine-with-nothing case, which is what makes "created fresh rather than
    /// snapshotted from the operator's own" a thing a test can say.
    pub host: Host,

    /// The operator's own seat, placed.
    pub orch: Placed,
    /// One fenced worker, placed.
    pub worker: Placed,
    /// The directory that worker was placed in: its own checkout of the target.
    pub worker_cwd: PathBuf,
}

/// Run `check` once per registered harness, and return how many times it ran.
///
/// `tag` names the checkpoint and only reaches the scratch directory's name, so a
/// failure says which harness and which checkpoint without two tests colliding on
/// disk.
pub fn for_each_registered(tag: &str, check: impl Fn(&Pass)) -> usize {
    let all = registered();
    assert!(
        !all.is_empty(),
        "no harness is registered, so this suite asserts nothing — a suite with one \
         registered harness is the point, a suite with none is decoration",
    );
    for harness in all {
        let pass = Pass::place(tag, *harness);
        check(&pass);
    }
    all.len()
}

impl Pass {
    /// Bring both seats up for one harness, against a scratch root of its own.
    fn place(tag: &str, harness: &'static dyn Harness) -> Self {
        let spec = harness.spec();
        let root = std::env::temp_dir().join(format!(
            "fleetor-conformance-{tag}-{}-{}-{:?}",
            spec.name,
            std::process::id(),
            std::thread::current().id(),
        ));
        let _ = std::fs::remove_dir_all(&root);
        let layout = Layout::under(root.join("state"));
        let target = root.join("repo");
        init_repo(&target);

        let context = conformance_context();
        let host = Host {
            fleet_bin: Some(root.join("fleet")),
            api_key: Some(WORKER_KEY.to_string()),
            ..Host::bare()
        };

        let orch = placement::place(PaneSpec::orch(harness), &layout, &host, &target, &context)
            .expect("placing the operator's own seat against a scratch layout");
        let worker =
            placement::place(PaneSpec::worker(WORKER_SLOT, harness), &layout, &host, &target, &context)
                .expect("placing a fenced worker against the same scratch layout");

        // The worker really is in its own checkout rather than the announced
        // fallback — otherwise every containment claim below would be about the
        // target repository instead of about the fleet's own tree.
        let worker_cwd = layout.worktree(&target, WORKER_SLOT);
        assert!(
            worker_cwd.join(".git").exists(),
            "the pass needs a worker in its own worktree, not the shared checkout",
        );

        let pass = Self {
            harness,
            spec,
            root,
            layout,
            target,
            context,
            host,
            orch,
            worker,
            worker_cwd,
        };
        pass.assert_placed_as_the_harness_under_test();
        pass
    }

    /// **Every seat was placed as the harness this pass is for.**
    ///
    /// It was trivially true while exactly one harness was registered, and it was
    /// written to be the thing that failed — loudly, in every checkpoint at once —
    /// on the day a second one was registered without `place` being able to be
    /// told which to place. That day is #33: [`PaneSpec::orch`] and
    /// [`PaneSpec::worker`] carry the answer, so this is now the assertion that a
    /// pass really did drive its own harness rather than the first one in the
    /// registry. It is here rather than in one checkpoint because it is a
    /// precondition of all of them.
    fn assert_placed_as_the_harness_under_test(&self) {
        for (seat, placed) in self.seats() {
            assert!(
                std::ptr::eq(placed.harness, self.spec),
                "{seat} was placed as {} while this pass is for {} — `place` has no way \
                 to be told which harness to place, so a second registered harness \
                 cannot be conformance-tested until it does",
                placed.harness.name,
                self.spec.name,
            );
        }
    }

    /// The two seats, named for failure messages: the attended one first.
    pub fn seats(&self) -> [(&'static str, &Placed); 2] {
        [("orch", &self.orch), ("worker", &self.worker)]
    }

    /// The unattended seat's own working directory, and the attended one's.
    pub fn cwd_of(&self, seat: &str) -> PathBuf {
        match seat {
            "orch" => self.target.clone(),
            _ => self.worker_cwd.clone(),
        }
    }

    /// The configuration directory a placed pane was actually pointed at —
    /// **read off the command rather than re-derived from the layout**, so a
    /// harness that seeds one directory and points the pane at another fails here
    /// instead of passing twice.
    pub fn config_dir(&self, placed: &Placed) -> PathBuf {
        let var = self.spec.config_dir.env_var;
        PathBuf::from(
            env_on(placed, var)
                .unwrap_or_else(|| panic!("the command must set {var}, checkpoint 4's own name")),
        )
    }

    /// Everything under a placed pane's configuration directory, as text where it
    /// is text. The seeded file, the settings file and anything else a harness
    /// writes are all in here, so a checkpoint can look for a key without knowing
    /// the file format.
    pub fn config_text(&self, placed: &Placed, file: &str) -> String {
        let path = self.config_dir(placed).join(file);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
    }

    /// Whether `path` is inside the layout this pass handed placement, or inside
    /// the target it handed it. Those two are the whole of what a placement may
    /// name.
    pub fn is_contained(&self, path: &Path) -> bool {
        let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        [self.layout.root().to_path_buf(), self.target.clone()].iter().any(|allowed| {
            let allowed = std::fs::canonicalize(allowed).unwrap_or_else(|_| allowed.clone());
            resolved.starts_with(&allowed)
        })
    }

    /// Place a second worker against another repository, for the checkpoints about
    /// what survives a target switch. The config directory is the same one, which
    /// is the whole point.
    pub fn place_worker_against(&self, name: &str) -> (PathBuf, Placed) {
        let other = self.root.join(name);
        init_repo(&other);
        let placed =
            placement::place(PaneSpec::worker(WORKER_SLOT, self.harness), &self.layout, &self.host, &other, &self.context)
                .expect("placing the same worker after a target switch");
        (other, placed)
    }

    // --- what checkpoints 8–14 needed the pass to carry -------------------------

    /// Place the attended seat against a machine that has **no `fleet` binary**,
    /// for the half of checkpoint 8 that is about a pane which cannot talk.
    ///
    /// Everything else about the machine is this pass's own, so the only thing
    /// that can differ between this placement and [`Pass::orch`] is the one fact
    /// being varied.
    pub fn place_orch_without_fleet_bin(&self) -> Placed {
        let host = Host {
            fleet_bin: None,
            api_key: Some(WORKER_KEY.to_string()),
            ..Host::bare()
        };
        placement::place(PaneSpec::orch(self.harness), &self.layout, &host, &self.target, &self.context)
            .expect("placing the operator's own seat on a machine with no fleet binary")
    }

    /// Place both seats against a machine that **has** an operator installation,
    /// and return where that installation is along with the two placements.
    ///
    /// **Checkpoint 6's second machine, and #33 is why it exists.** The pass's own
    /// machine deliberately has nothing on it, which is what makes "the isolated
    /// directory is created fresh rather than snapshotted" a thing a test can say
    /// rather than assume. That was the whole story while every registered harness
    /// answered `seeds_from_operator: false`; a harness that answers `true` needs
    /// the other machine too, or the checkpoint can only refuse it.
    ///
    /// The operator's `HOME` here is a scratch directory under this pass's own
    /// root. It is never the machine's real one — [`Host::bare`] carries `None`
    /// and this suite sets no environment variable — so a seeder that walks it
    /// cannot reach the operator's own installation even by accident, which is the
    /// property `Seed::operator_home` was made a value for.
    pub fn place_against_an_operator_installation(&self) -> (PathBuf, Placed, Placed) {
        let home = self.root.join("an-operators-home");
        std::fs::create_dir_all(&home).expect("a scratch operator home");
        let host = Host {
            fleet_bin: self.host.fleet_bin.clone(),
            api_key: Some(WORKER_KEY.to_string()),
            operator_home: Some(home.clone()),
            ..Host::bare()
        };
        let orch =
            placement::place(PaneSpec::orch(self.harness), &self.layout, &host, &self.target, &self.context)
                .expect("placing the attended seat against a machine with an operator home");
        let worker = placement::place(
            PaneSpec::worker(WORKER_SLOT, self.harness),
            &self.layout,
            &host,
            &self.target,
            &self.context,
        )
        .expect("placing a fenced seat against the same machine");
        (home, orch, worker)
    }

    /// The directory a placed pane's transcripts live in: checkpoint 13's `subdir`
    /// under the configuration directory the pane was **actually pointed at**, for
    /// the same reason [`Pass::config_dir`] reads that off the command.
    ///
    /// It does not exist after a placement, and that is correct rather than a bug:
    /// `place` makes the config dir and seeds it, and the harness's own process
    /// makes this one when it first writes a transcript. A checkpoint that wants a
    /// transcript to exist plants it.
    pub fn transcript_dir(&self, placed: &Placed) -> PathBuf {
        self.config_dir(placed).join(self.spec.transcript.subdir)
    }

    /// Plant one transcript file where a pane of this harness would leave it, and
    /// return where it was put.
    ///
    /// `slug` stands in for whatever the harness names its per-project directory.
    /// **That naming is deliberately not re-derived here**: it is not one of the
    /// fourteen answers, so a test that computed it would be encoding one vendor's
    /// rule as though it were the seam's. What the harvest is asserted on is the
    /// two things that *are* spec'd — the subdirectory and the extension.
    pub fn plant_transcript(&self, placed: &Placed, slug: &str, file: &str, body: &str) -> PathBuf {
        let dir = self.transcript_dir(placed).join(slug);
        std::fs::create_dir_all(&dir).expect("a scratch transcript directory");
        let path = dir.join(file);
        std::fs::write(&path, body).expect("a scratch transcript");
        path
    }

    /// Rotate this pass's layout into an archive, and return the run directory
    /// that was written — the public path to checkpoint 13's harvest.
    ///
    /// A previous run has to exist for rotation to archive one, so one is put
    /// there. It is deliberately **not** a real database: rotation's documented
    /// fallback is that a log which cannot be opened must still be archivable,
    /// which is exactly what a checkpoint about transcripts wants — the harvest
    /// runs, and nothing here depends on the log itself.
    pub fn harvest_into_a_run(&self) -> PathBuf {
        let shell = self.layout.shell();
        std::fs::create_dir_all(&shell).expect("the layout's own shell directory");
        std::fs::write(shell.join("state.db"), b"not a database; rotation archives it anyway")
            .expect("a previous run for rotation to archive");

        let runs_root = self.root.join("runs");
        runs::rotate(&shell, &runs_root, 1_700_000_000_000);

        let mut archived: Vec<PathBuf> = std::fs::read_dir(&runs_root)
            .expect("rotation makes the runs directory")
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        assert_eq!(
            archived.len(),
            1,
            "rotation must archive exactly one run for the harvest to be readable: {archived:?}",
        );
        archived.pop().expect("the archived run")
    }

    /// Type `body` into a **real pty** the way the hub types every message, and
    /// return what the process on the far end read back, plus how long the write
    /// itself took. Checkpoint 9's evidence; [`Self::echo_of`] is the mechanics.
    pub fn echo_of_a_paste(&self, body: &str) -> (String, Duration) {
        self.echo_of(|registry, pane| registry.write_paste(pane, body))
    }

    /// Type `command` into a **real pty** the way the hub types every slash
    /// command, and return what the process on the far end read back.
    ///
    /// **The sibling checkpoint 10 was missing** (C35). That checkpoint used to
    /// assert against [`fleetor_core::Command::keystrokes`], which is
    /// harness-free: it is the *canonical* spelling, so the suite asserted what
    /// the fleet decided rather than what the harness types, and it passed only
    /// because Claude Code's table happens to be the identity map. A harness
    /// whose table was not the identity would be typed correctly and asserted
    /// wrongly. Driving [`PaneRegistry::write_command`] over a real pty asserts
    /// the production translation instead — the same call `deliver` makes, fed
    /// the same `keystrokes()` the hub hands it, with the spelling applied where
    /// production applies it.
    ///
    /// Everything else is [`Self::echo_of_a_paste`]'s: the same placement, the
    /// same stand-in pane program, the same zero tokens. The elapsed time is
    /// dropped because the submit gap is checkpoint 9's assertion and asserting
    /// it twice would make one checkpoint's failure look like two.
    pub fn echo_of_a_command(&self, command: &str) -> String {
        self.echo_of(|registry, pane| registry.write_command(pane, command)).0
    }

    /// One stand-in pane, one write into it, and what the far end read back.
    ///
    /// **This is the one thing on `Pass` that is not a placement**, because
    /// checkpoints 9 and 10 are not properties of a command — they are properties
    /// of the bytes that are later written into one. It still starts at
    /// [`placement::place`]: the pane is placed exactly as this pass's worker is,
    /// against the same layout, with the one documented difference that the
    /// machine carries a stand-in pane program. That override is checkpoint 1's
    /// own escape hatch — it is a fact about the machine rather than about the
    /// harness, which is why it lives on [`Host`] — and it is what buys a real
    /// process on the far end of a real pty for no tokens. Measuring against the
    /// *vendor's* own binary is C13's tier, not this one; for codex that is
    /// `tests/codex_typing.rs`.
    ///
    /// The stand-in is a plain shell, so it echoes the paste markers back as
    /// ordinary characters and only ever prints a line it was given as a
    /// **submitted** one — which is what makes both halves of a profile visible
    /// from outside.
    fn echo_of(
        &self,
        write: impl FnOnce(&PaneRegistry, PaneId) -> Result<(), String>,
    ) -> (String, Duration) {
        let seen: Arc<Mutex<String>> = Arc::default();
        let sink = seen.clone();
        let emit: Emit = Arc::new(move |_channel: &str, payload: String| {
            let bytes = STANDARD.decode(&payload).unwrap_or_default();
            if let Ok(mut text) = sink.lock() {
                text.push_str(&String::from_utf8_lossy(&bytes));
            }
        });

        // `inherited_path` is the one field here that must be the machine's truth
        // rather than a scratch value: the stand-in's `#!/usr/bin/env bash` has to
        // find a real `bash`. It is *read* to describe the machine and handed in as
        // a value — nothing here sets an environment variable, and the placement
        // still computes the pane's PATH itself. `tests/panes.rs` makes the same
        // exception, for the same sentence's worth of reason.
        let host = Host {
            fleet_bin: Some(self.root.join("fleet")),
            api_key: Some(WORKER_KEY.to_string()),
            pane_program: Some(stand_in_pane_program().display().to_string()),
            inherited_path: std::env::var("PATH").unwrap_or_default(),
            ..Host::bare()
        };
        let placed = placement::place(
            PaneSpec::worker(WORKER_SLOT, self.harness),
            &self.layout,
            &host,
            &self.target,
            &self.context,
        )
        .expect("placing a worker against a machine whose pane program is the stand-in");

        let pane = PaneId::Worker(WORKER_SLOT);
        let registry = PaneRegistry::new(emit, self.root.join("pane-pids.json"));
        registry.spawn(pane, placed.command, placed.harness, 24, 80).expect("a real pty for the stand-in");
        wait_until(&seen, "ready", 0);

        // **Everything already on the far end is the *previous* conversation, and a
        // harness that has to be woken has one.** `BringUp::AfterWaking` presses a
        // key into the pane before it is announced (#42, C26), and the stand-in —
        // which submits on any newline — answers that press with an `echo: ` of its
        // own. Waiting for the marker from position zero would therefore return on
        // the *wake's* reply, before the write under test had produced anything,
        // and every assertion below would be made against a transcript that did not
        // contain it yet. The floor is taken here, so what is waited for is a reply
        // this write caused. Claude Code's `AtOnce` panes start at zero and are
        // unaffected — which is exactly the shape of thing registering a second
        // harness exists to surface.
        let from = seen.lock().map(|t| t.len()).unwrap_or(0);

        let started = Instant::now();
        let written = write(&registry, pane);
        let elapsed = started.elapsed();
        written.expect("a live pane accepts a write");

        // Waited for on the stand-in's *own* marker, never on the body: a tty echoes
        // what was written to it long before the process on the far end has read a
        // line, so waiting for the body would return while only the echo had
        // arrived. Whatever arrives, arrives — a profile that never submits produces
        // no reply at all, and that has to fail as the checkpoint's own assertion
        // with the collected text in hand rather than as a timeout in here.
        wait_until(&seen, "echo: ", from);
        let text = seen.lock().map(|t| t[from.min(t.len())..].to_string()).unwrap_or_default();
        registry.kill_all();
        (text, elapsed)
    }
}

/// How long a stand-in pane gets to start and echo on a loaded machine — long
/// enough not to flake, short enough that a real failure does not look like a
/// hang. `tests/panes.rs`'s number, for its reason.
const PATIENCE: Duration = Duration::from_secs(10);

/// A pane that is not the harness's own binary: a real process on the far end of
/// a real pty, spending nothing. The same stand-in `tests/panes.rs` drives.
fn stand_in_pane_program() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fake-pane/fake-pane.sh")
}

/// Poll until `needle` shows up **after byte `from`**, or give up quietly and let
/// the caller assert.
///
/// The offset is what makes this usable twice against one pane: a harness whose
/// bring-up types into the pane before it is announced has already produced
/// output, and "has the far end replied" has to mean "since the moment I asked".
fn wait_until(seen: &Arc<Mutex<String>>, needle: &str, from: usize) {
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        if seen.lock().is_ok_and(|text| text.len() > from && text[from.min(text.len())..].contains(needle)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

impl Drop for Pass {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

// --- reading what a placement produced ------------------------------------------

/// What `place` put in a command's environment.
pub fn env_on(placed: &Placed, key: &str) -> Option<String> {
    placed.command.get_env(key).map(|v| v.to_string_lossy().to_string())
}

/// A command's whole argv, program first.
pub fn argv_of(placed: &Placed) -> Vec<String> {
    placed.command.get_argv().iter().map(|a| a.to_string_lossy().into_owned()).collect()
}

/// The argument immediately following `flag`, or `None` when the flag is not
/// there. **Found by the flag rather than by position**, so a change to argument
/// order fails loudly rather than quietly asserting on the wrong thing.
pub fn arg_after(placed: &Placed, flag: &str) -> Option<String> {
    let argv = argv_of(placed);
    argv.iter().position(|a| a == flag).and_then(|i| argv.get(i + 1).cloned())
}

/// Every `--root` the installed guardrail hook was given, in order.
///
/// The hook's own spelling, not any harness's: the script, its arguments and the
/// journal are the fleet's and identical for every harness (checkpoint 7's doc
/// says so). What varies — the settings file, the event, the matcher — comes off
/// the spec instead.
pub fn roots_in(settings_text: &str) -> Vec<String> {
    let mut roots = Vec::new();
    let mut rest = settings_text;
    while let Some(at) = rest.find("--root '") {
        rest = &rest[at + "--root '".len()..];
        match rest.find('\'') {
            Some(end) => {
                roots.push(rest[..end].to_string());
                rest = &rest[end..];
            }
            None => break,
        }
    }
    roots
}

/// **The one reader of the three checkpoint key lists** — [`Posture::sandbox_keys`],
/// [`Credentials::provider_keys`] and [`Outbound::reachability_keys`] — asking
/// whether a pane's configuration directory actually states `key = value`.
/// Returns the file that states it.
///
/// **Reshaped in #33, and this is what it was shaped around** (C36, C41). The
/// three lists are *dotted paths* — each row is exactly what would have been typed
/// after `-c` — and the assertion that read them searched the config dir for the
/// **whole dotted key** as a literal substring. That is only ever true of a
/// document that renders a path as one flat string, which the sole registered
/// harness's did, because its own three lists were empty and nothing had rendered
/// a path at all. A seeder writes a nested path as *structure*: codex's
/// `model_providers.fleetor.base_url` arrives as the table header
/// `[model_providers.fleetor]` and the leaf `base_url = "…"`, and the whole dotted
/// key is nowhere in the file.
///
/// **Matching the last segment alone would have been the loosening**, and it is
/// what this deliberately is not: a harness that wrote `base_url` at top level,
/// under the wrong table, or in a different file entirely would pass that. What is
/// required instead is all three of —
///
///  1. **every segment of the path present**, so the tables the leaf hangs under
///     were really created;
///  2. **the leaf and the value on one line**, so the key holds *this* value
///     rather than the value appearing somewhere else in the document; and
///  3. **both in the same file**, which the old assertion did not require either —
///     it called `files_containing` twice and never compared the answers.
///
/// So the reshape is strictly stronger than what it replaced, and it still fails
/// against a stub: a harness that writes nothing states nothing.
pub fn config_states(dir: &Path, key: &str, value: &str) -> Option<PathBuf> {
    let segments: Vec<&str> = key.split('.').collect();
    let leaf = *segments.last().unwrap_or(&key);

    let mut stack = vec![dir.to_path_buf()];
    let mut found: Vec<PathBuf> = Vec::new();
    while let Some(next) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            if !segments.iter().all(|segment| text.contains(segment)) {
                continue;
            }
            if text.lines().any(|line| line.contains(leaf) && line.contains(value)) {
                found.push(path);
            }
        }
    }
    found.sort();
    found.into_iter().next()
}

/// Every file under `dir` whose text contains `needle`, as paths relative to it.
/// A repository's own `.git` is skipped: it is git's bookkeeping, not something a
/// harness wrote into the operator's working tree.
pub fn files_containing(dir: &Path, needle: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(next) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.file_name().is_some_and(|n| n == ".git") {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if std::fs::read_to_string(&path).is_ok_and(|text| text.contains(needle)) {
                found.push(path.strip_prefix(dir).unwrap_or(&path).to_path_buf());
            }
        }
    }
    found.sort();
    found
}

/// Whether `dir` holds anything at all.
pub fn is_non_empty_dir(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_some())
}

/// The pane a seat name belongs to, for the layout lookups a checkpoint may want.
pub fn pane_of(seat: &str) -> PaneId {
    match seat {
        "orch" => PaneId::Orch,
        _ => PaneId::Worker(WORKER_SLOT),
    }
}

// --- the fixture ---------------------------------------------------------------

/// The briefs and launch values every pass hands in.
///
/// Each one is a scratch value nothing would produce by accident, which is what
/// turns "the command carries a model" into "the command carries *the model the
/// caller named*". The extra write roots are cleared explicitly: an operator's
/// `[fence] allow` is a legitimate widening (WP-17) and the containment claims
/// below are about what placement does on its own.
fn conformance_context() -> PaneContext {
    let mut context = PaneContext::baked();
    context.orch_template = format!("{ORCH_BRIEF_MARK}\n{}", context.orch_template);
    context.worker_template = format!("{WORKER_BRIEF_MARK}\n{}", context.worker_template);
    context.launch.worker_model = WORKER_MODEL.to_string();
    context.launch.worker_base_url = WORKER_BASE_URL.to_string();
    context.launch.worker_permission_mode = WORKER_POSTURE.to_string();
    context.launch.fence_allow = Vec::new();
    context
}

/// Make `dir` a real repository with one commit — what `git worktree add` needs
/// before it will succeed. Identity and signing are pinned per-command so the
/// result does not depend on the machine's own git configuration.
fn init_repo(dir: &Path) {
    std::fs::create_dir_all(dir).expect("scratch repository");
    std::fs::write(dir.join("README.md"), "scratch\n").expect("scratch repository file");
    run_git(dir, &["init", "-q", "-b", "main"]);
    run_git(dir, &["add", "-A"]);
    run_git(dir, &["commit", "-q", "-m", "first"]);
}

fn run_git(dir: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.email=conformance@fleetor.test"])
        .args(["-c", "user.name=Conformance"])
        .args(["-c", "commit.gpgsign=false"])
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(args)
        .output()
        .expect("git must be on PATH for the conformance suite");
    assert!(
        output.status.success(),
        "git {args:?} in {} failed: {}",
        dir.display(),
        String::from_utf8_lossy(&output.stderr),
    );
}
