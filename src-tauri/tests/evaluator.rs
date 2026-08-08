//! The boundaries the evaluator is not allowed to cross (WP-15).
//!
//! `tests/dev_mode.rs` holds the **veil** — what a pane is shown. This file
//! holds the other four, and each is checked by reading the source for the same
//! reason `write_guardrail.rs` and `dev_mode.rs` do: what is worth pinning is
//! that a future session cannot add the coupling without a test noticing.
//!
//! 1. **Tier 1.4** — the wake must not put anything between `fleet send` and a
//!    pty. Checked in both directions, as WP-17 checks its own.
//! 2. **No override path for the evaluator's brief.** The asymmetry that makes
//!    "the fleet cannot influence the grader" structural rather than policed.
//! 3. **The brief is not in this repo**, in any form, including in a test.
//! 4. **§7 rule 5** — the second window's terminal stays mounted, and closing
//!    the window hides it rather than destroying its buffer.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("src-tauri has a parent").to_path_buf()
}

fn read(rel: &str) -> String {
    let file = repo_root().join(rel);
    std::fs::read_to_string(&file)
        .unwrap_or_else(|e| panic!("{} must exist to be checked: {e}", file.display()))
}

/// Lines of `rel` containing any of `needles`, case-insensitively.
fn hits(rel: &str, needles: &[&str]) -> Vec<String> {
    read(rel)
        .lines()
        .enumerate()
        .filter(|(_, line)| {
            let lowered = line.to_lowercase();
            needles.iter().any(|n| lowered.contains(n))
        })
        .map(|(i, line)| format!("  {rel}:{} {}", i + 1, line.trim()))
        .collect()
}

// --- 1. Tier 1.4 ---------------------------------------------------------------

/// **The half that matters: the delivery path cannot see the wake.**
///
/// The list is the files a message passes through, minus the two that own the
/// terminals. `src-tauri/src/pty.rs` is deliberately absent and that is not a
/// gap: the registry owns ptys, the evaluator *has* a pty, and a registry that
/// could not name it could not give it a channel or keep it off the fleet's
/// roster. What matters is that nothing which can *delay, refuse, reorder, drop
/// or alter a message* knows the wake exists — and `pty.rs`'s knowledge is
/// confined to `channel_key` and `roster()`, neither of which `writable` or
/// `write_paste` consults.
#[test]
fn the_delivery_path_cannot_see_the_evaluator() {
    const WORDS: [&str; 3] = ["evaluator", "handoff watch", "retro"];
    let found: Vec<String> = [
        "src-tauri/src/deliver.rs",
        "crates/fleetor-server/src/hub.rs",
        "crates/fleetor-server/src/bus.rs",
        "crates/fleetor-server/src/lib.rs",
        "crates/fleetor-core/src/message.rs",
        "crates/fleetor-core/src/wire.rs",
        "crates/fleetor-core/src/command.rs",
        "crates/fleetor-cli/src/main.rs",
    ]
    .iter()
    .flat_map(|rel| hits(rel, &WORDS))
    .collect();

    assert!(
        found.is_empty(),
        "Tier 1.4: the wake reacts to a handoff *after* it is in the log, from a task that \
         sends no AppCommand and that nothing waits on. A file on the path from `fleet send` \
         to a pty that knew about it would be one `if` from a delivery that behaves \
         differently once the mission is declared over — which is the gate D-034 refused \
         twice. Note the CLI needs no arm either: it parses a `PaneId` and the hub answers.\n{}",
        found.join("\n"),
    );
}

/// The other direction, the move `write_guardrail.rs` makes: the evaluator's own
/// module cannot reach the machinery a message travels through. It resolves a
/// mission, renders a brief and lays out a directory; it has no business holding
/// a `Hub`, an `AppCommand` or a pane registry.
#[test]
fn the_evaluator_cannot_reach_the_message_path_either() {
    const FORBIDDEN: [&str; 5] = ["hub", "appcommand", "paneregistry", "deliver::", "opresult"];
    let found = hits("src-tauri/src/evaluator.rs", &FORBIDDEN);
    assert!(
        found.is_empty(),
        "`evaluator.rs` decides whether there is a grader and what it is told. Reaching the \
         routing or delivery machinery from here is how the wake would stop being downstream \
         of the log and start being part of it.\n{}",
        found.join("\n"),
    );
}

/// The wake is a **subscriber**, not a call from the hub. Stated as a property
/// of the one function's shape: it takes the bus and the store, and the hub is
/// not among its arguments.
#[test]
fn the_wake_is_a_bus_subscriber_and_not_something_the_hub_calls() {
    let fleet = read("src-tauri/src/fleet.rs");
    assert!(
        fleet.contains("fn spawn_evaluator_wake(") && fleet.contains("bcast.follow(0)"),
        "the wake must ride the event bus D-020 built for downstream readers",
    );
    let signature = fleet
        .split("fn spawn_evaluator_wake(")
        .nth(1)
        .and_then(|rest| rest.split(')').next())
        .expect("the wake has a signature");
    for forbidden in ["Hub", "AppCommand", "mpsc"] {
        assert!(
            !signature.contains(forbidden),
            "the wake takes no {forbidden} — it is downstream of the log, not wired to the \
             router: {signature}",
        );
    }
}

// --- 2 and 3. the brief ---------------------------------------------------------

