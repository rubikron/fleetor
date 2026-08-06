//! Each pane's system prompt at spawn (D-030, D-042, D-043).
//!
//! These are the *only* thing that tells a live `claude` it is part of a fleet.
//! They go in via `--system-prompt`, never as a `CLAUDE.md` in the pane's cwd —
//! a file there would show up in `git status` and the worker could delete it.
//!
//! **This is a replacement, not an append (D-043).** Since 2.1.223 the flag is
//! `--system-prompt`, so what is rendered here is the pane's whole prompt rather
//! than a postscript to Claude Code's own. `docs/system-prompt-notes.md` measured
//! what that costs: CC's ~6.5 KB of guidance leaves, while its tools, the memory
//! files, the skills listing and the git-status section all stay. The part worth
//! restating is `scaffolding.md`, and the one thing genuinely lost is the working
//! directory — hence `{cwd}`.
//!
//! **The prose lives in `prompts/*.md`, not here.** This module is the renderer:
//! it bakes those files in with `include_str!` so the binary always has a working
//! brief, fills their placeholders, and says whether a template is usable. The
//! operator's own copies are loaded from disk by `src-tauri::prompts` and handed
//! back here as a `template` argument — `fleetor-core` does no I/O (lib.rs §1).
//!
//! Three of the files are **fragments, not templates**: `delivery-contract.md`,
//! `broadcast-rule.md` and `scaffolding.md` are composed *into* the briefs at a
//! placeholder rather than written out in them. That is deliberate. All three are
//! load-bearing — the first is the only reason a model can tell a failed send
//! from a good one, the second is the only mitigation left for broadcast
//! amplification after the rate limiter was removed (D-031), and the third is the
//! working posture a pane no longer inherits from CC now that the prompt is
//! replaced rather than appended (D-043). A template that drops its placeholder is
//! refused rather than rendered. An operator can rewrite everything around them;
//! they cannot be lost by editing prose.
//!
//! [`VERBS`] is the shared list the briefs are checked against and the `fleet` CLI
//! registers its subcommands from, so a rename shows up in both places at once.
//! The tests below assert against **hard-coded literals** on purpose: a test that
//! read `VERBS` would happily pass while the brief taught the model a verb that no
//! longer exists.

use crate::pane::PaneId;

/// Every `fleet` verb, in the order the briefs introduce them. The Phase-3 CLI
/// asserts its clap subcommands match this list exactly.
pub const VERBS: [&str; 8] =
    ["send", "broadcast", "reply", "cmd", "task", "done", "roster", "whoami"];

/// The baked-in orchestrator template. Used when the operator has not put their
/// own `orch.md` in `~/.fleetor/prompts/`, and as the fallback when the one they
/// did put there fails [`validate_orch`].
pub const DEFAULT_ORCH: &str = include_str!("../../../prompts/orch.md");

/// The baked-in worker template. Every worker slot renders from this same file —
/// they differ only in `{me}` and `{peers}`.
pub const DEFAULT_WORKER: &str = include_str!("../../../prompts/worker.md");

/// What a `fleet` exit code means. Composed into both briefs verbatim.
const DELIVERY_CONTRACT: &str = include_str!("../../../prompts/delivery-contract.md");

/// The L5 anti-amplification clause. Composed into the worker brief verbatim.
const BROADCAST_RULE: &str = include_str!("../../../prompts/broadcast-rule.md");

/// The working posture a pane used to inherit from Claude Code's own system
/// prompt and no longer does (D-043). Composed into both briefs verbatim.
const SCAFFOLDING: &str = include_str!("../../../prompts/scaffolding.md");

/// `docs/futureDesign/vision_tenets.md`, distilled to what an orchestrator can
/// act on (D-044). Composed into the orchestrator brief verbatim — a fragment
/// rather than prose in `orch.md` so an operator can tune how the fleet talks
/// about vision without touching what it does.
const VISION_TENETS: &str = include_str!("../../../prompts/vision-tenets.md");

/// The placeholders an orchestrator template must contain to be usable.
const ORCH_PLACEHOLDERS: [&str; 5] =
    ["cwd", "workers", "delivery_contract", "scaffolding", "vision_tenets"];

/// The placeholders a worker template must contain to be usable.
const WORKER_PLACEHOLDERS: [&str; 6] =
    ["me", "cwd", "peers", "delivery_contract", "broadcast_rule", "scaffolding"];

