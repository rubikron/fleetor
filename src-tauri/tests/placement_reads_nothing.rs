//! **Placement's one rule, asserted** (WP-25 phase 3, issue #34; C8, C14, D-075).
//!
//! `placement`'s module header states it in the first paragraph: *nothing in here
//! reads the process*. No home directory, no environment variable, no configuration
//! path resolved mid-call — everything arrives on the [`Layout`], the [`Host`], the
//! target or the `PaneContext`. That is not a style preference. It is the property
//! that makes the module testable at all: the sequence it replaced bottomed out in
//! functions that read the operator's real `$HOME` at call time and took no
//! argument, so it could only ever be run against the operator's own installation.
//!
//! Until this file the rule was prose. #34 is the ticket that made asserting it
//! urgent, because it adds the most tempting violation the module has yet had: a
//! vendor diagnostic that answers login state, account shape, model list and
//! resolved posture in one call. Every one of those is something a bring-up arm
//! might plausibly want, the call takes about 1.4 s, and **it makes a live provider
//! reachability request** — so a spawn path that reached for it would put a network
//! round trip between a click and a pty. It lives on [`Host`] instead, filled in by
//! `Host::discover_for_the_gate` and by nothing else.
//!
//! ## Why it reads source rather than running anything
//!
//! The same move `tests/harness_literals.rs` and `tests/dev_mode.rs` make, and for
//! the same reason: the property worth pinning is *which code may spell a thing*,
//! and a test that runs the code can only observe that the right value arrived,
//! never where it came from. A `place_worker` that read `$HOME` itself would
//! produce a byte-identical command on the machine it was written on — the whole
//! conformance suite stays green and the seam is quietly gone.
//!
//! ## What it permits, deliberately
//!
//! 1. **`Layout::for_operator` and `Host::discover`**, the module's two declared
//!    process reads. They are constructors of the values the rule exists to make
//!    everything else take as arguments; the header names them both.
//! 2. **`Host::discover_for_the_gate`**, the third, added by #34. It is where the
//!    vendor diagnostic *is* run.
//! 3. **Everything below the first `#[cfg(test)]`.** A unit test that reads the
//!    environment is asserting an observable, not relaying one into a placement.
//! 4. **Comment lines**, which have to be able to name their own subject.
//!
//! ## What it does not claim to catch
//!
//! A process read reached through a helper in another module whose name says
//! nothing — `crate::something::current_user()`. The needles here are the spellings
//! the standard library and this crate actually use, and a new indirection is a new
//! row. That is the same bound `harness_literals.rs` states about its own needles,
//! and it is why the behavioural pin in `placement::tests` sits beside this one.

use std::path::{Path, PathBuf};

/// The files that make up the bring-up sequence and the values it takes.
const PLACEMENT: [&str; 4] = [
    "src-tauri/src/placement/mod.rs",
    "src-tauri/src/placement/spawn.rs",
    "src-tauri/src/placement/harness.rs",
    "src-tauri/src/placement/codex.rs",
];

/// How a Rust program reads the process it is running in.
///
/// Each needle is a spelling that appears in this crate, not a guess at what one
/// might look like — `the_needles_still_describe_a_process_read` keeps that true by
/// requiring every one of them to be found *somewhere*, in a place this file
/// excuses, so a needle that stops matching anything fails rather than passing
/// forever.
const PROCESS_READS: [&str; 6] = [
    "std::env::var",
    "std::env::var_os",
    "env::var(",
    "env::var_os(",
    "std::env::temp_dir",
    "std::env::current_dir",
];

/// The functions inside `placement` that are allowed to read the process, each with
/// the sentence saying why.
///
/// **Three constructors, and the three readers one of them is made of.** A seventh
/// row is the rule being deleted one function at a time: the fix for a placing arm
/// that needs a machine fact is a field on [`Host`], which is what the whole value
/// exists for.
///
/// The bottom three live in `spawn.rs` rather than beside the constructor that
/// calls them, which is where they were when [`Host`] was extracted around them.
/// Excusing them by name would be a hole, so
/// [`the_hosts_own_readers_are_reached_from_nowhere_else`] pins the other half:
/// each is named exactly once inside `placement`, in `Host::discover`.
const CONSTRUCTORS: [(&str, &str); 6] = [
    ("pub fn for_operator", "the layout's one production root"),
    ("pub fn discover", "the machine, per spawn (D13)"),
    ("pub fn discover_for_the_gate", "the machine plus each harness's reading (#34, C8)"),
    ("fn pane_program", "`Host::discover`'s stand-in override reader"),
    ("fn fleet_bin_path", "`Host::discover`'s fleet-binary reader"),
    ("fn operator_toolchain", "`Host::discover`'s rustup reader"),
];

/// The three readers [`Host::discover`] is made of, which may be *called* from
/// there and from nowhere else in `placement`.
const HOSTS_OWN_READERS: [&str; 3] = ["pane_program(", "fleet_bin_path(", "operator_toolchain("];

