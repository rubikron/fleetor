//! Startup orphan reconciliation for the panes fleetor-shell spawns.
//!
//! Close-time teardown (`lib.rs`'s window-close and `RunEvent::Exit` handlers)
//! only runs if the process gets a chance to run it — Force Quit, `kill -9`,
//! and a crash all skip it, and pty.rs's own warning applies a day later just
//! as much as it does at close: a leaked `claude` with no terminal and nobody
//! watching is a bill that keeps growing. This is what bounds that leak to "one
//! session, until the app is next opened" instead of "forever": every spawn and
//! reap durably records the live pane pids, and the next launch kills whatever
//! the last one left running before it spawns anything of its own.

use std::path::{Path, PathBuf};

use crate::fleet;

/// The real registry location. `pub(crate)` so [`crate::pty::PaneRegistry`] can
/// hand it to a live app's registry as its default, while a test builds its own
/// `PaneRegistry` around a scratch path instead — nothing in this crate's test
/// suite should be writing into the operator's real `~/.fleetor`.
pub(crate) fn registry_path() -> PathBuf {
    fleet::layout().shell().join("panes.pids")
}

/// A snapshot, not a delta — always the full set of pids that should still be
/// running, replacing whatever was recorded before. Called after every spawn
/// and every reap. Takes the path explicitly, the same reason [`sweep_at`]
/// does: so a test can drive it without touching the real registry.
pub(crate) fn write_registry(path: &Path, pids: &[u32]) {
    let Some(parent) = path.parent() else { return };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let text = pids.iter().map(u32::to_string).collect::<Vec<_>>().join("\n");
    // Sibling temp file + rename, same as `placement::spawn::seed_config_dir`: a
    // half-written registry must never be read back as "these pids are live"
    // when it is actually truncated.
    let tmp = path.with_extension("pids.tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

/// Kill whatever the last session left running. Best-effort and deliberately
/// silent — startup has nowhere to report to either, and a pid that turns out
/// to already be gone, or to no longer be a pane, is not a failure.
pub(crate) fn sweep() {
    sweep_at(&registry_path());
}

fn sweep_at(path: &Path) {
    sweep_at_named(path, &confirmable_comm_suffixes());
}

/// Every `comm` suffix that could name a live pane — checkpoint 12's
/// [`OrphanNames::comm_suffixes`](crate::placement::harness::OrphanNames::comm_suffixes),
/// collected across **every registered harness** (WP-25, C11).
///
/// The union, and not a per-pid lookup, because the registry is a list of pids
/// and nothing else. That is deliberate: it is written by the same process that
/// spawned them, at a point where the pane is still an object in memory, and it
/// has to be readable by a *later* launch that knows nothing about the fleet it
/// is cleaning up after. Recording which harness each pid ran would make the
/// registry a small database whose schema has to survive a crash mid-write,
/// which is a strictly worse trade than confirming against a slightly wider set
/// of names — the widening cannot reach the operator's own processes, because
/// [`reap_if_named`] never signals a pid that was not recorded here in the first
/// place.
fn confirmable_comm_suffixes() -> Vec<&'static str> {
    crate::placement::harness::registered()
        .iter()
        .flat_map(|harness| harness.spec().orphans.comm_suffixes.iter().copied())
        .collect()
}

/// Split from [`sweep_at`] purely so a test can drive it against names other
/// than the registered harnesses' — proving every matching pid in a batch gets
/// reaped, not just one, and that a harness naming more than one suffix has all
/// of them confirmed, without needing a stand-in that both matches `claude` and
/// survives macOS code-signing (a copy of a signed system binary, renamed, loses
/// its signature and behaves unpredictably).
#[cfg(unix)]
fn sweep_at_named(path: &Path, expected_comm_suffixes: &[&str]) {
    if let Ok(text) = std::fs::read_to_string(path) {
        for pid in text.lines().filter_map(|line| line.trim().parse::<i32>().ok()) {
            reap_if_named(pid, expected_comm_suffixes);
        }
    }
    let _ = std::fs::remove_file(path);
}

#[cfg(not(unix))]
fn sweep_at_named(path: &Path, _expected_comm_suffixes: &[&str]) {
    let _ = std::fs::remove_file(path);
}

/// A pid alone is not enough to act on: pids recycle, and a stale one can by
/// now name some unrelated process that just happens to have inherited the
/// number. Confirm it still matches one of `expected_comm_suffixes` before
/// signalling it; anything else — including "already gone" — is left alone.
///
/// **The names are a filter on recorded pids, never a search key.** Every pid
/// reaching here came out of the registry FLEETOR wrote, so a name that matches
/// nothing leaks a crashed pane rather than killing a live one, and the
/// operator's own harness — whose pid was never recorded — is out of reach no
/// matter what the registered harnesses are called.
#[cfg(unix)]
fn reap_if_named(pid: i32, expected_comm_suffixes: &[&str]) {
    if !process_is_named(pid, expected_comm_suffixes) {
        return;
    }
    unsafe { libc::killpg(pid, libc::SIGTERM) };
    std::thread::sleep(std::time::Duration::from_millis(200));
    if process_is_named(pid, expected_comm_suffixes) {
        unsafe { libc::killpg(pid, libc::SIGKILL) };
    }
}

#[cfg(unix)]
fn process_is_named(pid: i32, expected_comm_suffixes: &[&str]) -> bool {
    // Signal 0: delivers nothing, only reports whether the pid is reachable.
    if unsafe { libc::kill(pid, 0) } != 0 {
        return false;
    }
    let Ok(output) =
        std::process::Command::new("ps").args(["-o", "comm=", "-p", &pid.to_string()]).output()
    else {
        return false;
    };
    let comm = String::from_utf8_lossy(&output.stdout);
    let comm = comm.trim();
    expected_comm_suffixes.iter().any(|suffix| comm.ends_with(suffix))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_registry(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "fleetor-orphans-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("panes.pids")
    }

    #[test]
    fn the_registry_round_trips_whatever_pids_were_live() {
        let path = temp_registry("roundtrip");
        write_registry(&path, &[111, 222, 333]);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "111\n222\n333");
    }

    #[test]
    fn sweep_removes_the_registry_even_when_nothing_in_it_is_still_alive() {
        let path = temp_registry("stale");
        write_registry(&path, &[999999]); // not a real pid
        sweep_at(&path);
        assert!(!path.exists(), "a swept registry does not linger for the next launch to re-read");
    }

    #[test]
    fn sweep_is_a_no_op_when_no_previous_session_left_a_registry() {
        let path = temp_registry("absent");
        sweep_at(&path); // must not panic just because there is nothing to sweep
        assert!(!path.exists());
    }

    /// Distinct from "no previous session": a registry that *exists* but names
    /// nothing (every pane was already dead when the last session wrote it)
    /// takes the `Ok(text)` branch with an empty string, not the "file missing"
    /// branch above — a real, separate path through the `lines()` iterator.
    #[test]
    fn sweep_removes_a_present_but_empty_registry_without_panicking() {
        let path = temp_registry("empty");
        write_registry(&path, &[]);
        assert!(path.exists(), "the write actually landed a file to sweep");
        sweep_at(&path);
        assert!(!path.exists());
    }

    /// `write_registry` fails closed: if the registry's parent can't be
    /// created — here, because a *file* already occupies where a directory
    /// needs to go — it must give up quietly rather than panic. There is
    /// nowhere to report to at this point (same posture as `sweep`), and a
    /// startup crash over an unwritable state directory would be worse than a
    /// missed pid recording.
    #[test]
    fn write_registry_does_not_panic_when_its_parent_cannot_be_created() {
        let blocked_parent = std::env::temp_dir().join(format!(
            "fleetor-orphans-blocked-parent-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&blocked_parent);
        let _ = std::fs::remove_file(&blocked_parent);
        std::fs::write(&blocked_parent, "occupying the path a directory needs").unwrap();

        let target = blocked_parent.join("panes.pids");
        write_registry(&target, &[111, 222]); // must not panic

        assert!(!target.exists(), "nothing was written once the parent could not be created");
        let _ = std::fs::remove_file(&blocked_parent);
    }

    /// The realistic production case: a crash takes down several live panes at
    /// once, all still matching the sweep's identity check — not just one live
    /// match next to a decoy. A `sweep_at` that stopped after its first
    /// successful reap (e.g. an early `return` mistakenly added to the loop)
    /// would pass every other test here, which only ever put one live matching
    /// pid in a registry at a time, and still leave the second one running.
    #[cfg(unix)]
    #[test]
    fn sweep_reaps_every_live_matching_pid_in_one_pass() {
        use std::os::unix::process::CommandExt;

        let spawn_own_session = || {
            let mut command = std::process::Command::new("sleep");
            command.arg("300");
            unsafe {
                command.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
            command.spawn().unwrap()
        };
        let mut first = spawn_own_session();
        let mut second = spawn_own_session();

        let dir = std::env::temp_dir().join(format!("fleetor-orphans-two-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let registry = dir.join("panes.pids");
        write_registry(&registry, &[first.id(), second.id()]);

        sweep_at_named(&registry, &["sleep"]);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut first_exited = false;
        let mut second_exited = false;
        while std::time::Instant::now() < deadline && !(first_exited && second_exited) {
            first_exited = first_exited || matches!(first.try_wait(), Ok(Some(_)));
            second_exited = second_exited || matches!(second.try_wait(), Ok(Some(_)));
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(first_exited, "the first live matching pid was left running");
        assert!(second_exited, "the second live matching pid was left running");

        let _ = first.kill();
        let _ = first.wait();
        let _ = second.kill();
        let _ = second.wait();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The scenario this module exists for: fleetor-shell is killed before its
    /// close handlers can run, and something must still be able to reap the
    /// pane it left behind. Goes through [`reap_if_named`] rather than
    /// `sweep_at` directly, against an *unmodified*
    /// `/bin/sleep` — a copy renamed to `claude` loses its code signature on
    /// macOS and the OS's own signing checks make its behavior unpredictable,
    /// which is a fact about copying signed binaries, not about this logic.
    /// `expected_comm_suffix` exists precisely so the mechanism is testable
    /// without needing a file that both matches "claude" and stays validly
    /// executable.
    #[cfg(unix)]
    #[test]
    fn reap_kills_a_live_process_once_its_identity_matches() {
        use std::os::unix::process::CommandExt;

        let mut command = std::process::Command::new("/bin/sleep");
        command.arg("300");
        unsafe {
            // Its own session/group, same as a real pane gets from `openpty` —
            // so `killpg` here is scoped to it alone, never the test runner's.
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        let mut child = command.spawn().expect("spawn the stand-in");
        let pid = child.id() as i32;

        reap_if_named(pid, &["sleep"]);

        // `try_wait`, not `kill(pid, 0)`: a killed child is a *zombie* until its
        // parent reaps it, and a zombie's pid is still "reachable" — checking
        // liveness that way here would pass even if `reap_if_named` did
        // nothing at all. The orphan-sweep's real targets don't have this
        // wrinkle: their original parent is already gone, so `launchd` reaps
        // them; only in this test are we the parent ourselves.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut exited = false;
        while std::time::Instant::now() < deadline {
            if matches!(child.try_wait(), Ok(Some(_))) {
                exited = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(exited, "reap_if_named left the process running");

        let _ = child.kill();
        let _ = child.wait();
    }

    /// `sweep_at` must not stop at the first line: a registry naming several
    /// panes has to have every one of them checked, not just whichever
    /// happened to come first. The dead pid is placed *before* the live one so
    /// a loop that bailed out after its first "nothing to do here" would never
    /// even reach the second line.
    #[cfg(unix)]
    #[test]
    fn sweep_processes_every_pid_in_a_mixed_registry_not_just_the_first() {
        use std::os::unix::process::CommandExt;

        let mut command = std::process::Command::new("sleep");
        command.arg("300");
        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        let mut live_non_claude = command.spawn().unwrap();
        let live_pid = live_non_claude.id();

        let dir = std::env::temp_dir().join(format!("fleetor-orphans-mixed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let registry = dir.join("panes.pids");
        write_registry(&registry, &[999999, live_pid]);

        sweep_at(&registry);

        assert!(!registry.exists(), "the registry is cleared regardless of what was in it");
        assert!(
            matches!(live_non_claude.try_wait(), Ok(None)),
            "sweep touched a live process it was never told to name, reaching past the dead first entry"
        );

        let _ = live_non_claude.kill();
        let _ = live_non_claude.wait();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **The property C29 recorded as unasserted, now that the sweep reads the
    /// spec: the sweep does not search by process name.**
    ///
    /// Two live processes, identical in every way the sweep can observe — same
    /// program, same `comm`, both matching the suffix it is confirming against.
    /// One pid is in the registry FLEETOR wrote and one is not, and that alone
    /// decides which of them is signalled. This is why the operator's own
    /// harness is never at risk: it is not a name the sweep declines to match,
    /// it is a pid the sweep never had. Cleanup more dangerous than the leak is
    /// the failure worth guarding, and a sweeper that enumerated processes by
    /// name — the plausible "improvement" that would make a crashed pane whose
    /// pid went unrecorded reapable — kills the survivor here.
    ///
    /// The recorded process is in the same batch deliberately: without it a
    /// sweep that did nothing at all would pass.
    #[cfg(unix)]
    #[test]
    fn a_matching_process_the_registry_never_named_is_left_alone() {
        use std::os::unix::process::CommandExt;

        let spawn_own_session = || {
            let mut command = std::process::Command::new("sleep");
            command.arg("300");
            unsafe {
                command.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
            command.spawn().unwrap()
        };
        let mut recorded = spawn_own_session();
        let mut never_recorded = spawn_own_session();

        let dir =
            std::env::temp_dir().join(format!("fleetor-orphans-unrecorded-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let registry = dir.join("panes.pids");
        write_registry(&registry, &[recorded.id()]);

        sweep_at_named(&registry, &["sleep"]);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut recorded_exited = false;
        while std::time::Instant::now() < deadline && !recorded_exited {
            recorded_exited = matches!(recorded.try_wait(), Ok(Some(_)));
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(recorded_exited, "the recorded pid was not reaped, so this test proves nothing");
        assert!(
            matches!(never_recorded.try_wait(), Ok(None)),
            "the sweep signalled a process it matched by name rather than by a pid FLEETOR \
             recorded — this is the operator's own harness being killed"
        );

        let _ = recorded.kill();
        let _ = recorded.wait();
        let _ = never_recorded.kill();
        let _ = never_recorded.wait();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Checkpoint 12 answers with a *list* of suffixes — more than one wherever a
    /// harness runs under an interpreter and the bare interpreter name must never
    /// match alone. Every one of them has to be able to confirm a pid, not just
    /// whichever the registry's harness happens to name first: a sweep that
    /// checked only the first would leak every pane of every harness after it.
    #[cfg(unix)]
    #[test]
    fn any_of_a_harnesss_comm_suffixes_can_confirm_a_pid_not_only_the_first() {
        use std::os::unix::process::CommandExt;

        let mut command = std::process::Command::new("sleep");
        command.arg("300");
        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        let mut child = command.spawn().unwrap();

        let dir =
            std::env::temp_dir().join(format!("fleetor-orphans-suffixes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let registry = dir.join("panes.pids");
        write_registry(&registry, &[child.id()]);

        sweep_at_named(&registry, &["a-name-that-matches-nothing", "sleep"]);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut exited = false;
        while std::time::Instant::now() < deadline && !exited {
            exited = matches!(child.try_wait(), Ok(Some(_)));
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(exited, "only the first comm suffix was ever confirmed against");

        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The other half of the identity check: a live, unrelated process whose
    /// pid a stale registry happens to still name must be left alone.
    #[cfg(unix)]
    #[test]
    fn sweep_leaves_a_live_process_alone_when_it_is_not_claude() {
        use std::os::unix::process::CommandExt;

        let mut command = std::process::Command::new("sleep");
        command.arg("300");
        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        let mut child = command.spawn().unwrap();
        let pid = child.id() as i32;

        let dir = std::env::temp_dir().join(format!("fleetor-orphans-notclaude-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let registry = dir.join("panes.pids");
        write_registry(&registry, &[pid as u32]);

        sweep_at(&registry);

        assert_eq!(unsafe { libc::kill(pid, 0) }, 0, "swept a live process that was never a claude pane");

        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
