//! **Harness conformance, checkpoints 8–14** (WP-25, issue #16; M23, C17, C20,
//! C25, C26).
//!
//! The second half of the suite issue #15 opened. Same shape: one pass per
//! registered harness over the same assertions, driven through
//! [`placement::place`] against a scratch layout. Checkpoints 1–7 are the file
//! next door; the driver both share is `tests/conformance/mod.rs`, and its header
//! is where the shape, the rule and the extension point are written down.
//!
//! **The rule is the driver's, unchanged: assert end to end, never against the
//! spec constant.** The spec is the *input* to every assertion here — it says
//! which variable to read, which name to confirm, which key to look for — and
//! what is looked at is the command `place` returns, the files written into a
//! scratch layout, the bytes a real pty received, or the record the seed left
//! behind.
//!
//! **Three of these seven do not pass through `place`, and this file says so
//! where it happens rather than quietly asserting something weaker.** A pane's
//! typing profile is bytes written into a command's pty rather than anything on
//! the command; the command channel is `fleetor_core`'s; the orphan sweep is a
//! private module. Each is asserted through the widest public interface that
//! does reach it, and each carries a doc comment naming what could not be
//! reached and why — the discipline C27 set for checkpoint 5's scrub.

mod conformance;

use std::path::Path;
use std::time::Duration;

use conformance::{
    argv_of, config_states, env_on, files_containing, for_each_registered, pane_of, WORKER_SLOT,
};
use fleetor_core::pane::PaneId;
use fleetor_core::{Command, ALLOWED_COMMANDS};
use fleetor_shell::context_gauge::GaugeSources;
use fleetor_shell::placement::harness::Transport;
use fleetor_shell::placement::MISSING_FLEET_BIN;

/// A body nothing would type by accident, so finding it on the far end of a pty
/// proves it travelled.
const TYPED_BODY: &str = "FLEETOR-CONFORMANCE-TYPED-BODY";

/// The argument a command carries, so "the spelling arrives" can be told from
/// "the spelling arrives and eats its arguments".
const COMMAND_ARGUMENT: &str = "FLEETOR-CONFORMANCE-COMMAND-ARGUMENT";

/// The text a planted transcript holds, so the harvest can be followed by content
/// rather than by filename.
const TRANSCRIPT_MARK: &str = "FLEETOR-CONFORMANCE-TRANSCRIPT";

// --- checkpoint 8 ---------------------------------------------------------------

/// **Checkpoint 8 — outbound reachability.**
///
/// Not a networking detail: a pane that cannot reach the fleet's unix socket has
/// no route to the fleet at all, and `fleet send` typed inside it exits non-zero
/// having reached nothing. **There is no bridge and that is settled** (M28), so
/// the socket is the whole of the route and this checkpoint is about whether a
/// pane of this harness can take it.
///
/// Three things are asserted on the command `place` returns, and the third is the
/// one that is easy to get wrong. The pane is told where the socket is; the
/// `fleet` binary resolves on the PATH it was given; and **the socket it was told
/// about is the fleet's own rather than one under the pane's private `HOME`** —
/// a fenced pane whose socket path followed `HOME` is a mute pane that looks
/// completely healthy, which is the same shape of failure as checkpoint 4's
/// unseeded config dir.
///
/// The last assertion is about the machine rather than the harness, and it is
/// here because it is the only place a pane's inability to talk is ever
/// announced: a machine with no `fleet` binary produces a warning at placement
/// rather than a `command not found` discovered mid-turn (L4).
#[test]
fn checkpoint_8_every_seat_is_given_the_fleets_own_socket_and_a_fleet_binary_to_reach_it_with() {
    for_each_registered("cp8", |pass| {
        let outbound = &pass.spec.outbound;
        assert!(
            outbound.socket_reachable,
            "{}: a harness whose panes cannot connect the fleet socket is a fleet of mute \
             panes, and there is no bridge to give them (M28) — this is not something a \
             registration may leave open",
            pass.spec.name,
        );
        if outbound.sandboxed {
            assert!(
                !outbound.reachability_keys.is_empty(),
                "{}: this harness confines its panes and still claims the socket is \
                 reachable, while naming nothing that makes it so — checkpoint 8 stubbed",
                pass.spec.name,
            );
        } else {
            assert!(
                outbound.reachability_keys.is_empty(),
                "{}: this harness confines nothing, so there is no refusal for a \
                 reachability key to lift — a key here describes a sandbox the same \
                 checkpoint says is absent",
                pass.spec.name,
            );
        }
        // Whatever a harness does name has to have been written where the pane will
        // read it, or the reachability it buys is a claim rather than a fact. The
        // discipline checkpoint 3 applies to `sandbox_keys`, for the same reason.
        for (key, value) in outbound.reachability_keys {
            let dir = pass.config_dir(&pass.worker);
            assert!(
                config_states(&dir, key, value).is_some(),
                "{}: reachability key {key} = {value} is in the spec and not in {}",
                pass.spec.name,
                dir.display(),
            );
        }

        let socket = pass.layout.socket();
        for (seat, placed) in pass.seats() {
            let named = env_on(placed, "FLEET_SOCKET").unwrap_or_else(|| {
                panic!(
                    "{}/{seat}: the pane is told nothing about the socket, so `fleet send` \
                     inside it refuses before it does anything",
                    pass.spec.name,
                )
            });
            assert_eq!(
                Path::new(&named),
                socket,
                "{}/{seat}: the pane was pointed at a socket that is not the one this fleet \
                 listens on",
                pass.spec.name,
            );
            // Containment is checked on the directory rather than on the socket
            // itself: nothing is listening yet, so the socket is a path that does
            // not exist, and an unresolvable path cannot be compared against a
            // resolved root.
            let holding = Path::new(&named)
                .parent()
                .unwrap_or_else(|| panic!("{}/{seat}: the socket has no directory", pass.spec.name));
            assert!(
                pass.is_contained(holding),
                "{}/{seat}: the socket named on the command is outside the layout \
                 placement was handed: {named}",
                pass.spec.name,
            );

            // The Fence gives a fenced pane a private `HOME`; a socket path that
            // followed it would point at a directory nothing is listening in.
            if let Some(home) = env_on(placed, "HOME") {
                assert!(
                    !Path::new(&named).starts_with(&home),
                    "{}/{seat}: the socket is inside this pane's own HOME ({home}), so the \
                     pane would connect to nothing while looking healthy",
                    pass.spec.name,
                );
            }

            // And the CLI that opens it has to resolve by name.
            let fleet_bin = pass
                .host
                .fleet_bin
                .as_deref()
                .expect("the pass hands placement a machine that has a fleet binary");
            let rung = fleet_bin.parent().expect("the fleet binary lives in a directory");
            let path = env_on(placed, "PATH")
                .unwrap_or_else(|| panic!("{}/{seat}: the pane is given no PATH", pass.spec.name));
            assert!(
                path.split(':').any(|dir| Path::new(dir) == rung),
                "{}/{seat}: `fleet` does not resolve on this pane's PATH, so the socket it \
                 was told about is unreachable by name — PATH was {path:?}",
                pass.spec.name,
            );
        }

        // The machine-with-no-CLI case is announced rather than discovered.
        let mute = pass.place_orch_without_fleet_bin();
        assert!(
            mute.notices.iter().any(|(_, text)| text == MISSING_FLEET_BIN),
            "{}: a pane with no `fleet` on its PATH looks alive and cannot talk, and \
             placement said nothing about it: {:?}",
            pass.spec.name,
            mute.notices,
        );
        assert!(
            !pass.orch.notices.iter().any(|(_, text)| text == MISSING_FLEET_BIN),
            "{}: this pass's machine has a fleet binary, and placement warned about it \
             anyway — the notice does not depend on the fact it names",
            pass.spec.name,
        );
    });
}