/// The vendor diagnostic, by the two names any call to it must spell.
///
/// `diagnose` is the function; `doctor` is the subcommand it runs. A bring-up arm
/// that reached the probe by either spelling is what this file exists to stop.
const THE_PROBE: [&str; 2] = ["diagnose(", "DOCTOR"];

/// The one place in production that may run the probe.
const THE_GATE: &str = "src-tauri/src/fleet.rs";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("src-tauri has a parent").to_path_buf()
}

/// A file's production half as `(line number, text)`: everything above the first
/// `#[cfg(test)]`, with comment lines dropped.
///
/// Lifted deliberately from `tests/harness_literals.rs` rather than shared with it:
/// the two files pin different properties over different file sets, and a helper
/// crate for eight lines would be the seam this arc keeps declining to build.
fn production_lines(file: &Path) -> Vec<(usize, String)> {
    let text = std::fs::read_to_string(file)
        .unwrap_or_else(|e| panic!("{} must exist to be checked: {e}", file.display()));
    text.lines()
        .take_while(|line| line.trim() != "#[cfg(test)]")
        .enumerate()
        .map(|(i, line)| (i + 1, line.to_string()))
        .filter(|(_, line)| {
            let t = line.trim_start();
            !t.starts_with("//") && !t.starts_with("/*") && !t.starts_with('*')
        })
        .collect()
}

/// Which function a line is inside, tracked by the last `fn` header seen at or
/// above it.
///
/// Crude on purpose: it needs to attribute a process read to the item that contains
/// it, not to parse Rust. A nested closure or an `impl` block does not change the
/// answer, because what matters is whether the enclosing named function is one of
/// the three [`CONSTRUCTORS`].
fn enclosing_functions(lines: &[(usize, String)]) -> Vec<(usize, String, String)> {
    let mut current = String::from("<file scope>");
    lines
        .iter()
        .map(|(n, line)| {
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed
                .strip_prefix("pub fn ")
                .or_else(|| trimmed.strip_prefix("fn "))
                .or_else(|| trimmed.strip_prefix("pub(crate) fn "))
                .or_else(|| trimmed.strip_prefix("pub(super) fn "))
            {
                let name = rest.split(['(', '<', ' ']).next().unwrap_or(rest);
                let visibility = if trimmed.starts_with("pub fn ") { "pub fn " } else { "fn " };
                current = format!("{visibility}{name}");
            }
            (*n, current.clone(), line.clone())
        })
        .collect()
}

/// **Nothing in `placement` reads the process, outside the three constructors that
/// exist to.**
///
/// The rule in the module header, as a check. Read that header before adding a row
/// to [`CONSTRUCTORS`]: a placing arm that reads the environment is one that can
/// only ever be run against the operator's own machine, and every test that reaches
/// it through `place` against a scratch layout stops meaning what it says.
#[test]
fn no_placement_function_but_the_three_constructors_reads_the_process() {
    let root = repo_root();
    let mut hits: Vec<String> = Vec::new();

    for rel in PLACEMENT {
        let file = root.join(rel);
        for (n, function, line) in enclosing_functions(&production_lines(&file)) {
            let excused = CONSTRUCTORS.iter().any(|(name, _)| function == *name);
            if excused {
                continue;
            }
            if PROCESS_READS.iter().any(|needle| line.contains(needle)) {
                hits.push(format!("  {rel}:{n} [{function}] {}", line.trim()));
            }
        }
    }

    assert!(
        hits.is_empty(),
        "something in `placement` reads the process. Every machine fact reaches this module \
         on `Host` and every path on `Layout` — that is the whole reason a placement can be \
         run against a scratch directory, and a read here is testable only against the \
         operator's own installation. Put the fact on `Host` and take it as an argument.\n{}",
        hits.join("\n"),
    );
}

