//! The briefings appended to each pane's system prompt at spawn (D-030, D-042).
//!
//! These are the *only* thing that tells a live `claude` it is part of a fleet.
//! They go in via `--append-system-prompt`, never as a `CLAUDE.md` in the pane's
//! cwd — a file there would show up in `git status` and the worker could delete it.
//!
//! **The prose lives in `prompts/*.md`, not here.** This module is the renderer:
//! it bakes those files in with `include_str!` so the binary always has a working
//! brief, fills their placeholders, and says whether a template is usable. The
//! operator's own copies are loaded from disk by `src-tauri::prompts` and handed
//! back here as a `template` argument — `fleetor-core` does no I/O (lib.rs §1).
//!
//! Two of the four files are **fragments, not templates**: `delivery-contract.md`
//! and `broadcast-rule.md` are composed *into* the briefs at a placeholder rather
//! than written out in them. That is deliberate. Both are load-bearing —
//! the first is the only reason a model can tell a failed send from a good one,
//! the second is the only mitigation left for broadcast amplification after the
//! rate limiter was removed (D-031) — and a template that drops its placeholder
//! is refused rather than rendered. An operator can rewrite everything around
//! them; they cannot be lost by editing prose.
//!
//! [`VERBS`] is the shared list the briefs are checked against and the `fleet` CLI
//! registers its subcommands from, so a rename shows up in both places at once.
//! The tests below assert against **hard-coded literals** on purpose: a test that
//! read `VERBS` would happily pass while the brief taught the model a verb that no
//! longer exists.

use crate::pane::PaneId;

/// Every `fleet` verb, in the order the briefs introduce them. The Phase-3 CLI
/// asserts its clap subcommands match this list exactly.
pub const VERBS: [&str; 5] = ["send", "broadcast", "reply", "roster", "whoami"];

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

/// The placeholders an orchestrator template must contain to be usable.
const ORCH_PLACEHOLDERS: [&str; 2] = ["workers", "delivery_contract"];

/// The placeholders a worker template must contain to be usable.
const WORKER_PLACEHOLDERS: [&str; 4] = ["me", "peers", "delivery_contract", "broadcast_rule"];

// --- the briefs ---------------------------------------------------------------

/// Briefing for the orchestrator pane, from the baked-in template — the operator's
/// own `claude`, driving the fleet. It keeps the operator's model and permissions;
/// all this adds is the existence of the other four terminals and how to reach them.
pub fn orch_brief(roster: &[PaneId]) -> String {
    render_orch(DEFAULT_ORCH, roster)
}

/// Briefing for a worker pane, from the baked-in template. Shorter than the
/// orchestrator's, and carrying the L5 anti-amplification clause — five peers that
/// all reply to broadcasts is a token fire that looks like a working fleet.
pub fn worker_brief(me: PaneId, roster: &[PaneId]) -> String {
    render_worker(DEFAULT_WORKER, me, roster)
}

/// The orchestrator brief from an arbitrary template — the seam the operator's own
/// `orch.md` comes in through. Validate it first; this renders whatever it is given.
pub fn render_orch(template: &str, roster: &[PaneId]) -> String {
    let workers = peer_list(roster, PaneId::Orch);
    render(
        template,
        &[("workers", workers.as_str()), ("delivery_contract", DELIVERY_CONTRACT.trim_end())],
    )
}