// --- checkpoint 9 ---------------------------------------------------------------

/// **Checkpoint 9 — typing profile.**
///
/// How bytes get from the hub into this harness's input box and become a
/// *submitted* turn. Asserted against a real pty with a real process on the far
/// end: the profile's framing is wrapped around a body nothing would produce by
/// accident, and the far end echoes back only lines it was given as submitted —
/// so one assertion covers both the framing and the submit byte, and a profile
/// that framed correctly and never submitted fails with the collected output in
/// hand.
///
/// **Not observable through `place`, and this says so rather than asserting
/// something weaker.** `place` returns a command; the profile is the bytes
/// written into that command's pty afterwards, and `pty.rs` holds them as private
/// constants with no public spelling. The widest public interface that reaches
/// them is `PaneRegistry::write_paste` over a real pty, which is what the driver's
/// `echo_of_a_paste` drives — through a placement, with the stand-in pane program
/// checkpoint 1's own doc comment sets aside for exactly this, so no tokens are
/// spent. What the vendor's *own* binary does with those bytes is C13's tier.
///
/// **There is no startup wait here, and its absence is deliberate** (C26). A
/// per-harness "wait N seconds after spawn before typing" is a new delay between
/// `fleet send` and a pty, which Tier 1.4 forbids and which has been argued and
/// lost twice; the answer for a harness that needs waiting for is readiness
/// detection on the bring-up path, which is a mechanism rather than a number.
/// The one gap that does exist — between closing the paste and submitting — is a
/// widening of `pty::SUBMIT_GAP`, which the invariant already sanctions (D-034),
/// and it is asserted below as a floor: the write cannot have returned sooner
/// than the profile says it takes.
#[test]
fn checkpoint_9_a_paste_reaches_a_real_pty_framed_as_the_profile_says_and_submitted() {
    for_each_registered("cp9", |pass| {
        let typing = &pass.spec.typing;
        assert!(
            !typing.submit_bytes.is_empty(),
            "{}: a profile that submits nothing types into an input box forever while \
             every `fleet send` reports `accepted`",
            pass.spec.name,
        );
        if typing.bracketed_paste {
            assert!(
                !typing.paste_start.is_empty() && !typing.paste_end.is_empty(),
                "{}: bracketed paste with no markers is not bracketed paste — a multi-line \
                 body would arrive as N submitted turns",
                pass.spec.name,
            );
        }

        let (seen, took) = pass.echo_of_a_paste(TYPED_BODY);

        // What the far end should have read, built from the profile rather than
        // from anything this file knows about a vendor.
        let framed = if typing.bracketed_paste {
            format!(
                "{}{TYPED_BODY}{}",
                String::from_utf8_lossy(typing.paste_start),
                String::from_utf8_lossy(typing.paste_end),
            )
        } else {
            TYPED_BODY.to_string()
        };
        assert!(
            seen.contains(&format!("echo: {framed}")),
            "{}: the far end did not read the profile's framing as one submitted line — \
             it saw {seen:?}",
            pass.spec.name,
        );

        assert!(
            took >= Duration::from_millis(typing.submit_gap_ms),
            "{}: the write returned in {took:?}, sooner than the {}ms this profile puts \
             between closing the paste and submitting it",
            pass.spec.name,
            typing.submit_gap_ms,
        );
    });
}

