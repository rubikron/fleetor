//! The Critic (WP-20, D-076) — a real `claude` that reads the archive of one run
//! and reports what the fleet actually did, with a citation behind every finding.
//!
//! This module is everything about it that is *not* a pty, a view or a
//! placement: what it is told, and where it works.
//!
//! ## What it is, said against the pane it is most likely to be confused with
//!
//! It is not the evaluator, and the two are separate identities rather than one
//! with a mode flag (the arc's D5). Every difference below falls out of a single
//! fact — **the Critic is a reader of a run, not a participant in one, and it is
//! a product feature rather than an instrument of an experiment**:
//!
//!  - **Its brief is a file in `prompts/`**, baked in with `include_str!` and
//!    overridable from `~/.fleetor/prompts/critic.md` like every other brief
//!    (D-042). The operator is meant to tune it.
//!  - **It exists with dev mode off.** Nothing gates it. There is no readiness
//!    question to ask, because there is no answer key it could be missing.
//!  - **It is openly named** — in the rail, in the Activity feed, in this
//!    source. Nothing about it depends on the fleet not knowing it exists.
//!  - **It cannot message the fleet.** Placement hands it no `FLEET_SOCKET`, so
//!    a `fleet send` typed inside it dials nothing and exits non-zero. Findings
//!    reach the operator through its own view; anything worth acting on the
//!    operator forwards from the composer they already have (the arc's D6).
//!
//! ## The brief is the one the spike converged on
//!
//! `prompts/critic.md` is v3 from `docs/notes/critic-spike-notes.md` §6,
//! verbatim, with `{ARCHIVE}` spelled `{archive}` to match this codebase's
//! placeholder convention. Two clauses in it are load-bearing and the spike says
//! so in §8: **the citation rule** and **the ban on prescription**. The measured
//! control run — the same archive with an unconstrained prompt — produced fluent,
//! largely *true*, entirely uncitable prose that quietly graded the work. That is
//! the failure this brief is shaped against, and it is worse than the bland mush
//! the arc predicted, because an operator cannot tell its true claims from its
//! unfalsifiable ones.
//!
//! **Six categories, not the arc's five.** D10 names five; the spike added a
//! sixth — *"Anything else the fleet did that cost the operator"* — and it
//! produced the single largest finding on both measured runs while D10's
//! categories 3 and 4 produced nothing on either. It is a **bounded** slot: it
//! carries the same citation discipline, an explicit ban on inference and on
//! prescription, and the rule that a moment you cannot name belongs in UNCITED.
//! The spike's own words for that are what keep it from reopening the mush door,
//! so they are not to be loosened into a free-form opinion slot.

use std::path::{Path, PathBuf};

/// The baked-in brief, from `prompts/critic.md`.
///
/// In this directory and not in a separate repository, which is the whole of
/// "the operator can rewrite it": the resolver in [`crate::prompts`] reads
/// `~/.fleetor/prompts/critic.md` at bootstrap and announces what it found.
pub const DEFAULT_BRIEF: &str = include_str!("../../prompts/critic.md");

/// The one placeholder the brief must keep. A Critic pointed at no directory
/// spends its first turn asking the operator where the run is.
const PLACEHOLDER: &str = "archive";

/// Where the Critic's own files live: `~/.fleetor/critic/`.
///
/// **A sibling of `_shell/`, not a child, for the reason the evaluator's `dev/`
/// is one** (D-065): every fleet pane's write guardrail has `_shell` as a root,
/// so a run laid out under `_shell` would be writable by every worker in the run
/// being critiqued.
pub fn critic_dir(fleetor: &Path) -> PathBuf {
    fleetor.join("critic")
}

/// The Critic's working directory: `~/.fleetor/critic/runs/<run-id>/`, holding
/// the run it was pointed at in the archive's own shape.
pub fn run_dir(fleetor: &Path, run_id: &str) -> PathBuf {
    critic_dir(fleetor).join("runs").join(run_id)
}

/// The Critic's `CLAUDE_CONFIG_DIR`.
///
/// **Deliberately not under `_shell/pane-config/`**, where every fleet pane's
/// lives. `runs::copy_transcripts` walks that directory on every snapshot, so a
/// Critic housed there would have its own transcript copied into the very
/// directory it is reading — growing as it works — and `runs::harvest_transcripts`
/// would file it into the run archive at rotation, where the next run's Critic
/// would read the last one's reasoning as if it were the fleet's.
pub fn config_dir(fleetor: &Path) -> PathBuf {
    critic_dir(fleetor).join("config")
}

/// The Critic's whole system prompt, or the reason it cannot be rendered.
///
/// A template that has lost `{archive}` is **refused rather than rendered**, the
/// rule `fleetor_core::brief` applies to every other template — except that here
/// the refusal can only ever be the operator's own override, because
/// [`validate`] is what stopped one from being loaded in the first place.
pub fn render_brief(template: &str, archive: &Path) -> Result<String, String> {
    validate(template)?;
    Ok(fleetor_core::brief::render(template, &[(PLACEHOLDER, &archive.display().to_string())]))
}

