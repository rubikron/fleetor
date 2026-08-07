//! The evaluator (WP-15) — a real `claude` in its own window that wakes when
//! `orch` calls `fleet handoff`, reads everything the run produced, and walks
//! `orch` back through its own decisions.
//!
//! This module is everything about it that is *not* a pty, a window or a
//! delivery: whether one may exist at all, what it is told, where it works, and
//! the one act that has to happen at the same instant it appears.
//!
//! ## Three conditions, all required, and they fail differently
//!
//!  1. **The `devmode` cargo feature.** Build-time. Without it there is no brief
//!     compiled in and [`BRIEF`] is `None`, so [`readiness`] can only ever
//!     answer [`Readiness::NotBuilt`]. A default build cannot reach a spawn.
//!  2. **Dev mode** (`dev::is_enabled()`, D-061). Runtime, and read fresh at
//!     wake rather than snapshotted, so the operator turning the mode off
//!     before a handoff means no evaluator for that handoff.
//!  3. **A rewind mission.** The brief is written around an answer key; a retro
//!     with no ground truth converges on congratulation, which is the exact
//!     failure the whole arc exists to prevent (D-060). So a run pointed at an
//!     ordinary repo produces **no evaluator and a `Warn` saying why**, rather
//!     than a grader with nothing to grade against.
//!
//! ## The veil lives in `PaneId`, not here
//!
//! `PaneId::is_fleet_member` is the one predicate; this module never re-derives
//! it. What this module holds is the other half of WP-12's hiding model: the
//! brief is compiled in from a separate repo (`build.rs`), **and there is no
//! `~/.fleetor/prompts/` override for it** — the asymmetry that makes "the fleet
//! cannot influence the grader" structural rather than policed. See
//! [`OVERRIDE_REFUSED`].

use std::path::{Path, PathBuf};

/// The evaluator's brief, compiled in from the harness repo under `devmode`.
///
/// `None` in a default build, and that is the whole of "outside dev mode the
/// evaluator does not exist": there is nothing to tell it, so nothing spawns.
#[cfg(feature = "devmode")]
pub const BRIEF: Option<&str> = Some(include_str!(concat!(env!("OUT_DIR"), "/evaluator-brief.md")));
#[cfg(not(feature = "devmode"))]
pub const BRIEF: Option<&str> = None;

/// **There is no operator override for this brief, and that is the point.**
///
/// Every other template in this app can be replaced by dropping a file in
/// `~/.fleetor/prompts/` (D-042). This one cannot, because an override path
/// would put the rubric on disk in a directory every pane can read by absolute
/// path — giving back both properties the separate repo exists to buy. Named as
/// a constant so `prompts/README.md`'s claim and this module cannot drift, and
/// so the exclusion is a thing in the source rather than a thing nobody did.
pub const OVERRIDE_REFUSED: &str = "evaluator";

/// The placeholders the brief carries, filled at render time. Kept in step with
/// `build.rs`'s own list, which refuses a template that has lost one.
const PLACEHOLDERS: [&str; 4] = ["mission", "run_dir", "mission_file", "answer_key_dir"];

/// Where the rewind harness is checked out. Mirrors `build.rs`, because the
/// brief and the reveal script are two files in the same repo and a build that
/// took its brief from one checkout while shelling out to another would grade
/// against an answer key the rubric never described.
const ENV_HARNESS: &str = "FLEETOR_EVAL_HARNESS";
const DEFAULT_HARNESS: &str = "harness/fleetor-eval";

/// The two roots `fleetor-eval/lib/common.sh` reads, spelled the same way here
/// so the app and the harness cannot disagree about where a mission lives.
const ENV_WORKSPACES: &str = "FLEETOR_EVAL_WORKSPACES";
const DEFAULT_WORKSPACES: &str = ".fleetor-eval/workspaces";
const ENV_ANSWERS: &str = "FLEETOR_EVAL_ANSWERS";
const DEFAULT_ANSWERS: &str = ".fleetor-eval-answers";