/// What `validate_orch` / `validate_worker` render `{cwd}` as. A template is
/// checked for verbs and placeholders, never for a real path, and inventing one
/// here would only make the failure messages lie about where a pane runs.
const CWD_FOR_VALIDATION: &str = "the pane's working directory";

// --- the briefs ---------------------------------------------------------------

/// Briefing for the orchestrator pane, from the baked-in template — the operator's
/// own `claude`, driving the fleet. It keeps the operator's model and permissions;
/// all this adds is the existence of the other four terminals and how to reach them.
pub fn orch_brief(roster: &[PaneId], cwd: &str) -> String {
    render_orch(DEFAULT_ORCH, roster, cwd)
}

/// Briefing for a worker pane, from the baked-in template. Shorter than the
/// orchestrator's, and carrying the L5 anti-amplification clause — five peers that
/// all reply to broadcasts is a token fire that looks like a working fleet.
pub fn worker_brief(me: PaneId, roster: &[PaneId], cwd: &str) -> String {
    render_worker(DEFAULT_WORKER, me, roster, cwd)
}

/// The orchestrator brief from an arbitrary template — the seam the operator's own
/// `orch.md` comes in through. Validate it first; this renders whatever it is given.
pub fn render_orch(template: &str, roster: &[PaneId], cwd: &str) -> String {
    let workers = peer_list(roster, PaneId::Orch);
    render(
        template,
        &[
            ("cwd", cwd),
            ("workers", workers.as_str()),
            ("delivery_contract", DELIVERY_CONTRACT.trim_end()),
            ("scaffolding", SCAFFOLDING.trim_end()),
            ("vision_tenets", VISION_TENETS.trim_end()),
        ],
    )
}

/// One worker's brief from an arbitrary template. `me`, `peers` and `cwd` are the
/// only things that differ between the four slots.
pub fn render_worker(template: &str, me: PaneId, roster: &[PaneId], cwd: &str) -> String {
    let peers = peer_list(roster, me);
    let name = me.to_string();
    render(
        template,
        &[
            ("me", name.as_str()),
            ("cwd", cwd),
            ("peers", peers.as_str()),
            ("delivery_contract", DELIVERY_CONTRACT.trim_end()),
            ("broadcast_rule", BROADCAST_RULE.trim_end()),
            ("scaffolding", SCAFFOLDING.trim_end()),
        ],
    )
}

/// Substitute `{name}` for each `(name, value)`. Deliberately a plain replace and
/// not `format!`: a template read from disk is not a literal, and prose that
/// happens to contain a brace must not be an error the operator has to debug.
pub fn render(template: &str, vars: &[(&str, &str)]) -> String {
    vars.iter()
        .fold(template.to_string(), |acc, (name, value)| acc.replace(&format!("{{{name}}}"), value))
}

// --- validation ---------------------------------------------------------------

/// Whether an orchestrator template can be used. `Err` carries a sentence the
/// operator can act on — it reaches them as a `Warn` on the Activity feed.
pub fn validate_orch(template: &str) -> Result<(), String> {
    require_placeholders(template, &ORCH_PLACEHOLDERS)?;
    require_verbs(&render_orch(template, &default_roster(), CWD_FOR_VALIDATION))
}

/// Whether a worker template can be used.
pub fn validate_worker(template: &str) -> Result<(), String> {
    require_placeholders(template, &WORKER_PLACEHOLDERS)?;
    require_verbs(&render_worker(template, PaneId::Worker(1), &default_roster(), CWD_FOR_VALIDATION))
}

fn default_roster() -> Vec<PaneId> {
    PaneId::roster(&crate::pane::WORKER_SLOTS)
}

/// Every placeholder must survive an edit. A missing `{delivery_contract}` is not
/// a cosmetic loss — it is a pane that cannot tell a failed send from a good one.
fn require_placeholders(template: &str, needed: &[&str]) -> Result<(), String> {
    match needed.iter().find(|name| !template.contains(&format!("{{{name}}}"))) {
        None => Ok(()),
        Some(missing) => Err(format!(
            "the template is missing its {{{missing}}} placeholder — \
             a brief without it would leave the pane unable to work. \
             Put {{{missing}}} back, or delete the file to use the built-in brief"
        )),
    }
}