/// **`Host::discover`'s three readers are reached from nowhere else in
/// `placement`** — the other half of excusing them above.
///
/// They read the environment, they live in `spawn.rs` beside the command builders
/// rather than beside the constructor they belong to, and `spawn.rs`'s own header
/// already says each is *"called from `Host::discover` and nowhere else"*. That
/// sentence is what makes excusing them safe, so it is checked rather than trusted:
/// a bring-up arm that called `operator_toolchain()` directly would be reading the
/// process through a name this file's needles do not spell.
#[test]
fn the_hosts_own_readers_are_reached_from_nowhere_else() {
    let root = repo_root();
    let mut hits: Vec<String> = Vec::new();
    for rel in PLACEMENT {
        let file = root.join(rel);
        for (n, function, line) in enclosing_functions(&production_lines(&file)) {
            // The definitions themselves, and the one constructor that calls them.
            if function == "pub fn discover" || CONSTRUCTORS[3..].iter().any(|(f, _)| function == *f)
            {
                continue;
            }
            if HOSTS_OWN_READERS.iter().any(|reader| line.contains(reader)) {
                hits.push(format!("  {rel}:{n} [{function}] {}", line.trim()));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "a placement function reaches one of `Host::discover`'s environment readers directly. \
         Those three are excused from the process-read rule *because* they have one caller; \
         a second caller is the rule gone, and the value the caller wants is already a field \
         on the `Host` it was handed.\n{}",
        hits.join("\n"),
    );
}

/// **The vendor diagnostic is on the gate and nowhere the spawn path reaches**
/// (#34; C8).
///
/// Two halves, because either alone passes on a mistake. The first says the probe
/// is not called from a placing arm; the second says the *only* production caller
/// outside `placement/codex.rs` is the gate — so a later ticket cannot reach it
/// from `spawn_pane` by adding a caller somewhere this file does not look.
#[test]
fn the_vendor_diagnostic_runs_at_the_gate_and_never_on_the_spawn_path() {
    let root = repo_root();

    // Half one: inside `placement`, only `Host::discover_for_the_gate` names it —
    // and `codex.rs`, which is where it is defined.
    let mod_rs = root.join("src-tauri/src/placement/mod.rs");
    let mut inside: Vec<String> = Vec::new();
    for (n, function, line) in enclosing_functions(&production_lines(&mod_rs)) {
        if function == "pub fn discover_for_the_gate" {
            continue;
        }
        for needle in THE_PROBE {
            if line.contains(needle) {
                inside.push(format!("  placement/mod.rs:{n} [{function}] {}", line.trim()));
            }
        }
    }
    assert!(
        inside.is_empty(),
        "a placement function reaches the vendor diagnostic. It makes a live provider \
         reachability request measured at about 1.4 s (C8), so a spawn path that took it \
         would put a network round trip between a click and a pty — a different product. \
         It belongs on `Host::discover_for_the_gate`.\n{}",
        inside.join("\n"),
    );

    // And `discover_for_the_gate` really is where it happens, so half one cannot
    // pass by the probe having been deleted.
    let source = std::fs::read_to_string(&mod_rs).expect("placement/mod.rs");
    let gate_body = source
        .split("pub fn discover_for_the_gate")
        .nth(1)
        .expect("the gate's constructor exists")
        .split("\n    }")
        .next()
        .expect("it has a body");
    assert!(
        gate_body.contains("codex::diagnose"),
        "the gate's constructor stopped asking any harness anything, so the check above \
         now passes vacuously:\n{gate_body}",
    );

    // Half two: outside `placement`, the one production caller of the gate's
    // constructor is the gate itself.
    let mut callers: Vec<String> = Vec::new();
    for entry in walk(&root.join("src-tauri/src")) {
        let rel = entry.strip_prefix(&root).unwrap_or(&entry).to_string_lossy().into_owned();
        if rel.starts_with("src-tauri/src/placement/") {
            continue;
        }
        for (n, line) in production_lines(&entry) {
            if line.contains("discover_for_the_gate") && rel != THE_GATE {
                callers.push(format!("  {rel}:{n} {}", line.trim()));
            }
        }
    }
    assert!(
        callers.is_empty(),
        "the gate's discovery is called from somewhere that is not the gate. There is one \
         screen where a live provider request and a 1.4 s wait are what the operator is \
         already there for, and this is not it.\n{}",
        callers.join("\n"),
    );

    let gate = std::fs::read_to_string(root.join(THE_GATE)).expect("the gate");
    assert!(
        gate.contains("discover_for_the_gate") && gate.contains("harness_notices"),
        "the gate stopped asking, or stopped telling the operator what it heard (#34)",
    );
}

/// **The needles still describe a process read**, so this file cannot rot into a
/// list of strings that match nothing.
///
/// The sibling of `harness_literals.rs`'s
/// `the_needles_are_still_the_specs_own_answers` and `dev_mode.rs`'s equivalent,
/// with the same worry: a grep whose needles have quietly stopped describing their
/// subject passes forever. Each needle must be found *somewhere* in the crate,
/// which is a weaker claim than "in the excused constructors" and a true one — some
/// spellings live in `fleet.rs` and in the test halves this file does not read.
#[test]
fn the_needles_still_describe_a_process_read() {
    let root = repo_root();
    let mut corpus = String::new();
    for entry in walk(&root.join("src-tauri/src")) {
        corpus.push_str(&std::fs::read_to_string(&entry).unwrap_or_default());
    }
    for needle in PROCESS_READS {
        assert!(
            corpus.contains(needle),
            "`{needle}` no longer appears anywhere in this crate, so it is a needle that \
             matches nothing and this file is weaker than it reads. Update it to the \
             spelling that replaced it — never drop the row.",
        );
    }
    for (name, why) in CONSTRUCTORS {
        assert!(
            corpus.contains(name),
            "`{name}` ({why}) is excused from a rule it no longer takes part in",
        );
    }
}

/// Every `.rs` file under a directory.
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(at) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&at) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}