/// The script that puts the answer key on disk, run once at wake.
const REVEAL_SCRIPT: &str = "reveal-answer-key.sh";

/// The window this pane is rendered in. Not in `tauri.conf.json`'s `windows`
/// array on purpose: a config-declared window exists at every launch, which is
/// precisely what "outside dev mode the evaluator does not exist" forbids.
pub const WINDOW_LABEL: &str = "evaluator";

// --- may there be one? ---------------------------------------------------------

/// Why there is or is not an evaluator for this run. Every arm but
/// [`Readiness::Ready`] is a reason, because a wake that silently does nothing
/// is indistinguishable from a wake that is broken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Readiness {
    Ready(Mission),
    /// No `devmode` feature: this binary has no grader in it at all.
    NotBuilt,
    /// Dev mode is off. The ordinary case, and deliberately silent.
    ModeOff,
    /// Dev mode is on and the fleet is not on a rewind mission.
    NoMission { target: PathBuf, workspaces: PathBuf },
}

/// What the fleet is working on, resolved from the target it was pointed at.
///
/// **Derived, never configured.** The operator points FLEETOR at
/// `<workspaces>/<mission>/repo` and that path already names the mission; a
/// second place to write it down is a second place for it to be wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mission {
    pub name: String,
    /// `fleetor-eval/missions/<name>.md` — where the judging weights live.
    pub file: PathBuf,
    /// Where [`reveal_answer_key`] puts upstream's real commit.
    pub answer_key_dir: PathBuf,
    /// The harness repo, for the reveal script.
    pub harness: PathBuf,
}

/// Whether this run gets an evaluator, and if not, why not.
///
/// `dev_enabled` is passed in rather than read here so the caller owns the one
/// read (`dev::is_enabled`) and this function stays testable without touching
/// the operator's real `config.json`.
pub fn readiness(dev_enabled: bool, target: &Path) -> Readiness {
    if BRIEF.is_none() {
        return Readiness::NotBuilt;
    }
    if !dev_enabled {
        return Readiness::ModeOff;
    }
    let workspaces = workspaces_root();
    match mission_for(target, &workspaces, &harness_root(), &answers_root()) {
        Some(mission) => Readiness::Ready(mission),
        None => Readiness::NoMission { target: target.to_path_buf(), workspaces },
    }
}

/// The mission a target belongs to, or `None` if it is not a prepared workspace.
///
/// The shape `prepare-mission.sh` builds is `<workspaces>/<name>/repo`, so the
/// test is "my parent's parent is the workspace root", and the name is the
/// directory between them. Deliberately strict: a target that merely *looks*
/// like a mission would produce a brief pointing at a mission file that does
/// not exist, and the evaluator would spend its first turn asking about it.
pub fn mission_for(
    target: &Path,
    workspaces: &Path,
    harness: &Path,
    answers: &Path,
) -> Option<Mission> {
    let workspace = target.parent()?;
    if workspace.parent()? != workspaces {
        return None;
    }
    let name = workspace.file_name()?.to_str()?.to_string();
    let file = harness.join("missions").join(format!("{name}.md"));
    if !file.is_file() {
        return None;
    }
    Some(Mission {
        answer_key_dir: answers.join(&name),
        harness: harness.to_path_buf(),
        name,
        file,
    })
}

// --- what it is told -----------------------------------------------------------