// --- checkpoint 10 --------------------------------------------------------------

/// **Checkpoint 10 — command-channel spellings.**
///
/// `fleet cmd` carries a small allowlist (Tier 2), and this is how each entry is
/// spelled for this harness. Two failures, and the table exists to make the
/// second visible: an allowlisted command with no row is a pane silently sent
/// something that does nothing, and a row for a command the fleet does not allow
/// is a spelling nothing will ever type.
///
/// The end-to-end half is the exact bytes: what a `Command` will type into the
/// receiving pane is this harness's spelling, unframed, with its arguments
/// intact. **Unframed is load-bearing** — a slash command is only a command when
/// `/` is the first character in the input box, so this is the one delivery in the
/// product that is deliberately not attributed to its sender.
///
/// **Not observable through `place`, and asserted on a real pty for the same
/// reason checkpoint 9 is** (C28, C35). The end-to-end half used to compare
/// `fleetor_core::Command::keystrokes()` against this harness's spelling —
/// **which was the wrong subject.** `keystrokes()` is harness-free: it is the
/// canonical word the fleet allowlisted, so that assertion tested what the fleet
/// decided and not what the harness types, and it held only because Claude Code's
/// table is the identity map. A harness whose table was not the identity would
/// have been typed correctly by `pty::write_command` and asserted wrongly here —
/// the gap C35 recorded and left open.
///
/// It is closed by driving the production path: `keystrokes()`, the exact string
/// the hub hands `deliver`, goes into `PaneRegistry::write_command` over a real
/// pty with the stand-in pane program on the far end, and the far end must read
/// **this harness's spelling**. The lookup, the framing and the submit byte are
/// all production's, so a `spell` that returned its argument fails against any
/// non-identity table.
#[test]
fn checkpoint_10_every_allowlisted_command_has_a_spelling_and_it_is_typed_unframed() {
    for_each_registered("cp10", |pass| {
        let spellings = pass.spec.commands.spellings;
        for allowed in ALLOWED_COMMANDS {
            assert!(
                spellings.iter().any(|(canonical, _)| *canonical == allowed),
                "{}: the fleet allows {allowed} and this harness has no spelling for it — a \
                 pane sent it would be sent something that does nothing",
                pass.spec.name,
            );
        }
        for (canonical, spelling) in spellings {
            assert!(
                ALLOWED_COMMANDS.contains(canonical),
                "{}: {canonical} is spelled here and is not a command the fleet allows — \
                 widening the allowlist is Tier 2 and costs a row in `ALLOWED_COMMANDS` \
                 first",
                pass.spec.name,
            );
            assert!(
                spelling.starts_with('/'),
                "{}: {canonical} is spelled {spelling:?}, which is not a slash command",
                pass.spec.name,
            );
        }

        for (canonical, spelling) in spellings {
            let command = Command::new(
                PaneId::Orch,
                PaneId::Worker(WORKER_SLOT),
                format!("{canonical} {COMMAND_ARGUMENT}"),
                "the conformance suite is checking the command channel",
            )
            .unwrap_or_else(|why| {
                panic!("{}: the fleet refused its own allowlisted {canonical}: {why}", pass.spec.name)
            });

            // What the far end must have read: the harness's *spelling*, its
            // argument intact, inside this profile's paste framing and submitted.
            // Built from the spec rather than from anything this file knows about
            // a vendor, exactly as checkpoint 9 builds its own.
            let typing = &pass.spec.typing;
            let spelled = format!("{spelling} {COMMAND_ARGUMENT}");
            let framed = if typing.bracketed_paste {
                format!(
                    "{}{spelled}{}",
                    String::from_utf8_lossy(typing.paste_start),
                    String::from_utf8_lossy(typing.paste_end),
                )
            } else {
                spelled
            };

            let seen = pass.echo_of_a_command(command.keystrokes());
            assert!(
                seen.contains(&format!("echo: {framed}")),
                "{}: the far end did not read this harness's spelling of {canonical}. The \
                 hub handed `write_command` {:?} — the canonical word — and this harness \
                 spells it {spelling:?}, so that is what had to reach the pty: its \
                 argument intact, the slash still in column 0, and as one submitted line. \
                 It saw {seen:?}",
                pass.spec.name,
                command.keystrokes(),
            );
        }
    });
}

// --- checkpoint 11 --------------------------------------------------------------

