//! The seeded **testbed** — a real, small project for the fleet to work on when
//! the operator hasn't pointed it at one of their own repos (Phase 2, D-030).
//!
//! The seed is **embedded**, not copied from disk. A path ladder (repo-relative in
//! dev, a Tauri resource in a bundle) is exactly the shape of L4: it works in dev
//! and silently resolves to nothing in a bundle, leaving the fleet with an empty
//! directory to "work on". `include_str!` costs ~9KB in the binary and removes the
//! failure mode entirely.
//!
//! Seeding is **idempotent and non-destructive**: a file that already exists is
//! left alone, because after the first run this directory holds the fleet's actual
//! work.

use std::path::{Path, PathBuf};

/// Every file the testbed is seeded with, as `(relative path, contents)`. Adding a
/// file to `testbed-seed/` means adding a line here — explicit, and the compiler
/// catches a path typo.
const SEED: &[(&str, &str)] = &[
    ("README.md", include_str!("../../testbed-seed/README.md")),
    ("run-tests.sh", include_str!("../../testbed-seed/run-tests.sh")),
    ("sample.log", include_str!("../../testbed-seed/sample.log")),
    ("src/__init__.py", include_str!("../../testbed-seed/src/__init__.py")),
    ("src/cli.py", include_str!("../../testbed-seed/src/cli.py")),
    ("src/parser.py", include_str!("../../testbed-seed/src/parser.py")),
    ("src/render.py", include_str!("../../testbed-seed/src/render.py")),
    ("src/stats.py", include_str!("../../testbed-seed/src/stats.py")),
    ("tests/__init__.py", include_str!("../../testbed-seed/tests/__init__.py")),
    ("tests/test_parser.py", include_str!("../../testbed-seed/tests/test_parser.py")),
    ("tests/test_stats.py", include_str!("../../testbed-seed/tests/test_stats.py")),
];

/// Seed files that must land executable — the testbed's own exit gate is one of
/// them, and a worker asked to "run the tests" hits permission denied without it.
const EXECUTABLE: &[&str] = &["run-tests.sh"];

/// Materialize the testbed at `dir` and make sure it is a git repo with at least
/// one commit. Idempotent: safe to call on every bootstrap.
///
/// Git matters beyond version control — Phase 3 gives each worker pane its own
/// `git worktree` under the target, and `git worktree add` needs a commit to
/// branch from.
pub fn ensure(dir: &Path) -> Result<PathBuf, String> {
    materialize(dir)?;
    ensure_git(dir);
    Ok(dir.to_path_buf())
}

/// Write any seed file that isn't already there. Existing files are never
/// overwritten: once the fleet has worked here, the seed is history, not truth.
fn materialize(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("create testbed {dir:?}: {e}"))?;
    for (rel, contents) in SEED {
        let path = dir.join(rel);
        if path.exists() {
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create {parent:?}: {e}"))?;
        }
        std::fs::write(&path, contents).map_err(|e| format!("write {path:?}: {e}"))?;
        if EXECUTABLE.contains(rel) {
            make_executable(&path);
        }
    }
    Ok(())
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_mode(perms.mode() | 0o111);
        let _ = std::fs::set_permissions(path, perms);
    }
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {}

/// `git init` + an initial commit, both only if missing. Best-effort: a machine
/// without git still gets a working project directory, it just isn't versioned —
/// so this warns rather than failing the bootstrap.
fn ensure_git(dir: &Path) {
    if !dir.join(".git").exists() && !git(dir, &["init", "-q"]) {
        eprintln!("fleet: could not `git init` the testbed at {dir:?}");
        return;
    }
    if git(dir, &["rev-parse", "--verify", "-q", "HEAD"]) {
        return; // already has a commit
    }
    // An identity is passed inline so seeding works on a machine with no global
    // git config, where `git commit` would otherwise fail with a bare "who are you".
    let committed = git(dir, &["add", "-A"])
        && git(
            dir,
            &[
                "-c",
                "user.name=FLEETOR",
                "-c",
                "user.email=fleetor@localhost",
                "commit",
                "-q",
                "-m",
                "testbed: initial seed",
            ],
        );
    if !committed {
        eprintln!("fleet: could not make the testbed's initial commit at {dir:?}");
    }
}

fn git(dir: &Path, args: &[&str]) -> bool {
    std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn scratch(name: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("fleetor-testbed-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// The seed is a *real* project, not an empty scaffold — a fleet handed an
    /// empty directory has nothing to coordinate over.
    #[test]
    fn every_seeded_file_carries_content_except_the_package_markers() {
        for (rel, contents) in SEED {
            if rel.ends_with("__init__.py") {
                continue; // package markers are legitimately empty
            }
            assert!(!contents.trim().is_empty(), "{rel} is empty");
        }
        assert!(SEED.iter().any(|(rel, _)| *rel == "src/parser.py"), "the shared record type is the reason to coordinate");
    }

    #[test]
    fn materializes_the_whole_project_under_a_fresh_directory() {
        let dir = scratch("fresh");
        materialize(&dir).unwrap();
        for (rel, _) in SEED {
            assert!(dir.join(rel).exists(), "{rel} was not written");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The one that matters: bootstrap runs on every launch, and by then this
    /// directory holds the fleet's actual work.
    #[test]
    fn seeding_twice_never_clobbers_what_is_already_there() {
        let dir = scratch("idempotent");
        materialize(&dir).unwrap();
        let edited = dir.join("src/parser.py");
        std::fs::write(&edited, "# a worker rewrote this\n").unwrap();

        materialize(&dir).unwrap();

        assert_eq!(std::fs::read_to_string(&edited).unwrap(), "# a worker rewrote this\n");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A worker told to run the testbed's tests must not hit permission denied.
    #[cfg(unix)]
    #[test]
    fn the_test_runner_lands_executable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("exec");
        materialize(&dir).unwrap();
        let mode = std::fs::metadata(dir.join("run-tests.sh")).unwrap().permissions().mode();
        assert_ne!(mode & 0o111, 0, "run-tests.sh is not executable (mode {mode:o})");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
