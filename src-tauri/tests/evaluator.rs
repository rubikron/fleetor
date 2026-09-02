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
//! 4. **§7 rule 5** — the evaluator's terminal stays mounted, hidden with
//!    `.is-hidden` when its view is not selected. D-073 moved this from a second
//!    OS window to a view in the rail; the rule did not move, only the level it
//!    is enforced at.

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

// --- 4. §7 rule 5, at the view level -------------------------------------------
//
// **These two tests changed their letter in D-073 and are stricter for it.**
// They used to pin the second window's mechanism: that `EvaluatorWindow.tsx`
// rendered its terminal unconditionally, and that closing that window was
// intercepted and turned into a hide. Both described machinery this ticket
// deleted, so both now assert the property those mechanisms were bought for —
// the terminal is never unmounted, and closing the app reaps exactly the fleet —
// at the place it now lives, plus the new claim that the machinery is *gone*.
// Neither property lost an assertion; the second gained four.

/// **The evaluator's terminal stays mounted.** Unmounting an xterm destroys its
/// buffer and there is no screen replay behind a pty (L7), so the evaluator's
/// view holds its terminal the way every other view holds its content: always in
/// the tree, toggled with `.is-hidden`.
///
/// Read on the evaluator's stage-view block alone. `App.tsx` is full of
/// legitimate `&&` elsewhere — the start gate genuinely unmounts, because it is
/// stateless chrome and has no buffer to lose.
#[test]
fn the_evaluators_terminal_is_never_conditionally_rendered() {
    let app = read("ui/src/App.tsx");
    // The evaluator's own stage-view, up to where the next one begins.
    let block = app
        .split("stage-view ${view === \"evaluator\"")
        .nth(1)
        .and_then(|rest| rest.split("stage-view ${view === \"settings\"").next())
        .expect("App.tsx has an evaluator stage-view, followed by the settings one");

    assert!(block.contains("<TerminalPane"), "the evaluator view holds a terminal");
    assert!(
        block.contains("pane={EVALUATOR}"),
        "and it is the evaluator's pane, not something that merely looks like one",
    );
    // Three hides, and every one of them is CSS. The view's own — the tail of
    // the `stage-view` class this block was split on — plus the two children
    // that swap: the pre-handoff prose and the terminal. Nothing in here leaves
    // the tree, which is §7 rule 5 stated as a count rather than as a hope.
    assert_eq!(
        block.matches("is-hidden").count(),
        3,
        "the evaluator view hides three things with `.is-hidden` and unmounts none: itself \
         when another view is selected, its pre-handoff prose once the evaluator wakes, and \
         its terminal until then. A different count means one of them started being \
         conditionally rendered instead:\n{block}",
    );
    for tell in ["&&", "? <", "?.("] {
        assert!(
            !block.contains(tell),
            "`{tell}` in the evaluator view's markup: §7 rule 5 says a terminal is hidden with \
             `.is-hidden`, never unmounted. A pty that is still alive behind an xterm that was \
             torn down comes back blank, and nothing on this side can repaint it:\n{block}",
        );
    }

    // **No start control** (WP-20 story 25). The evaluator wakes on a handoff and
    // the absence of a button is the design showing through, so a button here is
    // a regression even though nothing would fail at runtime.
    assert!(
        !block.contains("<button") && !block.contains("onClick"),
        "the evaluator view offers no control to start the evaluator — the sequencing is \
         deliberate, and a run that could be started ahead of its handoff would not be \
         evidence of anything:\n{block}",
    );

    // **The `review` label is retired** (D7). It collides with peer review, which
    // is the one convention about review the briefs actually teach.
    assert!(
        !block.contains("label=\"review\""),
        "the terminal's stale `review` label collides with the peer review a worker's brief \
         teaches (D7); it is retired:\n{block}",
    );
}

/// **Closing the application is unambiguous, because there is one window.**
///
/// The close handler used to have to ask *which* window closed before it could
/// act — a question it only had because WP-15 added a second one, and which it
/// answered wrongly for a while (any `CloseRequested` tore down all six ptys).
/// With the evaluator in a view, the only close that can arrive is the
/// application's, so the branch is gone and there is nothing left to get wrong.
#[test]
fn closing_the_application_reaps_the_fleet_and_nothing_asks_which_window() {
    let lib = read("src-tauri/src/lib.rs");
    let handler = lib
        .split(".on_window_event(")
        .nth(1)
        .and_then(|rest| rest.split("teardown_fleet(window)").next())
        .expect("the close handler still tears the fleet down");

    for gone in ["window.label()", "prevent_close", "window.hide()", "WINDOW_LABEL"] {
        assert!(
            !handler.contains(gone),
            "`{gone}` is back in the close handler. One window means one meaning for a close \
             (D-073); a handler that branches on a label is a handler that can reap more than \
             it meant to, which is the bug WP-15 shipped:\n{handler}",
        );
    }

    // The second window is gone from the source, not merely unused. `building.md`
    // §6: delete rather than deprecate — a builder left behind is a builder the
    // obvious next move re-points at something live.
    let fleet = read("src-tauri/src/fleet.rs");
    for gone in ["WebviewWindowBuilder", "get_webview_window", "index.html?window="] {
        assert!(
            !fleet.contains(gone),
            "`{gone}` in fleet.rs: the evaluator is a view, and nothing there creates a window",
        );
    }
    assert!(
        !read("src-tauri/src/evaluator.rs").contains("WINDOW_LABEL"),
        "the window label is deleted, not kept for a window that no longer exists",
    );
    let main_tsx = read("ui/src/main.tsx");
    for gone in ["URLSearchParams", "EvaluatorWindow"] {
        assert!(
            !main_tsx.contains(gone),
            "`{gone}` in main.tsx: one window means one root, mounted with no branch on which \
             window this is (D-073). The second root was the only true half of WP-15's \
             argument, and it stopped being needed when the window did",
        );
    }

    // Still not declared in tauri.conf.json — a config-declared window exists at
    // every launch, which "outside dev mode the evaluator does not exist"
    // forbids. It was true when the window was built at wake and it is true now
    // that there is no window at all.
    assert!(
        !read("src-tauri/tauri.conf.json").contains("evaluator"),
        "no evaluator window is declared in the config",
    );
    // And the per-window capability scoping is gone with the window it scoped.
    // Its absence is the assertion: a stale `evaluator` label here would grant
    // `invoke` and `listen` to a window nothing creates.
    let capabilities = read("src-tauri/capabilities/default.json");
    assert!(
        !capabilities.contains("\"evaluator\""),
        "capabilities are scoped by window label; the evaluator's label is scoped to a window \
         that no longer exists",
    );
    assert!(
        capabilities.contains("\"main\""),
        "the one window still needs its capability, or it gets no `invoke` and no `listen` at \
         all, silently",
    );
}

/// The wake is an event to the one webview, and the two spellings of its name
/// have to match — a listener on the wrong name renders nothing and reports no
/// error, which is the failure mode `ui/src/fleet/types.ts` already warns about.
#[test]
fn the_wake_reaches_the_webview_by_a_name_both_sides_spell_the_same() {
    const NAME: &str = "evaluator://wake";
    assert!(
        read("src-tauri/src/fleet.rs").contains(NAME),
        "the Rust side emits the wake on `{NAME}`",
    );
    assert!(
        read("ui/src/fleet/api.ts").contains(NAME),
        "and the webview listens on the same string — a mismatch is silent on both sides",
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