/// **The evaluator has no `~/.fleetor/prompts/` override, and every other brief
/// does.** That asymmetry is the whole of "the fleet cannot influence the
/// grader": an override path would put the rubric on disk in a directory every
/// pane can read by absolute path and a worker's auto-approve could write to,
/// which gives back both properties the separate repo exists to buy.
///
/// Checked at the resolver, because that is the one place a fourth template
/// would be added, and in the file that documents the mechanism.
#[test]
fn the_evaluators_brief_has_no_operator_override_path() {
    let resolver = hits("src-tauri/src/prompts.rs", &["evaluator"]);
    assert!(
        resolver.is_empty(),
        "`prompts::resolve` is what turns a file in ~/.fleetor/prompts/ into a pane's brief. \
         The evaluator must not be reachable from it — and `evaluator::OVERRIDE_REFUSED` \
         names the exclusion so it is a thing in the source rather than a thing nobody did.\n{}",
        resolver.join("\n"),
    );

    // And the account of the mechanism says so, so an operator reading
    // prompts/README.md does not go looking for a file that will never load.
    let readme = read("prompts/README.md").to_lowercase();
    assert!(
        readme.contains("evaluator"),
        "prompts/README.md is the full account of the override mechanism (D-042). A brief \
         that is deliberately excluded from it has to be named there, or the exclusion \
         reads as an oversight",
    );
}

/// **This repo's source carries zero evaluator prose**, which is what makes the
/// separate repo worth having at all. The brief arrives through `OUT_DIR` at
/// build time and lands in `target/`, never in the tree.
#[test]
fn no_file_in_this_repo_is_the_evaluators_brief() {
    let root = repo_root();
    for rel in ["prompts", "docs", "crates", "ui/src"] {
        let stray = find_brief(&root.join(rel));
        assert!(
            stray.is_none(),
            "a file named like the evaluator's brief turned up at {:?}. The rubric lives in a \
             separate repo (build.rs); a copy here is one an improve run could edit, and a \
             grader the generation it judges can soften makes 'did the fleet improve' \
             unanswerable",
            stray,
        );
    }
    // The include is by `OUT_DIR`, never by a path inside this tree.
    let build = read("src-tauri/build.rs");
    assert!(build.contains("OUT_DIR"), "the brief is copied into target/, not read from the repo");
    let module = read("src-tauri/src/evaluator.rs");
    assert!(
        module.contains("env!(\"OUT_DIR\")") && !module.contains("include_str!(\"../../prompts"),
        "`evaluator.rs` must include the brief from OUT_DIR and never from prompts/",
    );
}

fn find_brief(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_brief(&path) {
                return Some(found);
            }
        } else if path.file_name().and_then(|n| n.to_str()) == Some("brief.md") {
            return Some(path);
        }
    }
    None
}

// --- 4. §7 rule 5, at the window level -----------------------------------------

/// **The second window's terminal stays mounted.** Unmounting an xterm destroys
/// its buffer and there is no screen replay behind a pty (L7), so the evaluator
/// window renders its one terminal unconditionally — no ternary, no `&&`, no
/// gate — for the life of the window.
#[test]
fn the_evaluator_windows_terminal_is_never_conditionally_rendered() {
    let window = read("ui/src/EvaluatorWindow.tsx");
    let render = window.split("return (").nth(1).expect("the component returns markup");
    assert!(render.contains("<TerminalPane"), "the window is a terminal");
    for tell in ["&&", "? <", "?.(", "isHidden"] {
        assert!(
            !render.contains(tell),
            "`{tell}` in the evaluator window's markup: §7 rule 5 says a terminal is hidden \
             with `.is-hidden`, never unmounted — and this window has nothing to hide it for",
        );
    }
}

/// And the window itself: closing it is intercepted and turned into a hide, so
/// the webview — and the xterm buffer inside it — survives to be shown again.
/// The same rule one level up.
///
/// This also pins the fix for a bug the second window created: the close handler
/// used to tear the whole fleet down for *any* window, which with two windows is
/// a six-pty kill on closing the wrong one.
#[test]
fn closing_the_evaluator_window_hides_it_and_does_not_kill_the_fleet() {
    let lib = read("src-tauri/src/lib.rs");
    let handler = lib
        .split(".on_window_event(")
        .nth(1)
        .and_then(|rest| rest.split("teardown_fleet(window)").next())
        .expect("the close handler still tears the fleet down for the main window");
    assert!(
        handler.contains("window.label() == evaluator::WINDOW_LABEL"),
        "the close handler must branch on which window closed before tearing anything down",
    );
    assert!(
        handler.contains("api.prevent_close()") && handler.contains("window.hide()"),
        "the evaluator window's close is refused and turned into a hide (§7 rule 5)",
    );

    // The window is created at wake, never declared in tauri.conf.json — a
    // config-declared window exists at every launch, which is exactly what
    // "outside dev mode the evaluator does not exist" forbids.
    let conf = read("src-tauri/tauri.conf.json");
    assert!(!conf.contains("evaluator"), "the evaluator window must not be declared in the config");
    // …but its label must be in the capability file, or it silently gets no
    // `invoke` and no `listen` and renders a terminal that can never spawn.
    assert!(
        read("src-tauri/capabilities/default.json").contains("\"evaluator\""),
        "capabilities are scoped by window label; a missing label fails silently",
    );
}

// --- unreachable outside dev mode -----------------------------------------------

/// **The default build has no evaluator at all**, and this is the compile-time
/// half of it: no feature, no brief, no reachable spawn.
///
/// The runtime half — the mode, and the mission — is
/// `evaluator::tests` in the module itself, where the readiness table lives.
#[test]
#[cfg(not(feature = "devmode"))]
fn a_default_build_contains_no_evaluator_brief() {
    assert!(
        fleetor_shell::evaluator::BRIEF.is_none(),
        "a build without --features devmode must carry no evaluator prose whatsoever",
    );
    let manifest = read("src-tauri/Cargo.toml");
    assert!(manifest.contains("default = []"), "devmode must not be a default feature");
}