/// Whether an operator's own `critic.md` can be used.
///
/// One structural check and no prose checks, deliberately. The two clauses that
/// do the work — the citation rule and the ban on prescription — are prose the
/// operator is explicitly allowed to rewrite (the spike's §8 names this as the
/// cost of shipping the brief in `prompts/` and says nothing should defend
/// against it). What cannot be rewritten away is the pane knowing which
/// directory to read, because that is not an opinion.
pub fn validate(template: &str) -> Result<(), String> {
    if template.contains(&format!("{{{PLACEHOLDER}}}")) {
        return Ok(());
    }
    Err(format!(
        "the template is missing its {{{PLACEHOLDER}}} placeholder — the Critic would be \
         rendered without the directory it is meant to read, and would spend its first turn \
         asking where the run is. Put {{{PLACEHOLDER}}} back, or delete the file to use the \
         built-in brief"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped brief renders, and what it renders is the directory it was
    /// handed.
    #[test]
    fn the_shipped_brief_renders_around_the_run_it_was_pointed_at() {
        let archive = Path::new("/tmp/fleetor-critic/2026-09-02T10-00-00Z");
        let rendered = render_brief(DEFAULT_BRIEF, archive).expect("the shipped brief is usable");
        assert!(rendered.contains(&archive.display().to_string()));
        assert!(!rendered.contains("{archive}"), "the placeholder survived rendering");
    }

    /// The one refusal, and it names what to fix.
    #[test]
    fn a_brief_with_no_archive_placeholder_is_refused_rather_than_rendered() {
        let why = render_brief("You are the Critic. Read the run.", Path::new("/tmp/x"))
            .expect_err("a template with nowhere to read is not usable");
        assert!(why.contains("{archive}"), "the complaint names what to fix: {why}");
        assert!(why.contains("built-in"), "and says what is running instead: {why}");
    }

    /// Its directories sit outside `_shell`, so no pane in the run being
    /// critiqued may write into them, and outside `pane-config` so its own
    /// transcript is neither copied into its working directory nor archived
    /// with the run.
    #[test]
    fn the_critics_directories_sit_outside_the_fleets_own() {
        let fleetor = PathBuf::from("/home/x/.fleetor");
        let shell = fleetor.join("_shell");
        for dir in
            [critic_dir(&fleetor), run_dir(&fleetor, "2026-09-02T10-00-00Z"), config_dir(&fleetor)]
        {
            assert!(!dir.starts_with(&shell), "{} is inside _shell", dir.display());
            assert!(dir.starts_with(&fleetor), "{} escapes ~/.fleetor", dir.display());
        }
    }

    /// **The six categories the spike measured, in the brief that shipped.**
    ///
    /// Five are the arc's (D10); the sixth was added during the spike and
    /// produced the largest finding on both measured runs while D10's third and
    /// fourth produced nothing on either. Pinned as a count and by its own
    /// sentence, so a future trim of the brief has to be deliberate.
    /// The brief lowercased with every run of whitespace collapsed to one space.
    ///
    /// It is hard-wrapped — it was taken from the spike note verbatim, wraps and
    /// all — so a sentence spanning two lines would otherwise defeat a plain
    /// `contains`. Collapsing is what lets these tests quote the brief's own
    /// sentences rather than fragments chosen to fit between line breaks.
    fn flat() -> String {
        DEFAULT_BRIEF.to_lowercase().split_whitespace().collect::<Vec<&str>>().join(" ")
    }

    #[test]
    fn the_brief_reports_on_the_six_categories_the_spike_measured() {
        let lowered = flat();
        assert!(lowered.contains("exactly these six categories"));
        for category in [
            "**idle time.**",
            "**blocks marked done whose check never ran.**",
            "**blocks posted with no performance criteria.**",
            "**two workers editing one file.**",
            "**messages that got no reply.**",
            "**anything else the fleet did that cost the operator.**",
        ] {
            assert!(lowered.contains(category), "the brief lost `{category}`");
        }
    }

    /// **The two clauses the spike says are doing the work** (§8): every finding
    /// carries a citation, and nothing is prescribed. The control run — the same
    /// archive with an unconstrained prompt — breached both, and its output was
    /// fluent, largely true and unusable.
    ///
    /// Pinned on the shipped file rather than on an operator's override, which
    /// is deliberately not defended: story 35 makes the brief theirs to edit.
    #[test]
    fn the_shipped_brief_keeps_the_citation_rule_and_the_ban_on_prescription() {
        let lowered = flat();
        assert!(lowered.contains("every finding carries a citation"));
        assert!(
            lowered.contains("a claim you cannot anchor this way is not a finding"),
            "the refusal, not merely the requirement",
        );
        assert!(lowered.contains("an absence is citable"), "or the absences go under UNCITED");
        assert!(lowered.contains("you also do not prescribe"));
        assert!(lowered.contains("you have no answer key"), "the remit, stated as a limit");
        assert!(
            lowered.contains("you never say whether the work was good"),
            "it judges the run, not the work (D10)",
        );
        // The sixth category is bounded by the same discipline, which is why the
        // spike found it did not reopen the mush door.
        assert!(lowered.contains("under exactly the same rules"));
        assert!(lowered.contains("not a shortcoming you inferred"));
    }

    /// **The in-remit / out-of-remit line** (WP-21 stage A).
    ///
    /// "Find mistakes and lapses of judgement" reads like a reversal of the ban
    /// on judging, and it is not: a decision about *the run* is in remit, a
    /// judgement about *the code* is out. The arc doc calls that distinction its
    /// most important sentence, so both worked examples — real findings from the
    /// first live run — are pinned, not just the abstract rule.
    #[test]
    fn the_brief_draws_the_line_between_a_decision_about_the_run_and_a_judgement_about_the_code() {
        let lowered = flat();
        assert!(
            lowered.contains("the line that paragraph draws runs between the run and the code"),
            "the distinction has to be stated, not implied by the examples",
        );
        assert!(lowered.contains("a decision about **the run** is yours to report"));
        assert!(lowered.contains("a judgement about **the code** is not"));
        assert!(
            lowered.contains(
                "*\"orch reported the run verified by receipts; no receipt existed\"* is in remit"
            ),
            "the in-remit worked example",
        );
        assert!(
            lowered.contains(
                "*\"the retry logic should have been extracted\"* is not"
            ),
            "the out-of-remit worked example",
        );
        assert!(
            lowered.contains("asking about mistakes does not widen the remit"),
            "the remit does not widen; only the evidence inside it does",
        );
    }

    /// **Testimony is not record** (WP-21 stage A).
    ///
    /// The citation rule's ban on paraphrase — *"you can cite what it wrote, not
    /// why it wrote it"* — would be quietly deleted by admitting an interview
    /// answer as archive evidence, because an answer is exactly a pane saying
    /// why. So the third citation form is visually distinct, corroboration is
    /// the most an answer can do, a contradiction is itself an archive-anchored
    /// finding, and an unsettled question still goes to UNCITED.
    #[test]
    fn the_brief_admits_testimony_as_corroboration_and_never_as_the_anchor() {
        let lowered = flat();
        assert!(lowered.contains("**testimony is not record.**"));
        assert!(
            lowered.contains("cite an answer as `interview <pane> <ts>` — never in the shape of \
                              an archive citation"),
            "a third citation form, distinct from `events.json seq n` and a transcript line",
        );
        assert!(
            lowered.contains(
                "a finding may be **corroborated** by testimony and is **never established** by it"
            ),
            "the whole of what an answer is worth",
        );
        assert!(
            lowered.contains("every finding keeps its archive anchor"),
            "and the anchor is the archive's, always",
        );
        assert!(
            lowered.contains(
                "**an answer that contradicts the archive is itself a finding**, and an \
                 archive-anchored one"
            ),
            "this is where a lapse of judgement about the run surfaces",
        );
        assert!(
            lowered.contains("goes to uncited, **with the answer recorded**"),
            "UNCITED survives the interview, and it records what asking produced",
        );
    }

    /// **How an interview is conducted** — the gate, and the two verb shapes.
    ///
    /// A brief that taught `fleet reply <pane> "…"` would teach a shape the CLI
    /// refuses at parse (D-077), so the no-recipient rule is pinned by its own
    /// sentence. The closed interview failing at resolution is pinned too: a
    /// Critic that read "no such pane" as a defect in the run would file the
    /// operator's own switch as a finding.
    #[test]
    fn the_brief_says_when_the_critic_may_ask_and_in_what_shape() {
        let lowered = flat();
        assert!(lowered.contains("when the operator opens an interview — and only then"));
        assert!(
            lowered.contains("a send fails with \"no such pane\"; that is the interview being \
                              closed, not a fault of the run and not a finding"),
            "the closed gate is resolution failure, and it is not a finding",
        );
        assert!(lowered.contains("address a pane by name with `fleet send orch \"<text>\"`"));
        assert!(
            lowered.contains("`fleet reply` takes **no recipient**"),
            "D-077: a name in front of the text is refused at parse",
        );
        assert!(
            lowered.contains("do not ask what should have been built"),
            "the remit binds the questions as well as the findings",
        );
    }

    /// The interview gets its own output section, beside the three the spike
    /// converged on, so a reader can tell at a glance which claims rest on the
    /// record and which on testimony.
    #[test]
    fn the_output_has_an_interview_section_beside_orientation_findings_and_uncited() {
        let lowered = flat();
        for section in ["- **orientation**", "- **findings**", "- **interview**", "- **uncited**"] {
            assert!(lowered.contains(section), "the output lost `{section}`");
        }
        assert!(
            lowered.contains("- **interview** — only if the operator opened one"),
            "no interview, no section to pad",
        );
        let findings = lowered.find("- **findings**").expect("FINDINGS is in the output");
        let interview = lowered.find("- **interview**").expect("INTERVIEW is in the output");
        let uncited = lowered.find("- **uncited**").expect("UNCITED is in the output");
        assert!(findings < interview && interview < uncited, "testimony reads after the record");
    }
}