/// **Checkpoint 11 — context-gauge source and window constant.**
///
/// Where the live gauge reads a worker's usage from, and what it divides by. The
/// window is the more dangerous of the two, and D-054's rule is the whole
/// checkpoint: **one number, two consumers.** The fleet asserts a window because
/// the vendor does not recognize the worker model's name, and it exports the
/// identical number to the pane — so the vendor's own bookkeeping and the fleet's
/// display cannot disagree. A gauge reading 100% while the pane believes 40% is
/// exactly the quiet lie this product exists to avoid.
///
/// **An unavailable gauge says unavailable.** Sampled at the moment `place`
/// returns — before the pane's process exists, which is precisely when a
/// `fleet roster` is most likely to land — the answer is absent, never a number
/// reconstructed from what the fleet sent. That is the assertion this checkpoint
/// would be decoration without: a synthesized estimate is the number that looks
/// right and is wrong.
///
/// **What could not be asserted, stated plainly.** The *positive* path — a real
/// usage figure read back out of a transcript — cannot be built through the seam,
/// because the naming of the per-project directory under `transcript.subdir` is
/// not one of the fourteen answers. A test that planted a transcript where the
/// gauge would find it would have to re-derive one vendor's slug rule and call it
/// general. The gap is real and it is checkpoint 13's to close if a later harness
/// needs it.
#[test]
fn checkpoint_11_the_window_the_pane_is_told_is_the_gauges_own_and_an_absent_reading_says_so() {
    for_each_registered("cp11", |pass| {
        let gauge = &pass.spec.gauge;
        assert_eq!(
            gauge.window_tokens.is_some(),
            gauge.window_env.is_some(),
            "{}: a window the fleet asserts with no way to tell the pane about it is the \
             two-numbers disagreement D-054 exists to prevent",
            pass.spec.name,
        );

        if let (Some(window), Some(var)) = (gauge.window_tokens, gauge.window_env) {
            assert_eq!(
                env_on(&pass.worker, var).as_deref(),
                Some(window.to_string().as_str()),
                "{}: the window the pane is told is not the one the gauge divides by",
                pass.spec.name,
            );
            assert_eq!(
                env_on(&pass.orch, var),
                None,
                "{}: the operator's own seat runs their account and their model, so the \
                 fleet has no window to assert for it",
                pass.spec.name,
            );
        }

        if gauge.reads_transcript {
            let source = pass.worker.gauge.as_ref().unwrap_or_else(|| {
                panic!(
                    "{}: this harness's usage is read out of the pane's own transcript and \
                     placement returned no source to read it from",
                    pass.spec.name,
                )
            });
            assert_eq!(
                source.config_dir,
                pass.config_dir(&pass.worker),
                "{}: the gauge was pointed at a different configuration directory than the \
                 pane was — the trust flag, the delivery path and the gauge have to agree \
                 on one spelling of a pane",
                pass.spec.name,
            );
            assert_eq!(
                source.cwd, pass.worker_cwd,
                "{}: the gauge was pointed at a different working directory than the pane \
                 was placed in",
                pass.spec.name,
            );
            assert!(
                pass.is_contained(&source.config_dir),
                "{}: the gauge reads from outside the layout placement was handed: {}",
                pass.spec.name,
                source.config_dir.display(),
            );
        }
        assert!(
            pass.orch.gauge.is_none(),
            "{}: the attended seat is the operator's own and sampling its transcript is an \
             ownership call above this seam",
            pass.spec.name,
        );

        // Nothing has run yet, so there is nothing honest to report.
        let sources = GaugeSources::default();
        let pane = pane_of("worker");
        if let Some(source) = pass.worker.gauge.as_ref() {
            sources.record(pane, source.clone());
        }
        assert!(
            sources.sample(pane).is_none(),
            "{}: the gauge reported a number for a pane whose process does not exist yet — \
             an unavailable gauge says unavailable, it never estimates (D-054)",
            pass.spec.name,
        );
    });
}

// --- checkpoint 12 --------------------------------------------------------------

/// **Checkpoint 12 — orphan-sweep process names.**
///
/// The `comm` suffixes the sweep confirms before it signals a pid. What is
/// asserted here is the join that actually drifts: **every name the sweep would
/// confirm has to match the program `place` really launches.** A harness added to
/// the registry whose program no suffix matches leaks every crashed pane of it as
/// a terminal-less orphan — which is the live bug the roadmap names, one literal
/// `"claude"` standing where a per-harness answer belongs.
///
/// **The safety property could not be asserted here, and it matters more than the
/// name.** The sweep does not search by process name: it reads pids the fleet
/// itself recorded and uses these suffixes only to confirm that a recorded pid is
/// still the process it was, so the operator's own harness is never at risk and a
/// missing name leaks rather than kills. `orphans` is a private module whose
/// every function is private too, unreachable from `tests/` at all, so this suite
/// cannot drive a pid it controls past a wrong name — and reaching past the
/// interface to make it public would be a production change for a test's
/// convenience. That property is covered where it lives, by the crate-internal
/// tests in `orphans.rs` that spawn a real process and assert the sweep leaves it
/// alone under the wrong name. **Cleanup more dangerous than the leak is the
/// failure worth guarding**, and this file does not currently guard it.
#[test]
fn checkpoint_12_the_sweep_can_confirm_the_program_every_seat_is_actually_launched_with() {
    for_each_registered("cp12", |pass| {
        let suffixes = pass.spec.orphans.comm_suffixes;
        assert!(
            !suffixes.is_empty(),
            "{}: a harness the sweep cannot name leaks every crashed pane of it as a \
             terminal-less orphan that keeps spending",
            pass.spec.name,
        );
        for suffix in suffixes {
            assert!(
                !suffix.trim().is_empty(),
                "{}: an empty comm suffix matches every process the fleet ever recorded",
                pass.spec.name,
            );
        }

        for (seat, placed) in pass.seats() {
            let argv = argv_of(placed);
            let program = argv.first().unwrap_or_else(|| {
                panic!("{}/{seat}: the command runs no program at all", pass.spec.name)
            });
            let comm = Path::new(program)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| program.clone());
            assert!(
                suffixes.iter().any(|suffix| comm.ends_with(suffix)),
                "{}/{seat}: this pane is launched as {comm:?} and the sweep confirms \
                 {suffixes:?} — a crashed one would never be recognized as this harness's",
                pass.spec.name,
            );
        }
    });
}