/// One worker's brief from an arbitrary template. `me` and `peers` are the only
/// things that differ between the four slots.
pub fn render_worker(template: &str, me: PaneId, roster: &[PaneId]) -> String {
    let peers = peer_list(roster, me);
    let name = me.to_string();
    render(
        template,
        &[
            ("me", name.as_str()),
            ("peers", peers.as_str()),
            ("delivery_contract", DELIVERY_CONTRACT.trim_end()),
            ("broadcast_rule", BROADCAST_RULE.trim_end()),
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
    require_verbs(&render_orch(template, &default_roster()))
}

/// Whether a worker template can be used.
pub fn validate_worker(template: &str) -> Result<(), String> {
    require_placeholders(template, &WORKER_PLACEHOLDERS)?;
    require_verbs(&render_worker(template, PaneId::Worker(1), &default_roster()))
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

    /// A verb rename must not be able to leave the briefs teaching the old one.
    /// Asserted against literals, not `VERBS`, so this test is the tripwire.
    #[test]
    fn both_briefs_teach_every_cli_verb() {
        let literals = ["send", "broadcast", "reply", "roster", "whoami"];
        assert_eq!(literals.to_vec(), VERBS.to_vec(), "VERBS drifted from the verbs the briefs teach");
        for brief in [orch_brief(&roster()), worker_brief(PaneId::Worker(1), &roster())] {
            for verb in literals {
                assert!(brief.contains(&format!("fleet {verb}")), "brief never mentions `fleet {verb}`");
            }
        }
    }

    #[test]
    fn the_worker_brief_carries_the_anti_amplification_clause() {
        let brief = worker_brief(PaneId::Worker(2), &roster());
        assert!(brief.contains("Never reply to a broadcast unless it names you"), "L5 clause missing");
    }

    #[test]
    fn both_briefs_state_that_a_nonzero_exit_means_not_delivered() {
        for brief in [orch_brief(&roster()), worker_brief(PaneId::Worker(1), &roster())] {
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
        assert!(worker_brief(PaneId::Worker(1), &roster()).contains(prefix), "worker brief shows {prefix}");
        assert!(orch_brief(&roster()).contains("[fleet · worker-2]"));
        assert!(orch_brief(&roster()).contains("→ all"), "orch must be able to spot a broadcast too");
    }

    #[test]
    fn a_pane_is_never_listed_among_its_own_peers() {
        let brief = worker_brief(PaneId::Worker(2), &roster());
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
        for brief in [orch_brief(&roster()), worker_brief(PaneId::Worker(3), &roster())] {
            for name in ORCH_PLACEHOLDERS.iter().chain(WORKER_PLACEHOLDERS.iter()) {
                assert!(!brief.contains(&format!("{{{name}}}")), "unrendered {{{name}}}: {brief}");
            }
        }
    }

    /// The four workers share one template on purpose; only their identity differs.
    #[test]
    fn every_worker_renders_the_same_template_with_its_own_name() {
        let one = worker_brief(PaneId::Worker(1), &roster());
        let three = worker_brief(PaneId::Worker(3), &roster());
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
        }
        assert!(
            !DEFAULT_WORKER.contains("Never reply to a broadcast"),
            "the broadcast rule is spelled out in the template instead of composed in",
        );
    }

    /// The whole point of the fragment split: prose can be rewritten freely and
    /// the load-bearing clauses still arrive.
    #[test]
    fn a_rewritten_template_keeps_the_clauses_it_cannot_afford_to_lose() {
        let rewritten = "# hi {me}\n\nyour peers: {peers}. use fleet send / fleet broadcast / \
             fleet reply / fleet roster / fleet whoami.\n\n{delivery_contract}\n\n{broadcast_rule}\n";
        validate_worker(rewritten).expect("a template with every placeholder is usable");

        let brief = render_worker(rewritten, PaneId::Worker(2), &roster());
        assert!(brief.contains("exits non-zero"), "the delivery contract still arrives");
        assert!(brief.contains("Never reply to a broadcast unless it names you"), "L5 still arrives");
        assert!(brief.contains("You are `worker-2`") || brief.contains("hi worker-2"));
    }

    /// A template that lost a placeholder is refused, and the refusal names the
    /// one to put back — the operator reads this on the Activity feed.
    #[test]
    fn a_template_that_drops_a_load_bearing_placeholder_is_refused() {
        let no_contract = "# {me}\n\npeers: {peers}\n\n{broadcast_rule}\n";
        let why = validate_worker(no_contract).expect_err("must not be usable");
        assert!(why.contains("{delivery_contract}"), "the refusal names the placeholder: {why}");

        let no_rule = "# {me}\n\npeers: {peers}\n\n{delivery_contract}\n";
        assert!(validate_worker(no_rule).is_err(), "a worker brief without the L5 rule is not usable");

        let no_workers = "# orch\n\n{delivery_contract}\n";
        assert!(validate_orch(no_workers).is_err(), "an orch brief that never names its peers");
    }

    /// A template can keep every placeholder and still teach a verb list that has
    /// drifted from the CLI. That is a pane running commands which exit 2.
    #[test]
    fn a_template_that_forgets_a_verb_is_refused() {
        let missing_whoami = "# {me}\n\npeers: {peers}. fleet send / fleet broadcast / fleet reply / \
             fleet roster.\n\n{delivery_contract}\n\n{broadcast_rule}\n";
        let why = validate_worker(missing_whoami).expect_err("must not be usable");
        assert!(why.contains("fleet whoami"), "the refusal names the missing verb: {why}");
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
