//! What the rail offers, and where the app opens.
//!
//! Source-reading tripwires: the frontend has no test runner, and this suite
//! deliberately does not add one. They read declarations, not behaviour.

use std::path::{Path, PathBuf};

const RAIL: &str = "ui/src/components/Sidebar.tsx";
/// The stage, where each view's content is mounted. Read by one test (WP-20),
/// which pins §7 rule 5 for the Critic's terminal the way `tests/evaluator.rs`
/// pins it for the evaluator's.
const STAGE: &str = "ui/src/App.tsx";
/// The Critic's interview gate, as the UI holds it (WP-21 stage A). Read by two
/// tests below: one for the control the operator presses, one for the hook's
/// shape, which is `useDevMode`'s on purpose.
const INTERVIEW: &str = "ui/src/ui/useCriticInterview.ts";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("src-tauri has a parent").to_path_buf()
}

fn read(rel: &str) -> String {
    let file = repo_root().join(rel);
    std::fs::read_to_string(&file)
        .unwrap_or_else(|e| panic!("{} must exist to be checked: {e}", file.display()))
}

/// Every double-quoted literal in `text`, in order. The two declarations this
/// file reads contain nothing but view names between their delimiters, so a
/// quote scan is the whole parse — and a declaration that grew something else
/// inside it would fail loudly here rather than silently drop a name.
fn quoted(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('"') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find('"') else { break };
        out.push(rest[..close].to_string());
        rest = &rest[close + 1..];
    }
    out
}

/// The section of `text` between `after` and the next `until`, panicking with
/// the declaration's name if the shape it depends on is gone — a renamed
/// declaration must fail this test rather than quietly match nothing and pass.
fn between(text: &str, after: &str, until: char, what: &str) -> String {
    let tail = text
        .split(after)
        .nth(1)
        .unwrap_or_else(|| panic!("{what} no longer starts with `{after}` — this test reads it"));
    tail.split(until)
        .next()
        .unwrap_or_else(|| panic!("{what} has no closing `{until}`"))
        .to_string()
}

const REVIEW: &str = "ui/src/components/ReviewView.tsx";

fn critic_stage_block() -> String {
    let review = read(REVIEW);
    review.split("<div className=\"critic-view\">")
        .nth(1)
        .and_then(|rest| rest.split("<div className=\"evaluator-view\">").next())
        .expect("ReviewView.tsx has a critic-view section followed by evaluator-view")
        .to_string()
}

/// The views the rail declares: the `View` union in `Sidebar.tsx`.
fn rail_views() -> Vec<String> {
    quoted(&between(&read(RAIL), "export type View =", ';', "the rail's View union"))
}