// --- checkpoint 13 --------------------------------------------------------------

/// **Checkpoint 13 — transcript location and format.**
///
/// What a pane leaves behind, where, and how the archive is allowed to take it.
/// **Nothing is normalized** (M24): the archive keeps each harness's raw format,
/// so what this asserts is location and transport, never content.
///
/// The location half is a containment claim: a pane's transcripts live under the
/// configuration directory that pane was actually pointed at, hence inside the
/// layout — which is what makes `rm -rf ~/.fleetor` remove everything FLEETOR
/// made, and what lets rotation archive a run without knowing anything about a
/// vendor.
///
/// The transport half is driven through rotation, the public path to the harvest:
/// a transcript planted where a pane of this harness would leave one is taken into
/// the run, a file that is not one is left alone — which is what pins the
/// extension rather than assuming it — and **nothing is left behind**, because a
/// harvest that copied would make every archive accumulate its predecessors.
///
/// `place` does not create the transcript directory and should not: it makes the
/// config dir and seeds it, and the harness's own process makes this one when it
/// first writes. So the test plants one, and deliberately does not re-derive the
/// per-project directory's name — see checkpoint 11's note on the same gap.
#[test]
fn checkpoint_13_transcripts_live_under_the_panes_config_dir_and_the_harvest_moves_them() {
    for_each_registered("cp13", |pass| {
        let transcript = &pass.spec.transcript;
        // **`subdir` is no longer required to be non-empty, and that is a
        // reshape rather than a loosening** (#33). What this trio is for is a
        // Critic reading a run cold: it has to be able to *find* a transcript and
        // to *name* what it is holding, and those are `file_ext` and `format`.
        // `subdir` is neither — it is a path fragment under the pane's own
        // configuration directory, and the empty string is a real answer meaning
        // "the configuration directory itself". Claude Code keeps its sessions in
        // `projects/`, so requiring a non-empty subdirectory read as a general
        // rule while it was the only registered harness; it is that vendor's
        // layout. The property the assertion was reaching for is asserted below
        // and unchanged: wherever the directory is, it is *under* the
        // configuration directory this pane was pointed at, hence inside the
        // layout.
        for (what, value) in [("file_ext", transcript.file_ext), ("format", transcript.format)] {
            assert!(
                !value.trim().is_empty(),
                "{}: checkpoint 13's {what} is empty, and a run whose transcripts cannot be \
                 found or named is a run a Critic reads cold and cannot place",
                pass.spec.name,
            );
        }
        // **The transport is asserted as a mechanism, not as a permission** (#39).
        // The field was `file_move_is_safe: bool` and this assertion required it to
        // be `true`, which refused codex correctly: the harvest took every
        // transcript with a plain rename, and a WAL-mode database is `db` plus
        // `-wal` plus `-shm`, so renaming the `.sqlite` alone leaves every
        // committed transaction behind. The harvest now reads
        // `Transport` and has an arm for each, so what this checks is
        // that the declared arm *works* — driven through rotation below, and for a
        // database with the control that proves the mechanism is neither a rename
        // nor a copy. A harness declaring a transport nobody implements is a
        // compile error in `runs::take_transcript` rather than a refusal here.
        match transcript.transport {
            Transport::Rename | Transport::SqliteBackup => {}
        }

        // One transcript per seat, and one file beside it that is not one.
        let planted: Vec<(&str, std::path::PathBuf, std::path::PathBuf)> = pass
            .seats()
            .iter()
            .map(|(seat, placed)| {
                let kept = pass.plant_transcript(
                    placed,
                    "a-project",
                    &format!("session.{}", transcript.file_ext),
                    TRANSCRIPT_MARK,
                );
                let other = pass.plant_transcript(
                    placed,
                    "a-project",
                    "session.not-a-transcript",
                    TRANSCRIPT_MARK,
                );
                (*seat, kept, other)
            })
            .collect();

        for (seat, placed) in pass.seats() {
            let dir = pass.transcript_dir(placed);
            assert!(
                dir.starts_with(pass.config_dir(placed)),
                "{}/{seat}: transcripts live outside the configuration directory this pane \
                 was pointed at: {}",
                pass.spec.name,
                dir.display(),
            );
            assert!(
                pass.is_contained(&dir),
                "{}/{seat}: transcripts live outside the layout placement was handed, so \
                 rotation cannot archive them and removing the fleet's own tree would not \
                 remove them: {}",
                pass.spec.name,
                dir.display(),
            );
        }

        // **The control, taken before the harvest, because the harvest consumes
        // its subject.** For a harness whose transport is a database backup, this
        // is the whole reason the backup exists: a plain `fs::copy` of the
        // `.sqlite` — the naive implementation, and the one a `file_move_is_safe:
        // false` flag would have left a later author to invent — produces a file
        // that does *not* contain the committed row, because the row is in the
        // write-ahead log. If this ever stops failing to find the mark, the
        // fixture has stopped being a live database and the assertions below are
        // asserting nothing.
        if transcript.transport == Transport::SqliteBackup {
            for (seat, kept, _) in &planted {
                let torn = pass.root.join(format!("a-file-copy-of-{seat}.{}", transcript.file_ext));
                std::fs::copy(kept, &torn).expect("copying the planted database");
                let bytes = std::fs::read(&torn).expect("reading the copy back");
                assert!(
                    !bytes.windows(TRANSCRIPT_MARK.len()).any(|w| w == TRANSCRIPT_MARK.as_bytes()),
                    "{}/{seat}: a plain file copy of this transcript already contains the \
                     committed row, so it is not a live write-ahead-logged database and this \
                     checkpoint cannot tell a backup from a copy",
                    pass.spec.name,
                );
            }
        }

        let run = pass.harvest_into_a_run();
        let taken = files_containing(&run, TRANSCRIPT_MARK);
        assert_eq!(
            taken.len(),
            planted.len(),
            "{}: the harvest took {taken:?} out of {} planted transcripts",
            pass.spec.name,
            planted.len(),
        );
        for (seat, kept, other) in &planted {
            let filed = run
                .join("transcripts")
                .join(pane_of(seat).to_string())
                .join(format!("session.{}", transcript.file_ext));
            assert!(
                filed.is_file(),
                "{}/{seat}: this pane's transcript is not in the run at {}",
                pass.spec.name,
                filed.display(),
            );
            assert!(
                !kept.exists(),
                "{}/{seat}: the harvest copied instead of moving, so the next run's archive \
                 will hold this one's transcripts too",
                pass.spec.name,
            );
            assert!(
                other.exists(),
                "{}/{seat}: the harvest took a file that is not a `{}` transcript",
                pass.spec.name,
                transcript.file_ext,
            );

            // **The database half, and it is three claims rather than one.** That
            // the archived file contains the mark is already asserted above, by
            // the same count every harness gets; what is specific here is *how* it
            // got there. It is a database SQLite will open, holding the row that
            // was only ever in the write-ahead log — which the control above
            // proved a file copy does not carry — and it arrives with no journal
            // beside it, because a `-wal` left in an archive is a second file the
            // reader has to know to keep. Nothing of the original survives, the
            // log and its shared-memory index included: those hold the same rows,
            // and a fragment of this run's evidence left in a pane's configuration
            // directory would be harvested into the *next* run's archive.
            if transcript.transport == Transport::SqliteBackup {
                let conn = rusqlite::Connection::open(&filed).unwrap_or_else(|e| {
                    panic!("{}/{seat}: the archived transcript is not a database SQLite \
                            will open, so the harvest did not back it up: {e}", pass.spec.name)
                });
                let rows: i64 = conn
                    .query_row("SELECT count(*) FROM thread_items WHERE item_json = ?1", [TRANSCRIPT_MARK], |r| r.get(0))
                    .unwrap_or_else(|e| {
                        panic!("{}/{seat}: the archived database has no readable item — a \
                                rename of the main file alone would look exactly like \
                                this: {e}", pass.spec.name)
                    });
                assert_eq!(rows, 1, "{}/{seat}: the archived database lost the committed item", pass.spec.name);

                for suffix in ["-wal", "-shm"] {
                    let beside = filed.with_file_name(format!(
                        "{}{suffix}",
                        filed.file_name().unwrap_or_default().to_string_lossy(),
                    ));
                    assert!(
                        !beside.exists(),
                        "{}/{seat}: the archive holds a `{suffix}` beside the transcript, so \
                         it is a file copy of a live database rather than a backup of one",
                        pass.spec.name,
                    );
                    let left = kept.with_file_name(format!(
                        "{}{suffix}",
                        kept.file_name().unwrap_or_default().to_string_lossy(),
                    ));
                    assert!(
                        !left.exists(),
                        "{}/{seat}: the harvest took the database and left its `{suffix}` \
                         behind, so the next run's archive will hold a fragment of this one",
                        pass.spec.name,
                    );
                }
            }
        }
    });
}