/// A rendered brief must teach every verb the CLI actually has. A brief naming a
/// verb that does not exist teaches the model a command that exits 2, and one
/// omitting a verb hides a capability the fleet is built around.
fn require_verbs(rendered: &str) -> Result<(), String> {
    match VERBS.iter().find(|verb| !rendered.contains(&format!("fleet {verb}"))) {
        None => Ok(()),
        Some(missing) => Err(format!(
            "the rendered brief never mentions `fleet {missing}` — \
             a pane taught an incomplete verb list cannot use the fleet properly"
        )),
    }
}

/// "worker-1, worker-2 and worker-3" — the roster minus yourself, in prose.
fn peer_list(roster: &[PaneId], me: PaneId) -> String {
    let names: Vec<String> = roster.iter().filter(|p| **p != me).map(|p| p.to_string()).collect();
    match names.split_last() {
        None => "nobody else, yet".to_string(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pane::WORKER_SLOTS;

    fn roster() -> Vec<PaneId> {
        PaneId::roster(&WORKER_SLOTS)
    }

    /// Every test renders against the same working directory, so a brief that
    /// differs between two slots differs for a reason other than where it runs.
    const CWD: &str = "/tmp/fleetor-test-target";

    /// A verb rename must not be able to leave the briefs teaching the old one.
    /// Asserted against literals, not `VERBS`, so this test is the tripwire.
    #[test]
    fn both_briefs_teach_every_cli_verb() {
        let literals = ["send", "broadcast", "reply", "cmd", "task", "done", "roster", "whoami"];
        assert_eq!(literals.to_vec(), VERBS.to_vec(), "VERBS drifted from the verbs the briefs teach");
        for brief in [orch_brief(&roster(), CWD), worker_brief(PaneId::Worker(1), &roster(), CWD)] {
            for verb in literals {
                assert!(brief.contains(&format!("fleet {verb}")), "brief never mentions `fleet {verb}`");
            }
        }
    }

    #[test]
    fn the_worker_brief_carries_the_anti_amplification_clause() {
        let brief = worker_brief(PaneId::Worker(2), &roster(), CWD);
        assert!(brief.contains("Never reply to a broadcast unless it names you"), "L5 clause missing");
    }

    #[test]
    fn both_briefs_state_that_a_nonzero_exit_means_not_delivered() {
        for brief in [orch_brief(&roster(), CWD), worker_brief(PaneId::Worker(1), &roster(), CWD)] {
            assert!(brief.contains("exits non-zero"));
            assert!(brief.contains("not a promise"), "must not let the model read success as delivery (L3)");
        }
    }

    #[test]
    fn briefs_teach_the_framing_the_receiver_actually_sees() {
        // Cross-check against the single source of truth in `message`, so a
        // framing change cannot leave the briefs describing the old one.
        let direct = crate::message::frame_for_pane(PaneId::Worker(3), "x");
        let prefix = direct.trim_end_matches(" x");
        assert!(worker_brief(PaneId::Worker(1), &roster(), CWD).contains(prefix), "worker brief shows {prefix}");
        assert!(orch_brief(&roster(), CWD).contains("[fleet · worker-2]"));
        assert!(orch_brief(&roster(), CWD).contains("→ all"), "orch must be able to spot a broadcast too");
    }

    #[test]
    fn a_pane_is_never_listed_among_its_own_peers() {
        let brief = worker_brief(PaneId::Worker(2), &roster(), CWD);
        let peers = brief.lines().find(|l| l.contains("coordinates you")).unwrap();
        assert!(!peers.contains("worker-2"), "worker-2 listed itself as a peer: {peers}");
        assert!(peers.contains("worker-1") && peers.contains("worker-4"));
    }

    #[test]
    fn peer_prose_reads_correctly_at_every_size() {
        assert_eq!(peer_list(&[PaneId::Orch], PaneId::Orch), "nobody else, yet");
        assert_eq!(peer_list(&PaneId::roster(&[1]), PaneId::Orch), "worker-1");
        assert_eq!(peer_list(&PaneId::roster(&[1, 2]), PaneId::Orch), "worker-1 and worker-2");
        assert_eq!(
            peer_list(&PaneId::roster(&[1, 2, 3]), PaneId::Orch),
            "worker-1, worker-2 and worker-3"
        );
    }

    // --- the externalized templates -------------------------------------------

    /// Nothing may be left behind after rendering. A stray `{peers}` in a live
    /// system prompt is a pane being told the literal word "{peers}" — which
    /// reads as a working brief right up until it matters.
    #[test]
    fn a_rendered_brief_has_no_placeholders_left_in_it() {
        for brief in [orch_brief(&roster(), CWD), worker_brief(PaneId::Worker(3), &roster(), CWD)] {
            for name in ORCH_PLACEHOLDERS.iter().chain(WORKER_PLACEHOLDERS.iter()) {
                assert!(!brief.contains(&format!("{{{name}}}")), "unrendered {{{name}}}: {brief}");
            }
        }
    }

    /// The four workers share one template on purpose; only their identity differs.
    #[test]
    fn every_worker_renders_the_same_template_with_its_own_name() {
        let one = worker_brief(PaneId::Worker(1), &roster(), CWD);
        let three = worker_brief(PaneId::Worker(3), &roster(), CWD);
        assert!(one.contains("You are `worker-1`"));
        assert!(three.contains("You are `worker-3`"));

        // Exactly two lines may differ — the title and the peer list. Everything
        // else is the one shared template, byte for byte.
        assert_eq!(one.lines().count(), three.lines().count(), "the briefs are not the same shape");
        let differing: Vec<&str> =
            one.lines().zip(three.lines()).filter(|(a, b)| a != b).map(|(a, _)| a).collect();
        assert_eq!(differing.len(), 2, "more than identity differs: {differing:#?}");
        assert!(differing[0].contains("You are `worker-1`"), "{differing:#?}");
        assert!(differing[1].contains("coordinates you and your peers"), "{differing:#?}");
    }

    /// The fragments are composed in, not written out — so an operator rewriting
    /// the prose around them cannot drop them.
    #[test]
    fn the_locked_fragments_are_not_written_out_in_the_templates() {
        for template in [DEFAULT_ORCH, DEFAULT_WORKER] {
            assert!(
                !template.contains("exits non-zero"),
                "the delivery contract is spelled out in the template instead of composed in",
            );
            assert!(
                !template.contains("rendered as GitHub-flavored markdown"),
                "the scaffolding is spelled out in the template instead of composed in",
            );
            assert!(
                !template.contains("A vision is as much what it is not"),
                "the tenets are spelled out in the template instead of composed in",
            );
        }
        assert!(
            !DEFAULT_WORKER.contains("Never reply to a broadcast"),
            "the broadcast rule is spelled out in the template instead of composed in",
        );
    }

    /// D-043: the prompt is a *replacement* now, so everything CC used to supply
    /// and no longer does has to arrive from here. These are the clauses whose
    /// absence changes what a pane will actually do, pinned as literals for the
    /// same reason the verb list is — a reworded `scaffolding.md` that quietly
    /// dropped one would still render, still validate, and still look right.
    #[test]
    fn both_briefs_restate_the_posture_the_default_prompt_used_to_supply() {
        for brief in [orch_brief(&roster(), CWD), worker_brief(PaneId::Worker(1), &roster(), CWD)] {
            assert!(brief.contains("Report outcomes faithfully"), "a fleet runs on honest reports");
            assert!(brief.contains("never wrap up early"), "a pane must not abandon work for context");
            assert!(brief.contains("adapt rather than retrying it verbatim"), "denied means declined");
            assert!(brief.contains("they/them"), "the pronoun default governs user-visible text");
            assert!(brief.contains("refuse destructive techniques"), "the security posture is not optional");
        }
    }

    // --- the vision-partner content (D-044) -----------------------------------

    /// The rule the whole package exists for: the orchestrator confirms the
    /// vision in writing *before* it decomposes anything. Pinned as a literal,
    /// like the anti-amplification clause — a rewrite that softened this into
    /// "consider checking with the operator" would still render and still
    /// validate, and the fleet would go back to starting fast on the wrong thing.
    #[test]
    fn the_orch_brief_confirms_the_vision_in_writing_before_decomposing() {
        let brief = orch_brief(&roster(), CWD);
        assert!(brief.contains("Before you decompose anything into work"));
        assert!(brief.contains("State the vision back in writing, and get a yes"));
        assert!(
            brief.contains("Nothing goes to a worker until the vision is confirmed"),
            "the rule needs a consequence, not just an instruction",
        );
    }

    /// The authority bound. "Guide the operator to think bigger" is one proposal,
    /// then deference — an orchestrator that keeps relitigating the goal is worse
    /// than one that never raised it.
    #[test]
    fn the_orch_brief_proposes_a_bigger_frame_at_most_once() {
        let brief = orch_brief(&roster(), CWD);
        assert!(brief.contains("propose a bigger frame — once"));
        assert!(brief.contains("adopt their frame fully"), "declining must end it");
        assert!(brief.contains("their explicit word is final"));
        assert!(brief.contains("Do not raise it again in the same session"));
    }

    /// The integrity clause, and the sentence it is built on: a better route to
    /// the operator's vision is welcome, a substituted vision never is, and a
    /// deviation the operator finds out about later is the failure being guarded.
    #[test]
    fn the_orch_brief_may_improve_the_route_but_never_swaps_the_vision() {
        let brief = orch_brief(&roster(), CWD);
        assert!(brief.contains("never quietly substitute a vision of your own"));
        assert!(brief.contains("announced, not discovered later"));
        assert!(
            brief.contains("information to factor in, not an instruction that overrides what the operator asked"),
            "the pre-existing authority anchor must survive the rewrite",
        );
    }

    /// The three attitudes, and the ME→WE line. These are the whole of what
    /// distinguishes a FLEETOR worker from a `claude` in a worktree.
    #[test]
    fn the_worker_brief_carries_the_three_attitudes() {
        let brief = worker_brief(PaneId::Worker(2), &roster(), CWD);
        assert!(brief.contains("How can I be better?"));
        assert!(brief.contains("How can I push for more positive, meaningful impact?"));
        assert!(brief.contains("What do I do when I am confused?"));
        assert!(brief.contains("Improve yourself and the team around you"), "the ME→WE line");
    }

    /// Until WP-07 gives workers a way to address the operator directly, a
    /// question for the human routes through `orch`. This test is the reminder
    /// that the sentence exists and is meant to be replaced, not deleted.
    #[test]
    fn a_confused_worker_is_told_exactly_who_to_ask() {
        let brief = worker_brief(PaneId::Worker(3), &roster(), CWD);
        assert!(brief.contains("Ask a peer by name"));
        assert!(
            brief.contains("ask `orch` to put it to the operator"),
            "WP-07 replaces this sentence with direct addressing; until then it must be here",
        );
    }

    /// The tenets are distilled, not pasted. The source essay's quotations and
    /// framing are exactly what a brief cannot afford — this asserts the operative
    /// lines arrived and the essay did not come with them.
    #[test]
    fn the_orch_brief_distills_the_tenets_rather_than_quoting_them() {
        let brief = orch_brief(&roster(), CWD);
        for operative in [
            "Write the vision down",
            "A vision is as much what it is not",
            "The vision is the filter",
            "More than one vision is division",
            "ME → WE",
        ] {
            assert!(brief.contains(operative), "the tenets lost `{operative}`");
        }
        for essay in ["Habakkuk", "Wright Brothers", "Steve Jobs", "Michael Hyatt", "podcast"] {
            assert!(!brief.contains(essay), "the essay's framing leaked into the brief: {essay}");
        }
    }

    /// The one thing `--system-prompt` genuinely takes away: CC's `# Environment`
    /// section carried the working directory, and nothing else in the request
    /// names it. A pane that does not know where it is cannot be trusted to edit.
    #[test]
    fn every_brief_says_where_the_pane_is_working() {
        assert!(orch_brief(&roster(), "/tmp/target").contains("/tmp/target"));
        assert!(worker_brief(PaneId::Worker(2), &roster(), "/tmp/wt-2").contains("/tmp/wt-2"));
    }

    /// The whole point of the fragment split: prose can be rewritten freely and
    /// the load-bearing clauses still arrive.
    #[test]
    fn a_rewritten_template_keeps_the_clauses_it_cannot_afford_to_lose() {
        let rewritten = "# hi {me} in {cwd}\n\nyour peers: {peers}. use fleet send / fleet broadcast / \
             fleet reply / fleet cmd / fleet task / fleet done / fleet roster / fleet whoami.\n\n\
             {delivery_contract}\n\n{broadcast_rule}\n\n{scaffolding}\n";
        validate_worker(rewritten).expect("a template with every placeholder is usable");

        let brief = render_worker(rewritten, PaneId::Worker(2), &roster(), CWD);
        assert!(brief.contains("exits non-zero"), "the delivery contract still arrives");
        assert!(brief.contains("Never reply to a broadcast unless it names you"), "L5 still arrives");
        assert!(brief.contains("You are `worker-2`") || brief.contains("hi worker-2"));
    }

    /// A template that lost a placeholder is refused, and the refusal names the
    /// one to put back — the operator reads this on the Activity feed.
    #[test]
    fn a_template_that_drops_a_load_bearing_placeholder_is_refused() {
        let no_contract = "# {me} in {cwd}\n\npeers: {peers}\n\n{broadcast_rule}\n\n{scaffolding}\n";
        let why = validate_worker(no_contract).expect_err("must not be usable");
        assert!(why.contains("{delivery_contract}"), "the refusal names the placeholder: {why}");

        let no_rule = "# {me} in {cwd}\n\npeers: {peers}\n\n{delivery_contract}\n\n{scaffolding}\n";
        assert!(validate_worker(no_rule).is_err(), "a worker brief without the L5 rule is not usable");

        let no_workers = "# orch in {cwd}\n\n{delivery_contract}\n\n{scaffolding}\n\n{vision_tenets}\n";
        assert!(validate_orch(no_workers).is_err(), "an orch brief that never names its peers");

        // D-043's additions are load-bearing the same way, and refused the same way.
        let no_scaffolding = "# {me} in {cwd}\n\npeers: {peers}\n\n{delivery_contract}\n\n{broadcast_rule}\n";
        let why = validate_worker(no_scaffolding).expect_err("must not be usable");
        assert!(why.contains("{scaffolding}"), "the refusal names the placeholder: {why}");

        let no_cwd = "# {me}\n\npeers: {peers}\n\n{delivery_contract}\n\n{broadcast_rule}\n\n{scaffolding}\n";
        let why = validate_worker(no_cwd).expect_err("must not be usable");
        assert!(why.contains("{cwd}"), "the refusal names the placeholder: {why}");
    }

    /// A template can keep every placeholder and still teach a verb list that has
    /// drifted from the CLI. That is a pane running commands which exit 2.
    #[test]
    fn a_template_that_forgets_a_verb_is_refused() {
        let missing_whoami = "# {me} in {cwd}\n\npeers: {peers}. fleet send / fleet broadcast / \
             fleet reply / fleet cmd / fleet task / fleet done / fleet roster.\n\n\
             {delivery_contract}\n\n{broadcast_rule}\n\n{scaffolding}\n";
        let why = validate_worker(missing_whoami).expect_err("must not be usable");
        assert!(why.contains("fleet whoami"), "the refusal names the missing verb: {why}");
    }

    // --- the command channel (D-045) ------------------------------------------

    /// The mandatory `--why` is the reason the verb exists at all, so both briefs
    /// have to teach it as a requirement rather than a nicety. Pinned as literals
    /// for the same reason the anti-amplification clause is: a rewrite that
    /// softened it to "you may add a note" would still render and still validate,
    /// and the log would fill with effects whose reasons were never written down.
    #[test]
    fn both_briefs_teach_the_command_verb_with_its_mandatory_why() {
        for brief in [orch_brief(&roster(), CWD), worker_brief(PaneId::Worker(1), &roster(), CWD)] {
            assert!(brief.contains("fleet cmd"), "the verb itself");
            assert!(brief.contains("`--why` is required"), "the why must read as a requirement");
            assert!(
                brief.contains("a later self-improvement pass reads"),
                "the why is a reasoning chain, not paperwork",
            );
            // The allowlist is a constant in `command.rs`; a brief that promised
            // more would teach a command the hub refuses.
            for allowed in crate::command::ALLOWED_COMMANDS {
                assert!(brief.contains(allowed), "the brief never names {allowed}");
            }
            assert!(brief.contains("Anything else is refused"), "the boundary is stated");
        }
    }

    /// The worker's post-task self-maintenance move. Without this sentence the
    /// verb exists and nobody uses it: a worker has no other prompt to look at
    /// its own context, and "between tasks, never mid-task" is what keeps it from
    /// compacting away the thing it is in the middle of.
    #[test]
    fn the_worker_brief_teaches_the_post_task_self_maintenance_move() {
        let brief = worker_brief(PaneId::Worker(2), &roster(), CWD);
        assert!(brief.contains("When you finish a block of work, look at your own context"));
        assert!(brief.contains("fleet cmd self"), "the worker points the verb at itself");
        assert!(brief.contains("never mid-task"), "the timing bound is not optional");
    }

    /// The after-`/clear` rule, and it is a *prompt* rule on purpose: nothing in
    /// the delivery path may gate, delay or follow up a command (Tier 1.4), so
    /// the only place a re-brief can be required is here.
    #[test]
    fn the_orch_brief_re_briefs_a_worker_it_clears() {
        let brief = orch_brief(&roster(), CWD);
        assert!(brief.contains("If you clear a worker, immediately `fleet send` it its task context back"));
        assert!(
            brief.contains("report confident nonsense"),
            "the rule needs its consequence, or it reads as advice",
        );
    }

    // --- the task board (WP-05) -------------------------------------------------

    /// The sentence the whole package rests on. The board is a record; the
    /// **send** is the assignment. Pinned as a literal because a rewrite that
    /// softened it — "post the block and the worker picks it up" — would still
    /// render, still validate, and would teach the fleet to wait on a board that
    /// nothing dispatches from. That is the ticket system growing back in prose
    /// instead of code.
    #[test]
    fn the_orch_brief_says_the_send_is_the_assignment_and_the_board_is_the_record() {
        let brief = orch_brief(&roster(), CWD);
        assert!(brief.contains("Posting a block assigns nobody"));
        assert!(brief.contains("nothing reads it, nothing runs from it"));
        assert!(brief.contains("The send is the assignment"));
        assert!(
            brief.contains("a claim its author made rather than a verified fact"),
            "`done` is unverified until WP-06's review; the brief must not promise otherwise",
        );
    }

    /// Decomposition happens *after* the vision is confirmed (WP-02's clause),
    /// and the criteria have to be falsifiable or the block cannot be argued with.
    #[test]
    fn the_orch_brief_decomposes_only_after_the_vision_and_demands_real_criteria() {
        let brief = orch_brief(&roster(), CWD);
        assert!(brief.contains("Once the vision is confirmed, cut the work into blocks"));
        assert!(brief.contains("Write criteria that could fail"));
        assert!(brief.contains("\"Works well\" cannot"), "the counter-example is the teaching");
    }

    /// The worker's half: the criteria *are* done, checking comes before claiming,
    /// and the claim is a claim. Without these three sentences the verb exists and
    /// the board fills up with unchecked `done`s.
    #[test]
    fn the_worker_brief_treats_the_criteria_as_the_definition_of_done() {
        let brief = worker_brief(PaneId::Worker(2), &roster(), CWD);
        assert!(brief.contains("the definition of done, not a summary of it"));
        assert!(brief.contains("Before you claim done, actually run the technical checks"));
        assert!(
            brief.contains("a claim you are making with your name on it"),
            "the claim must not read as a system-verified fact",
        );
        assert!(brief.contains("fleet task update"), "the worker knows how to say it");
    }

    /// Both briefs name the four statuses, because a model that invents
    /// `in-progress` gets a refusal instead of an update.
    #[test]
    fn both_briefs_name_every_status_the_board_accepts() {
        for brief in [orch_brief(&roster(), CWD), worker_brief(PaneId::Worker(1), &roster(), CWD)] {
            for status in crate::task::TASK_STATUSES {
                assert!(brief.contains(status), "the brief never names `{status}`");
            }
        }
    }

    // --- receipts, review and the merge (WP-06) ---------------------------------

    /// The delivery contract, restated where a new verb could quietly break it.
    /// `fleet done` is the only verb that runs something, so it is the only place
    /// a model could reasonably assume the exit code describes the *check*. If it
    /// did, a failing check would read as "not delivered" and the worker would
    /// resend a receipt that already arrived. Pinned as a literal, because a
    /// rewrite that dropped the sentence would still render and still validate.
    #[test]
    fn the_worker_brief_separates_the_receipt_from_the_checks_own_result() {
        let brief = worker_brief(PaneId::Worker(2), &roster(), CWD);
        assert!(brief.contains("fleet done"), "the verb itself");
        assert!(brief.contains("means only that the *receipt* did not arrive"));
        assert!(brief.contains("never in the exit code"), "the consequence, not just the rule");
        assert!(
            brief.contains("A failing check is information"),
            "a worker that hides a red check is the failure this verb exists to prevent",
        );
    }

    /// The receipt names a commit and the reviewer reads that commit. A worker who
    /// reports before committing sends its reviewer to look at code that does not
    /// contain the work — an honest review of the wrong thing.
    #[test]
    fn the_worker_brief_says_to_commit_before_reporting() {
        let brief = worker_brief(PaneId::Worker(2), &roster(), CWD);
        assert!(brief.contains("Commit your work first"));
        assert!(brief.contains("not the files still sitting in your worktree"));
    }

    /// Review happens from the reviewer's **own** worktree, over the shared object
    /// database (D-048). Two things are pinned as literals here and both are
    /// load-bearing: the three-dot diff, because the two-dot form a model would
    /// reach for first renders a peer's additions as deletions; and the Tier-1.7
    /// boundary, because `cd`-ing into a peer's checkout is the security
    /// escalation this arrangement exists to make unnecessary.
    #[test]
    fn the_worker_brief_reviews_from_its_own_worktree_with_the_right_diff() {
        let brief = worker_brief(PaneId::Worker(2), &roster(), CWD);
        assert!(brief.contains("Stay in your own worktree"));
        assert!(brief.contains("shares one git object database"), "why it works with no fetch");
        assert!(brief.contains("git diff HEAD...fleet/worker-3"), "the three-dot form");
        assert!(brief.contains("git log --oneline HEAD..fleet/worker-3"));
        assert!(
            brief.contains("two would show it backwards"),
            "the trap has to be named, or a reviewer reads a peer's work as a deletion",
        );
        assert!(brief.contains("Never `cd` into a peer's worktree"), "Tier 1.7");
    }

    /// A review answers the block's criteria. Without this the verb produces
    /// taste, and taste from a peer is the thing the criteria were written to
    /// replace.
    #[test]
    fn the_worker_brief_reviews_against_the_criteria_rather_than_taste() {
        let brief = worker_brief(PaneId::Worker(3), &roster(), CWD);
        assert!(brief.contains("not your taste"));
        assert!(brief.contains("name the criterion each finding is about"));
    }

    /// Tier 1.1, in the one place the fleet now has a reason to reach for trunk.
    /// Pinned as a literal for the same reason the vision clause is: "merge the
    /// reviewed branch" softened by one word becomes a fleet that merges to main.
    #[test]
    fn the_orch_brief_merges_to_integration_and_never_to_trunk() {
        let brief = orch_brief(&roster(), CWD);
        assert!(brief.contains("fleet/integration"));
        assert!(brief.contains("**never into trunk.**"));
        assert!(
            brief.contains("Trunk is the operator's, and they merge it themselves"),
            "the rule needs whose it is, or it reads as a temporary restriction",
        );
        assert!(brief.contains("git worktree add"), "orch must not switch the operator's checkout");
    }

    /// The reviewer is named at assignment time, by orch, in the message that is
    /// already the assignment — **not** by a field on the task block (D-049). The
    /// brief is therefore the only place the duty is created, so this is the test
    /// that says it exists at all.
    #[test]
    fn the_orch_brief_names_a_reviewer_who_is_neither_the_author_nor_itself() {
        let brief = orch_brief(&roster(), CWD);
        assert!(brief.contains("Name a reviewer in the same message that hands out the block"));
        assert!(brief.contains("never the block's author, and never you"));
        assert!(
            brief.contains("finds what it expected to find"),
            "the reason has to travel with the rule",
        );
    }

    /// A receipt is evidence, and orch must not read a zero exit as a finished
    /// block — that would put the verification back in the machine, which is
    /// exactly what "no hub-side verification" rules out.
    #[test]
    fn the_orch_brief_treats_a_receipt_as_evidence_rather_than_a_verdict() {
        let brief = orch_brief(&roster(), CWD);
        assert!(brief.contains("evidence, not a verdict"));
        assert!(brief.contains("not that the block is done"));
        assert!(
            brief.contains("Nothing in the code checks any of this"),
            "the merge rule is a prompt rule, and has to say so",
        );
    }

    /// What ships must pass the check it imposes on everyone else.
    #[test]
    fn the_baked_in_templates_validate() {
        validate_orch(DEFAULT_ORCH).expect("the shipped orch template is usable");
        validate_worker(DEFAULT_WORKER).expect("the shipped worker template is usable");
    }

    /// Prose with a brace in it is not a format string and must not be treated as
    /// one — the operator writes markdown, not Rust.
    #[test]
    fn a_brace_in_prose_survives_rendering() {
        let rendered = render("use {me} for `Vec<{}>` and {unknown}", &[("me", "worker-1")]);
        assert_eq!(rendered, "use worker-1 for `Vec<{}>` and {unknown}");
    }
}