/// The evaluator's whole system prompt, or the reason it cannot be rendered.
///
/// A template that has lost a placeholder is **refused rather than rendered**,
/// the rule `fleetor-core::brief` applies to every other template. `build.rs`
/// checks the same list at compile time, so this is the belt to that's braces —
/// it exists because the check that runs is worth more than the check that was
/// meant to have run.
pub fn render_brief(mission: &Mission, run_dir: &Path) -> Result<String, String> {
    let template = BRIEF.ok_or("this build has no evaluator brief compiled in")?;
    let vars: Vec<(&str, String)> = vec![
        ("mission", mission.name.clone()),
        ("run_dir", run_dir.display().to_string()),
        ("mission_file", mission.file.display().to_string()),
        ("answer_key_dir", mission.answer_key_dir.display().to_string()),
    ];
    for (name, _) in &vars {
        if !template.contains(&format!("{{{name}}}")) {
            return Err(format!(
                "the evaluator's brief has lost its {{{name}}} placeholder — \
                 it would be rendered pointing at a path nobody chose"
            ));
        }
    }
    let borrowed: Vec<(&str, &str)> = vars.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let rendered = fleetor_core::brief::render(template, &borrowed);
    // The brief tells the evaluator to ask the operator about any path that
    // arrives still in braces. That is the right degradation for a placeholder
    // this app does not know about; it is the wrong one for the four it does,
    // which is why they are checked above rather than left to be noticed.
    debug_assert!(PLACEHOLDERS.iter().all(|n| !rendered.contains(&format!("{{{n}}}"))));
    Ok(rendered)
}

// --- where it works ------------------------------------------------------------

/// The dev-mode root: `~/.fleetor/dev/`.
///
/// **A sibling of `_shell/`, not a child, and that is load-bearing.** Every
/// pane's write guardrail has `_shell` as a root (D-065), so a retro snapshot
/// under `_shell` would be writable by every worker in the fleet being judged.
pub fn dev_dir(fleetor: &Path) -> PathBuf {
    fleetor.join("dev")
}

/// Where the run being evaluated is laid out for reading, and the evaluator's
/// own working directory: `~/.fleetor/dev/retro/<run-id>/`.
pub fn retro_dir(fleetor: &Path, run_id: &str) -> PathBuf {
    dev_dir(fleetor).join("retro").join(run_id)
}

/// The evaluator's `CLAUDE_CONFIG_DIR`.
///
/// **Deliberately not under `_shell/pane-config/`**, which is where every fleet
/// pane's lives (D-062). `runs::harvest_transcripts` walks that directory and
/// moves what it finds into `runs/<id>/transcripts/`, so an evaluator housed
/// there would file its own session — its reasoning, its rubric applied, its
/// verdict — into the archive that the *next* generation's fleet can read. That
/// is "past retros", one of the three things WP-12 requires to be absent.
pub fn config_dir(fleetor: &Path) -> PathBuf {
    dev_dir(fleetor).join("evaluator-config")
}

// --- the answer key ------------------------------------------------------------

/// Put upstream's real commit on disk. **This is the other half of the double
/// line** (WP-12): the evaluator becoming visible to `orch` and the answer key
/// landing are one instant, and this is the second of them.
///
/// Run before the pane is spawned, because the brief has the evaluator seal a
/// written verdict against the key *before* it speaks to `orch` — an answer key
/// that arrived mid-conversation would be read after the evaluator had already
/// formed its position from the fleet's own account.
///
/// An existing key is success, not an error: `reveal-answer-key.sh` refuses to
/// overwrite without `--force`, and a second `fleet handoff` in one run must not
/// turn that refusal into a failed wake.
pub fn reveal_answer_key(mission: &Mission) -> Result<Option<String>, String> {
    if mission.answer_key_dir.is_dir() {
        return Ok(None);
    }
    let script = mission.harness.join(REVEAL_SCRIPT);
    if !script.is_file() {
        return Err(format!(
            "the answer key cannot be fetched: {} is not there. The evaluator will start \
             without it and say so.",
            script.display()
        ));
    }
    let out = std::process::Command::new(&script)
        .arg(&mission.name)
        .current_dir(&mission.harness)
        .output()
        .map_err(|e| format!("running {}: {e}", script.display()))?;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "{} {} failed ({}): {}",
            script.display(),
            mission.name,
            out.status,
            why.trim().lines().last().unwrap_or("no output")
        ));
    }
    Ok(Some(format!("the answer key for {} is on disk", mission.name)))
}