// --- checkpoint 14 --------------------------------------------------------------

/// **Checkpoint 14 — project identity and trust seeding** (C17, generalized from
/// "project-key canonicalization").
///
/// **Both halves, because the key's shape is only half the problem.** Every
/// harness parks a fresh pane on a first-run trust gate, and a pane sitting on one
/// looks completely healthy while every `fleet send` reports `accepted` (L1).
/// Getting the key wrong and not writing the flag at all produce the identical
/// symptom, which is why they are one checkpoint — and why a checkpoint covering
/// only the key would let the next harness discover the second half as a fleet of
/// silent panes.
///
/// The key's shape is asserted as behaviour, not as a string: two spellings that
/// reach the same directory key the same when the harness canonicalizes, and the
/// key an `exact_path_match` harness records is the pane's **own** working
/// directory — a harness resolving trust by repository root would have written
/// the target instead, and seeding it per worktree would write rows it never
/// reads.
///
/// The gate is asserted as *seeded past* rather than merely mentioned: every trust
/// key is present under this pane's record and answers affirmatively. A record
/// that named the key and said `false` would satisfy a text search and park the
/// pane exactly as an absent one would.
///
/// **The container is searched for, not navigated to.** Which object a harness
/// keeps its per-project records in is not one of the fourteen answers — only the
/// file and the keys are — so a checkpoint that walked a named path would be
/// asserting one vendor's file layout under a general name.
#[test]
fn checkpoint_14_the_key_is_the_panes_own_directory_and_the_first_run_gate_is_seeded_past() {
    for_each_registered("cp14", |pass| {
        let identity = &pass.spec.project_identity;
        assert!(
            !identity.trust_file.trim().is_empty(),
            "{}: a harness with no trust file has nowhere to record that a pane may start",
            pass.spec.name,
        );
        assert!(
            !identity.trust_keys.is_empty(),
            "{}: a harness that writes no trust record parks every fresh pane on a \
             first-run dialog while every `fleet send` reports `accepted` — the key's shape \
             is only half of this checkpoint (C17)",
            pass.spec.name,
        );

        for (seat, placed) in pass.seats() {
            let cwd = pass.cwd_of(seat);
            let key = pass.harness.project_key(&cwd);
            assert!(
                Path::new(&key).is_absolute(),
                "{}/{seat}: {key:?} is not an absolute path, so nothing a pane reports about \
                 its own cwd could ever match it",
                pass.spec.name,
            );

            if identity.canonicalize {
                // A child's own cwd comes back resolved — on macOS `/tmp` and
                // `/var` are symlinks — so an unresolved key would never match.
                let link = pass.root.join(format!("another-way-to-{seat}"));
                let _ = std::fs::remove_file(&link);
                std::os::unix::fs::symlink(&cwd, &link).expect("a scratch symlink");
                assert_eq!(
                    pass.harness.project_key(&link),
                    key,
                    "{}/{seat}: two spellings of the same directory key differently, so a \
                     pane reached by one would never find the record written under the other",
                    pass.spec.name,
                );
            }

            let text = pass.config_text(placed, identity.trust_file);
            let recorded = document(&text).unwrap_or_else(|why| {
                panic!("{}/{seat}: {} is not readable: {why}", pass.spec.name, identity.trust_file)
            });

            let record = record_under(&recorded, &key).unwrap_or_else(|| {
                panic!(
                    "{}/{seat}: {} records nothing under {key} — this pane starts on a \
                     first-run dialog and reports `accepted` to everything sent to it",
                    pass.spec.name, identity.trust_file,
                )
            });
            for trust_key in identity.trust_keys {
                let answer = record.get(trust_key).unwrap_or_else(|| {
                    panic!(
                        "{}/{seat}: {trust_key} is not set for {key}, so the first-run gate \
                         is not seeded past",
                        pass.spec.name,
                    )
                });
                assert!(
                    is_affirmative(answer),
                    "{}/{seat}: {trust_key} is recorded as {answer} for {key} — a gate \
                     answered in the negative parks the pane exactly as an unanswered one \
                     does",
                    pass.spec.name,
                );
            }

            if identity.exact_path_match {
                // **Every directory this seed trusted is one placement was handed**
                // (#33). This assertion used to be narrower and wrong: it said a
                // record under the *repository root* proved the harness had
                // resolved trust by root instead of by cwd, and had written a row
                // it will never read. That is true of a harness that records one
                // key. Codex records two — the canonicalized cwd and the git root
                // that cwd resolves to — and for a linked worktree the second is
                // the main repository, which it does read (C34, row 11): a pane
                // whose seed carries only the worktree key parks on the first-run
                // gate the moment its cwd is one directory below it.
                //
                // So what the old form actually caught — the positive, that this
                // pane's own directory is recorded — is asserted above and
                // unchanged, and this is the containment property it was reaching
                // for and did not state: a trust record is a directory a pane may
                // start in without a human, so a harness that trusted the
                // operator's home, or `/`, would have passed the old assertion and
                // fails this one.
                for (recorded_key, _) in trusted_directories(&recorded, identity.trust_keys) {
                    assert!(
                        pass.is_contained(Path::new(&recorded_key)),
                        "{}/{seat}: the seed recorded trust for {recorded_key}, which is \
                         outside the layout and the target placement was handed — a first-run \
                         gate answered for a directory nobody asked about",
                        pass.spec.name,
                    );
                }
            }
        }

        // The re-seed, which is the single most likely way to reintroduce L1: the
        // trust record is keyed by path, so a fleet pointed at a new target has to
        // write one for the *new* cwd. Checkpoint 4 asserts the old one survives;
        // this asserts the new one arrives.
        let (other, placed) = pass.place_worker_against("cp14-second-repo");
        let moved_to = pass.layout.worktree(&other, WORKER_SLOT);
        let moved_to = if moved_to.join(".git").exists() { moved_to } else { other };
        let after = document(&pass.config_text(&placed, identity.trust_file))
            .expect("the trust file is still readable after a target switch");
        let key = pass.harness.project_key(&moved_to);
        let record = record_under(&after, &key).unwrap_or_else(|| {
            panic!(
                "{}: switching targets left the new working directory {key} with no trust \
                 record, so every pane of the new target starts on a dialog",
                pass.spec.name,
            )
        });
        for trust_key in identity.trust_keys {
            assert!(
                record.get(trust_key).is_some_and(is_affirmative),
                "{}: {trust_key} was not re-applied for {key} after a target switch",
                pass.spec.name,
            );
        }
    });
}

