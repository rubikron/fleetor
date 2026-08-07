//! Build script. Two jobs: Tauri's own codegen, and — under the `devmode`
//! feature only — pulling the evaluator's brief in from a **separate repo**
//! (WP-15).
//!
//! ## Why the brief is not a file in this repo
//!
//! `prompts/*.md` are baked in with `include_str!` from paths inside this tree
//! (D-042), and that is right for every brief except one. The evaluator's brief
//! is the rubric `orch` is judged against, and it buys two properties by living
//! somewhere this repo cannot see:
//!
//!  - **`orch` cannot read it.** A fleet that has seen the rubric optimises for
//!    the rubric, and the run stops being evidence of how the fleet works.
//!  - **`orch` cannot *edit* it.** An improve run points a fleet at this repo's
//!    own source with workers auto-approving inside their worktrees. A grader
//!    that lived in that tree could be softened by the generation it judges, and
//!    "did the fleet improve, or did the grader get soft" would stop being
//!    answerable — WP-12's Case 5, the confounder nobody would catch.
//!
//! ## Why it fails the build rather than substituting anything
//!
//! A placeholder brief would produce a retro that looks like a retro and grades
//! nothing, which is the failure this whole arc exists to avoid. So a `devmode`
//! build with no brief, an empty brief, or a brief missing a placeholder it
//! needs **does not compile**, and the panic names the absolute path.

use std::path::PathBuf;

/// Where the evaluator's brief lives, relative to the operator's home. The
/// harness repo's own default (`fleetor-eval/README.md`).
const DEFAULT_HARNESS: &str = "harness/fleetor-eval";

/// Points the build at a different checkout of the harness repo. A build-time
/// knob for the operator running the build, not a runtime override: there is
/// deliberately no `~/.fleetor/prompts/` path for this brief, because a file on
/// disk is exactly what the separate repo exists to avoid (`prompts/README.md`).
const ENV_HARNESS: &str = "FLEETOR_EVAL_HARNESS";

/// The file itself, inside the harness repo.
const BRIEF_REL: &str = "evaluator/brief.md";

/// What the renderer fills in. A template that has lost one of these is refused
/// rather than rendered — the rule `fleetor-core::brief` already applies to
/// every other template, applied here at build time because there is no
/// operator sitting in front of a `Warn` notice for this one.
const REQUIRED_PLACEHOLDERS: [&str; 4] = ["mission", "run_dir", "mission_file", "answer_key_dir"];

/// The name the compiled-in copy is written under in `OUT_DIR`.
const BAKED: &str = "evaluator-brief.md";

fn main() {
    tauri_build::build();
    println!("cargo:rerun-if-env-changed={ENV_HARNESS}");
    if std::env::var_os("CARGO_FEATURE_DEVMODE").is_some() {
        bake_evaluator_brief();
    }
}

/// Copy the brief into `OUT_DIR` so `evaluator.rs` can `include_str!` it from a
/// path that exists at compile time regardless of where the harness is checked
/// out. The copy lives in `target/`, never in this repo's tree.
fn bake_evaluator_brief() {
    let harness = harness_root();
    let brief = harness.join(BRIEF_REL);
    println!("cargo:rerun-if-changed={}", brief.display());

    let text = std::fs::read_to_string(&brief).unwrap_or_else(|e| {
        panic!(
            "\n\n\
             the `devmode` feature needs the evaluator's brief, and it is not readable at\n\
               {}\n\
             ({e})\n\n\
             That file lives in a separate repo on purpose (WP-15): it is the rubric `orch` is\n\
             judged against, and nothing in FLEETOR's own tree may contain it. There is no\n\
             fallback and no placeholder — a build that substituted one would produce a retro\n\
             that grades nothing while looking like it graded something.\n\n\
             Fix it by checking out the harness repo there, or point this build somewhere else\n\
             with {ENV_HARNESS}=/path/to/fleetor-eval. To build without an evaluator at all,\n\
             drop `--features devmode` — that is the default and it is a complete app.\n",
            brief.display(),
        )
    });

    if text.trim().is_empty() {
        panic!("the evaluator's brief at {} is empty", brief.display());
    }
    for name in REQUIRED_PLACEHOLDERS {
        assert!(
            text.contains(&format!("{{{name}}}")),
            "\n\nthe evaluator's brief at {} has lost its {{{name}}} placeholder.\n\
             A brief rendered without it would send the evaluator at a path nobody chose,\n\
             so it is refused here rather than rendered — the same rule `fleetor-core::brief`\n\
             applies to every other template.\n",
            brief.display(),
        );
    }

    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR")).join(BAKED);
    std::fs::write(&out, text)
        .unwrap_or_else(|e| panic!("writing {} : {e}", out.display()));
}

/// `$FLEETOR_EVAL_HARNESS`, else `$HOME/harness/fleetor-eval`.
fn harness_root() -> PathBuf {
    if let Some(explicit) = std::env::var_os(ENV_HARNESS) {
        return PathBuf::from(explicit);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| {
        panic!("neither {ENV_HARNESS} nor HOME is set, so the harness repo cannot be located")
    });
    PathBuf::from(home).join(DEFAULT_HARNESS)
}