// --- roots ---------------------------------------------------------------------

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
}

fn harness_root() -> PathBuf {
    std::env::var_os(ENV_HARNESS).map(PathBuf::from).unwrap_or_else(|| home().join(DEFAULT_HARNESS))
}

fn workspaces_root() -> PathBuf {
    std::env::var_os(ENV_WORKSPACES)
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(DEFAULT_WORKSPACES))
}

fn answers_root() -> PathBuf {
    std::env::var_os(ENV_ANSWERS).map(PathBuf::from).unwrap_or_else(|| home().join(DEFAULT_ANSWERS))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "fleetor-eval-{tag}-{}-{}",
            std::process::id(),
            fleetor_core::ids::new_id("t")
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A prepared workspace, exactly the shape `prepare-mission.sh` builds.
    fn mission_fixture(name: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
        let base = scratch("mission");
        let workspaces = base.join("workspaces");
        let repo = workspaces.join(name).join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let harness = base.join("harness");
        std::fs::create_dir_all(harness.join("missions")).unwrap();
        std::fs::write(harness.join("missions").join(format!("{name}.md")), "# mission").unwrap();
        let answers = base.join("answers");
        (repo, workspaces, harness, answers)
    }

    #[test]
    fn a_prepared_workspace_names_its_own_mission() {
        let (repo, workspaces, harness, answers) = mission_fixture("hyperfine-conclude");
        let mission = mission_for(&repo, &workspaces, &harness, &answers).expect("a mission");
        assert_eq!(mission.name, "hyperfine-conclude");
        assert_eq!(mission.file, harness.join("missions/hyperfine-conclude.md"));
        assert_eq!(mission.answer_key_dir, answers.join("hyperfine-conclude"));
    }

    /// An ordinary repo is not a mission, and must not be guessed into one — a
    /// retro with no answer key is the congratulation D-060 exists to prevent.
    #[test]
    fn an_ordinary_target_is_not_a_mission() {
        let (_, workspaces, harness, answers) = mission_fixture("hyperfine-conclude");
        let ordinary = scratch("ordinary").join("my-project");
        std::fs::create_dir_all(&ordinary).unwrap();
        assert_eq!(mission_for(&ordinary, &workspaces, &harness, &answers), None);
    }

    /// A workspace whose mission file was never written is refused too: the
    /// brief cites `{mission_file}` five times, and pointing it at a path that
    /// is not there costs the evaluator its first turn.
    #[test]
    fn a_workspace_with_no_mission_file_is_refused() {
        let (repo, workspaces, harness, answers) = mission_fixture("hyperfine-conclude");
        std::fs::remove_file(harness.join("missions/hyperfine-conclude.md")).unwrap();
        assert_eq!(mission_for(&repo, &workspaces, &harness, &answers), None);
    }

    /// The right shape at the wrong depth. `<workspaces>/<name>` without the
    /// `repo` level would make the *workspace* the mission name's parent.
    #[test]
    fn the_workspace_layout_is_matched_exactly_not_loosely() {
        let (repo, workspaces, harness, answers) = mission_fixture("hyperfine-conclude");
        let too_shallow = repo.parent().unwrap();
        assert_eq!(mission_for(too_shallow, &workspaces, &harness, &answers), None);
        let too_deep = repo.join("src");
        std::fs::create_dir_all(&too_deep).unwrap();
        assert_eq!(mission_for(&too_deep, &workspaces, &harness, &answers), None);
    }

    /// **The default build has no evaluator, and this is where that is true.**
    /// Every other refusal is downstream of this one.
    #[test]
    #[cfg(not(feature = "devmode"))]
    fn a_default_build_carries_no_brief_and_can_never_be_ready() {
        assert!(BRIEF.is_none(), "a default build must contain no evaluator prose");
        let (repo, _, _, _) = mission_fixture("hyperfine-conclude");
        assert_eq!(readiness(true, &repo), Readiness::NotBuilt, "even with the mode on");
    }

    #[test]
    #[cfg(feature = "devmode")]
    fn a_devmode_build_carries_a_brief_with_every_placeholder() {
        let template = BRIEF.expect("devmode compiles the brief in");
        for name in PLACEHOLDERS {
            assert!(template.contains(&format!("{{{name}}}")), "brief lost {{{name}}}");
        }
    }

    /// The mode is still a gate in a `devmode` build — the feature says this
    /// binary *has* a grader, the mode says the operator wants one now.
    #[test]
    fn the_mode_is_a_gate_even_when_the_brief_is_compiled_in() {
        let (repo, _, _, _) = mission_fixture("hyperfine-conclude");
        let off = readiness(false, &repo);
        assert!(
            matches!(off, Readiness::ModeOff | Readiness::NotBuilt),
            "the mode being off is never Ready: {off:?}"
        );
    }

    #[test]
    #[cfg(feature = "devmode")]
    fn a_rendered_brief_has_no_braces_left_from_the_four_it_fills() {
        let (repo, workspaces, harness, answers) = mission_fixture("hyperfine-conclude");
        let mission = mission_for(&repo, &workspaces, &harness, &answers).unwrap();
        let run_dir = PathBuf::from("/tmp/fleetor-retro/2026-08-07T14-32-05Z");
        let rendered = render_brief(&mission, &run_dir).unwrap();
        for name in PLACEHOLDERS {
            assert!(!rendered.contains(&format!("{{{name}}}")), "{{{name}}} survived rendering");
        }
        assert!(rendered.contains("hyperfine-conclude"));
        assert!(rendered.contains(&run_dir.display().to_string()));
    }

    /// The three directories are outside `_shell`, and the config dir is
    /// outside `pane-config` — the two placements that keep the evaluator's own
    /// reasoning out of the archive the next generation reads.
    #[test]
    fn the_evaluators_directories_sit_outside_the_fleets_own() {
        let fleetor = PathBuf::from("/home/x/.fleetor");
        let shell = fleetor.join("_shell");
        for dir in
            [dev_dir(&fleetor), retro_dir(&fleetor, "2026-08-07T14-32-05Z"), config_dir(&fleetor)]
        {
            assert!(!dir.starts_with(&shell), "{} is inside _shell", dir.display());
            assert!(dir.starts_with(&fleetor), "{} escapes ~/.fleetor", dir.display());
        }
        assert!(!config_dir(&fleetor).starts_with(shell.join("pane-config")));
    }

    /// An answer key that is already there is not a failure. `orch` may hand
    /// back twice (D-064 refuses to argue with that), and the reveal script
    /// refuses to overwrite without `--force`.
    #[test]
    fn an_answer_key_that_already_exists_is_left_alone() {
        let (repo, workspaces, harness, answers) = mission_fixture("hyperfine-conclude");
        let mission = mission_for(&repo, &workspaces, &harness, &answers).unwrap();
        std::fs::create_dir_all(&mission.answer_key_dir).unwrap();
        assert_eq!(reveal_answer_key(&mission), Ok(None), "no second fetch, and no error");
    }

    /// A missing reveal script is a reason the operator can read, not a panic
    /// and not a silent skip — the evaluator still wakes and says it has no key.
    #[test]
    fn a_missing_reveal_script_is_a_sentence_rather_than_a_failure_to_wake() {
        let (repo, workspaces, harness, answers) = mission_fixture("hyperfine-conclude");
        let mission = mission_for(&repo, &workspaces, &harness, &answers).unwrap();
        let err = reveal_answer_key(&mission).unwrap_err();
        assert!(err.contains(REVEAL_SCRIPT), "the reason names the script: {err}");
    }
}