/// **This harness's trust file, as a value that can be searched** — whatever
/// document format it is in.
///
/// **Reshaped in #33, and this is the shape it was in.** It was
/// `serde_json::from_str`, which is not a checkpoint answer: `trust_file` names a
/// *file*, and which document format that file is in is the vendor's — Claude
/// Code's is JSON and codex's is the same `config.toml` the rest of its seed goes
/// into. A checkpoint that could only read one of them was asserting one vendor's
/// format under a general name, and would have failed a correct harness on its
/// first line.
///
/// Both are parsed into the same `serde_json::Value`, so everything below —
/// [`record_under`], [`is_affirmative`], [`trusted_directories`] — stays one
/// implementation rather than one per format. That is the point: what a checkpoint
/// asserts is *what the record says*, and the encoding it says it in is not a
/// property of the seam.
fn document(text: &str) -> Result<serde_json::Value, String> {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
        return Ok(value);
    }
    let doc =
        text.parse::<toml_edit::DocumentMut>().map_err(|e| format!("neither JSON nor TOML: {e}"))?;
    Ok(as_value(doc.as_item()))
}

/// One TOML item as the same `serde_json::Value` a JSON document would parse to.
///
/// Written out rather than reached for through a serde bridge because the mapping
/// is three lines and the alternative is a feature flag on a production
/// dependency, turned on for a test. Only the shapes a trust record can be in
/// matter: tables become objects, arrays become arrays, and a scalar becomes
/// whatever [`is_affirmative`] can judge.
fn as_value(item: &toml_edit::Item) -> serde_json::Value {
    use toml_edit::{Item, Value as Toml};
    match item {
        Item::None => serde_json::Value::Null,
        Item::Value(Toml::String(s)) => serde_json::Value::String(s.value().clone()),
        Item::Value(Toml::Integer(i)) => serde_json::Value::from(*i.value()),
        Item::Value(Toml::Float(f)) => serde_json::Value::from(*f.value()),
        Item::Value(Toml::Boolean(b)) => serde_json::Value::Bool(*b.value()),
        Item::Value(Toml::Datetime(d)) => serde_json::Value::String(d.value().to_string()),
        Item::Value(Toml::Array(array)) => serde_json::Value::Array(
            array.iter().map(|v| as_value(&Item::Value(v.clone()))).collect(),
        ),
        Item::Value(Toml::InlineTable(table)) => serde_json::Value::Object(
            table.iter().map(|(k, v)| (k.to_string(), as_value(&Item::Value(v.clone())))).collect(),
        ),
        Item::Table(table) => serde_json::Value::Object(
            table.iter().map(|(k, v)| (k.to_string(), as_value(v))).collect(),
        ),
        Item::ArrayOfTables(tables) => serde_json::Value::Array(
            tables.iter().map(|t| as_value(&Item::Table(t.clone()))).collect(),
        ),
    }
}