/// **The app opens on Home**, whatever view it was closed on: no fleet is running
/// at launch, so a restored Terminals or Feed would open on nothing.
#[test]
fn the_app_opens_on_home() {
    assert!(
        read(STAGE).contains(r#"useState<View>("home")"#),
        "{STAGE} no longer starts on Home",
    );
    assert!(
        !repo_root().join("ui/src/ui/usePersistedNav.ts").exists(),
        "the view is being restored from storage again",
    );
}

/// The consolidated views (home, feed, review) and History are in the rail.
#[test]
fn the_rail_names_the_consolidated_views() {
    let rail = rail_views();
    assert!(rail.len() >= 5, "the rail's View union parsed as {rail:?} — that cannot be right");
    for view in ["home", "feed", "review", "history"] {
        assert!(rail.iter().any(|v| v == view), "{view} must be in the rail: {rail:?}");
    }
}

/// The old views (messages, activity, critic, evaluator) are gone from the rail.
#[test]
fn legacy_views_are_not_in_the_rail() {
    let rail = rail_views();
    for view in ["messages", "activity", "critic", "evaluator"] {
        assert!(!rail.iter().any(|v| v == view), "{view} should not be in the rail: {rail:?}");
    }
}

/// **The Critic's terminal stays mounted, and it has the Start control the
/// evaluator deliberately does not** (WP-20, D-076).
///
/// The first half is §7 rule 5: unmounting an xterm destroys its buffer and there
/// is no screen replay behind a pty (L7), so the view holds its terminal the way
/// every other view holds its content — always in the tree, toggled with
/// `.is-hidden`.
///
/// The second half is the difference between these two panes said as an
/// assertion. The evaluator's sequencing *is* the evidence, so its view offers
/// nothing to press and `tests/evaluator.rs` fails if a button appears. The
/// Critic answers an ordinary question, so its view offers exactly one control
/// and this test fails if that disappears.
#[test]
fn the_critics_terminal_is_never_conditionally_rendered_and_it_has_a_start_control() {
    let block = critic_stage_block();

    assert!(block.contains("<TerminalPane"), "the Critic's view holds a terminal");
    assert!(
        block.contains("pane={CRITIC}"),
        "and it is the Critic's pane, not something that merely looks like one",
    );
    // Three hides, and every one of them is CSS — the view's own, plus the two
    // children that swap. Nothing here leaves the tree, which is §7 rule 5 stated
    // as a count rather than as a hope.
    assert_eq!(
        block.matches("is-hidden").count(),
        3,
        "the Critic's view hides three things with `.is-hidden` and unmounts none: itself \
         when another view is selected, its pre-start prose once it is running, and its \
         terminal until then:\n{block}",
    );
    for tell in ["&&", "? <", "?.("] {
        assert!(
            !block.contains(tell),
            "`{tell}` in the Critic view's markup: §7 rule 5 says a terminal is hidden with \
             `.is-hidden`, never unmounted. A pty that is still alive behind an xterm that \
             was torn down comes back blank, and nothing on this side can repaint it:\n{block}",
        );
    }

    // The one control, and the sentence that says what it will and will not do.
    assert!(block.contains("<button"), "the operator starts the Critic when they want it");
    assert!(
        block.contains("never says whether the work was any good"),
        "the view states the remit before the operator spends anything on it:\n{block}",
    );
}

/// **The interview gate is an operator control, and it prints what it costs**
/// (WP-21 stage A).
///
/// WP-21's semantic criteria include: *"the operator can tell, before pressing
/// the control, that it will spend the fleet's turns."* A `title` cannot satisfy
/// that — it is invisible on the way to the button and does not exist for a
/// keyboard press — and the arc already names the tooltip-plus-lede shape of
/// ticket 08 as the weak part of that ticket. So this test reads the *printed*
/// sentence, out of the element that renders it, and fails if the cost moves
/// back into an attribute.
///
/// It also pins the two halves the control cannot lose: both labels, so the
/// verb changes with the state, and the state said in words beside them,
/// because "Open interview" alone is a verb or an adjective depending on who is
/// reading it.
#[test]
fn the_critics_interview_gate_is_an_operator_control_that_prints_its_cost() {
    let block = critic_stage_block();

    // The control itself.
    assert!(
        block.contains("critic-view__interview-toggle"),
        "the Critic view has the interview control — the operator opening it is the whole \
         feature, not a convenience:\n{block}",
    );
    for label in ["Open interview", "Close interview"] {
        assert!(
            block.contains(label),
            "the control carries `{label}`: the verb has to change with the state, or the \
         operator cannot tell from the button which way pressing it goes:\n{block}",
        );
    }

    // …and the state, said outright rather than inferred from the verb.
    assert!(
        block.contains("critic-view__gate"),
        "the gate's state is rendered in words next to the control:\n{block}",
    );
    for state in ["\"open\"", "\"closed\""] {
        assert!(block.contains(state), "the state readout names {state}:\n{block}");
    }

    // **The cost, printed.** Read out of the paragraph that renders it, so a
    // future session that demotes it to a `title=` fails here.
    let cost = block
        .split("critic-view__cost\">")
        .nth(1)
        .and_then(|rest| rest.split("</p>").next())
        .expect(
            "the Critic view renders a `.critic-view__cost` paragraph — WP-21 requires the \
             operator to know the cost *before* pressing, which a tooltip cannot deliver",
        );
    assert!(
        cost.contains("spends the fleet"),
        "the printed cost line says that opening the interview spends the fleet's turns. \
         This is a performance criterion of WP-21, not a nicety: an interview costs `orch` \
         or a worker a turn it would have spent on the run:\n{cost}",
    );

    // And it is never hidden. The operator meets the sentence whether or not the
    // Critic has been started, because the decision is taken before either.
    let gate_row = block
        .split("critic-view__interview\">")
        .nth(1)
        .and_then(|rest| rest.split("critic-view__waiting").next())
        .expect("the gate row sits above the two states it does not belong to");
    assert!(
        !gate_row.contains("is-hidden"),
        "the interview gate is never hidden: the cost sentence is what the operator reads on \
         the way to the button:\n{gate_row}",
    );

    // Disabled while there is nothing to interview. `=== null` is doing that
    // work: the hook holds `null` with no run *and* while the backend has not
    // answered, and both are states in which pressing would be a guess.
    assert!(
        block.contains("disabled={interview.open === null}"),
        "the control is disabled until the gate's real state is known — with no fleet \
         running there is nothing to interview:\n{block}",
    );
}

/// **The gate is `useDevMode`'s shape, not a second one** (WP-21 stage A).
///
/// The state lives on the Rust side because it decides whether a `fleet send`
/// from inside the Critic *resolves*; the hook is a view of it. Three states,
/// with `null` meaning not-yet-answered, and — the part worth a tripwire — **no
/// optimistic flip**: the switch moves when the write lands. A control showing
/// an interview that did not actually open is a control lying about what the
/// fleet's turns are being spent on.
#[test]
fn the_interview_hook_has_three_states_and_never_flips_optimistically() {
    let hook = read(INTERVIEW);

    assert!(
        hook.contains("open: boolean | null"),
        "three states, and the third is `null` — not-yet-answered. Rendering \"closed\" while \
         the answer is in flight would tell the operator no turns are being spent at a moment \
         when they might be ({INTERVIEW})",
    );
    assert!(
        hook.contains("setOpen(stored)"),
        "the switch moves to what the backend says is *stored*, which is the whole of the \
         no-optimistic-flip rule `useDevMode.ts` states and this hook mirrors ({INTERVIEW})",
    );
    assert!(
        !hook.contains("setOpen(!open)"),
        "…and never to what was merely requested: an optimistic flip here shows an interview \
         that may not have opened ({INTERVIEW})",
    );

    assert!(
        critic_stage_block().contains("interview.toggle"),
        "and the Critic's view is what calls it",
    );
    assert!(
        read(STAGE).contains("useCriticInterview(started)"),
        "the gate is asked about when the fleet is up — with no run there is nothing to \
         interview ({STAGE})",
    );
}
