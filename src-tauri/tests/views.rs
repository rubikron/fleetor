//! The interface declares its views twice, and the two lists must agree (D-073).
//!
//! The rail in `ui/src/components/Sidebar.tsx` says which views exist. The
//! restore list in `ui/src/ui/usePersistedNav.ts` says which of them may come
//! back on relaunch. A view in the first and not the second is selectable and
//! then silently unreachable: the persisted id fails `isValidView`, falls back
//! to Terminals, and says nothing — the *same* fallback a corrupt value gets,
//! which is precisely why it is invisible. `history` sat in that state from the
//! day it shipped until this test was written.
//!
//! **Why a Rust test for a TypeScript property.** The frontend has no test
//! runner, and this arc deliberately does not add one — introducing one as a
//! side effect of moving a terminal into a tab is how a small package becomes
//! the polluted one. So this is a source-reading tripwire, the move
//! `tests/dev_mode.rs` and `tests/evaluator.rs` already make and for the same
//! stated reason: what is worth pinning is that a future session cannot add a
//! view to one list and forget the other without a test noticing.
//!
//! It reads the *lists*, not the behaviour. It cannot tell you that restoring a
//! view works — only that the two lists name the same things, which is the one
//! way this has actually gone wrong.

use std::path::{Path, PathBuf};

const RAIL: &str = "ui/src/components/Sidebar.tsx";
const RESTORE: &str = "ui/src/ui/usePersistedNav.ts";
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

/// The Critic's own stage-view in `App.tsx`, up to where the next block's
/// comment begins — the *comment*, not the next `stage-view`, because the prose
/// between them discusses `.is-hidden` and would be counted by the tests below.
fn critic_stage_block() -> String {
    let app = read(STAGE);
    app.split("stage-view ${view === \"critic\"")
        .nth(1)
        .and_then(|rest| rest.split("{/* The evaluator").next())
        .expect("App.tsx has a critic stage-view, followed by the evaluator's")
        .to_string()
}

/// The views the rail declares: the `View` union in `Sidebar.tsx`.
fn rail_views() -> Vec<String> {
    quoted(&between(&read(RAIL), "export type View =", ';', "the rail's View union"))
}

/// The views the app will restore: the `VIEWS` array in `usePersistedNav.ts`.
fn restorable_views() -> Vec<String> {
    quoted(&between(&read(RESTORE), "const VIEWS: readonly View[] = [", ']', "the restore list"))
}

/// **The tripwire.** Both lists, compared in both directions, with the missing
/// names in the message — a failure that only said "these differ" would send the
/// next session back to diffing two files by eye.
#[test]
fn the_rail_and_the_restore_list_name_the_same_views() {
    let rail = rail_views();
    let restorable = restorable_views();

    assert!(rail.len() >= 6, "the rail's View union parsed as {rail:?} — that cannot be right");

    let unrestorable: Vec<&String> = rail.iter().filter(|v| !restorable.contains(v)).collect();
    assert!(
        unrestorable.is_empty(),
        "{unrestorable:?} {} in the rail ({RAIL}) and missing from the restore list \
         ({RESTORE}). The operator can select {} and, on the next launch, land on Terminals \
         with nothing said — the same silent fallback a corrupt stored value gets. Add the \
         name to `VIEWS`.\n  rail:     {rail:?}\n  restores: {restorable:?}",
        if unrestorable.len() == 1 { "is" } else { "are" },
        if unrestorable.len() == 1 { "it" } else { "them" },
    );

    let phantom: Vec<&String> = restorable.iter().filter(|v| !rail.contains(v)).collect();
    assert!(
        phantom.is_empty(),
        "{phantom:?} in the restore list ({RESTORE}) and not in the rail ({RAIL}). Restoring a \
         view the rail does not declare puts the app on a stage nothing can navigate away \
         from, which is worse than the omission this test was written for.\n  \
         rail:     {rail:?}\n  restores: {restorable:?}",
    );
}

/// The omission that motivated the tripwire, named. The test above would catch
/// it again, but only as one entry in a diff; this one says which view it was
/// and why it mattered, so the next reader does not have to reconstruct it from
/// a decisions entry.
#[test]
fn history_is_restorable_which_it_was_not_before_d073() {
    assert!(
        restorable_views().iter().any(|v| v == "history"),
        "`history` is the view the restore list omitted from the day it shipped: selecting \
         History and relaunching landed on Terminals, silently. It is also the reason this \
         file exists",
    );
}

/// The evaluator is a view like any other, and this is the half of that claim a
/// source read can make (D-073). The rest — that its terminal survives a view
/// switch — is `tests/evaluator.rs`, and the part no test here can reach is in
/// the operator's by-hand list.
#[test]
fn the_evaluator_is_one_of_the_rails_views() {
    let rail = rail_views();
    assert!(
        rail.iter().any(|v| v == "evaluator"),
        "the evaluator is a view in the rail, not a second window (D-073): {rail:?}",
    );
    assert!(
        restorable_views().iter().any(|v| v == "evaluator"),
        "and it restores like the others — `App.tsx` corrects it when dev mode is off, which \
         it can only do for a view that was restored in the first place",
    );
}

/// **The Critic is a view like every other, and unlike the evaluator's row it is
/// not dev-only** (WP-20, D-076). It is a product feature: the operator has it
/// whatever mode the app is in, so nothing in the rail filters it out.
///
/// The last clause is what a source read can reach. `devOnly` is a field on the
/// row, and the rail's `rows` filter is the one thing that consults it, so a
/// Critic row that grew the flag would be a Critic that vanished with the mode —
/// exactly the failure this whole file exists for, one level up.
#[test]
fn the_critic_is_a_rail_view_that_is_not_dev_only() {
    let rail = rail_views();
    assert!(rail.iter().any(|v| v == "critic"), "the Critic is a view in the rail: {rail:?}");
    assert!(
        restorable_views().iter().any(|v| v == "critic"),
        "and it restores like the others — nothing corrects it away afterwards, because \
         nothing gates it",
    );

    // The row itself, read out of the rail's own declaration.
    let source = read(RAIL);
    let row = between(&source, "view: \"critic\"", '}', "the Critic's rail row");
    assert!(
        !row.contains("devOnly"),
        "the Critic must be in the rail whatever the mode is — it is a product feature, not \
         an instrument of an experiment:{row}",
    );

    // **And the rail says which question each of the two run-reading views
    // answers.** In dev mode they sit next to each other, and two rows labelled
    // only "Critic" and "Evaluator" would leave the operator to guess which one
    // holds the answer key. Both carry a `hint`, and it reaches the row's `title`.
    let evaluator_row = between(&source, "view: \"evaluator\"", '}', "the evaluator's rail row");
    for (which, row) in [("critic", &row), ("evaluator", &evaluator_row)] {
        assert!(row.contains("hint:"), "the {which} row says what it answers:{row}");
    }
    assert!(
        source.contains("item.hint"),
        "…and the hint is rendered, not merely declared — `title={{collapsed ? item.label : \
         item.hint}}` is what puts it in front of the operator",
    );
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

    // The stage reads it, keyed on there being a run to interview.
    assert!(
        critic_stage_block().contains("interview.toggle"),
        "and the Critic's view is what calls it",
    );
    assert!(
        read(STAGE).contains("useCriticInterview(started)"),
        "the gate is asked about when the fleet is up — with no run there is nothing to \
         interview, and claiming \"closed\" would be asserting a fact about a run that does \
         not exist ({STAGE})",
    );
}