/// Every directory this document has recorded a trust answer for, with the record.
///
/// **Found by the trust keys rather than by walking a named path**, for
/// [`record_under`]'s reason: which object a harness keeps its per-project records
/// in is not one of the fourteen answers, so a checkpoint that navigated to one
/// would be asserting a vendor's file layout. What *is* an answer is
/// `trust_keys` — so any object that answers one of them is a trust record, and
/// the name it hangs under is the directory it trusts.
fn trusted_directories(
    value: &serde_json::Value,
    trust_keys: &[&str],
) -> Vec<(String, serde_json::Value)> {
    let mut found = Vec::new();
    collect_trusted(value, trust_keys, &mut found);
    found
}

fn collect_trusted(
    value: &serde_json::Value,
    trust_keys: &[&str],
    found: &mut Vec<(String, serde_json::Value)>,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (name, nested) in map {
                let answers = nested
                    .as_object()
                    .is_some_and(|record| trust_keys.iter().any(|key| record.contains_key(*key)));
                if answers && Path::new(name).is_absolute() {
                    found.push((name.clone(), nested.clone()));
                }
                collect_trusted(nested, trust_keys, found);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_trusted(item, trust_keys, found);
            }
        }
        _ => {}
    }
}

/// The object this document records under `key`, wherever it keeps it.
///
/// **Searched rather than navigated**, for the reason checkpoint 14's doc comment
/// gives: the container is not one of the fourteen answers.
fn record_under(value: &serde_json::Value, key: &str) -> Option<serde_json::Value> {
    match value {
        serde_json::Value::Object(map) => map
            .get(key)
            .cloned()
            .or_else(|| map.values().find_map(|nested| record_under(nested, key))),
        serde_json::Value::Array(items) => items.iter().find_map(|item| record_under(item, key)),
        _ => None,
    }
}

/// Whether a recorded answer actually opens the gate.
///
/// A boolean `true` for one harness, a word like `"trusted"` for another — what
/// they have in common is what this refuses: `false`, `null`, an empty string and
/// a zero are all a gate answered in the negative, and all of them satisfy a test
/// that only looked for the key.
fn is_affirmative(answer: &serde_json::Value) -> bool {
    match answer {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(yes) => *yes,
        serde_json::Value::String(text) => !text.trim().is_empty(),
        serde_json::Value::Number(n) => n.as_f64().is_some_and(|v| v != 0.0),
        serde_json::Value::Array(items) => !items.is_empty(),
        serde_json::Value::Object(map) => !map.is_empty(),
    }
}
