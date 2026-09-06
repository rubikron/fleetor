//! **The write guardrail** (WP-17) — a pre-tool-use hook, installed into every
//! pane's configuration directory at spawn, that refuses a tool call which would
//! write outside that pane's own roots.
//!
//! This module is the Rust half of what is the *fleet's*: where the roots come
//! from, the script and the interpreter, the command line the hook is invoked
//! with, and how a refusal reaches the Activity feed. The decision itself is
//! `write_guardrail.py`, baked in with `include_str!` for the reason the briefs
//! are (D-042): one reviewable artifact, and the binary always has a working
//! copy.
//!
//! **What is the vendor's is a [`Harness`] method** — `install_guardrail`, which
//! writes the settings document the hook is registered in (#31, C32). Claude
//! Code's is JSON, codex's is the same `config.toml` as the rest of its seed, and
//! that difference is why the installer could not stay one shared function
//! pretending to be general.
//!
//! **Writes only, and that is the operator's decision, not an omission.** Reads
//! stay open. A shell command a hook allows can read anything internally, so
//! read-blocking is friction wearing enforcement's clothes — and its allowlist
//! would have to cover every toolchain path, which wedges a pane in the way that
//! looks exactly like a healthy pane. `docs/roadmap/17-write-guardrail.md` §Scope
//! records the read denylist and the read+write allowlist as considered and
//! rejected.
//!
//! [`Harness`]: crate::placement::harness::Harness
//!
//! **The roots, per pane** (`roots_for`):
//!
//!  - the pane's **own cwd** — a worker's git worktree, `orch`'s target repo;
//!  - `~/.fleetor/_shell/` — the socket, the worktrees and the pane config dirs
//!    all live there and panes legitimately touch them;
//!  - anything the operator added with `[fence] allow` in `prompts/launch.conf`.
//!
//! …minus `_shell/pane-config/`, which is inside `_shell` and still refused: it
//! holds this guardrail's own rules, and no pane edits its own policy.
//!
//! **What this enforces and what it merely deters.** A structured edit names its
//! destination in the tool call, so those refusals are enforcement. A shell
//! command does not: the hook can only see paths the command
//! actually *names*, which is why `cargo build` writing into `~/.cargo` and a
//! worker's `git commit` writing objects into the target repo's `.git` both pass
//! — measured, in `docs/notes/write-guardrail-notes.md`. That is deliberate:
//! those two are exactly the writes a pane cannot work without, and a guardrail
//! that stopped them would wedge every worker on `fleet done`'s first step.
//!
//! **Tier 1.7 is narrowed here, not widened.** Workers spawn `--permission-mode
//! auto` scoped to their own worktree; this refuses a subset of what that
//! already allowed. §9.2's "ask before widening auto-approve" is not tripped by
//! making it smaller.
//!
//! **Tier 1.4: nothing here is on the message path.** A hook that could refuse a
//! *delivery* is the thing §9.3 records as argued and lost twice. This governs a
//! pane's own tool calls; the hub, the delivery loop, the pty registry, the wire
//! and the CLI neither import this module nor mention it, and
//! `src-tauri/tests/write_guardrail.rs` fails if that ever stops being true.

use std::path::{Path, PathBuf};

use fleetor_core::event::NoticeLevel;
use fleetor_core::pane::PaneId;

use crate::placement::harness::GuardrailInstall;

/// The decision, baked in. Written into each pane's config dir at spawn so the
/// hook command can name it by absolute path.
const HOOK_SCRIPT: &str = include_str!("write_guardrail.py");

/// What it is called on disk, inside the pane's config dir. The fleet's name for
/// the fleet's script, which is why it is a constant here and
/// [`GuardrailInstall::hook_file`] names it rather than repeating it.
pub const HOOK_FILE: &str = "write-guardrail.py";

/// The interpreter, by absolute path rather than by name: a pane's PATH is
/// curated (WP-08) and a hook that resolved `python3` differently from the
/// operator's shell would be a different program.
const INTERPRETER: &str = "/usr/bin/python3";

// The tools the hook is registered for used to be a `WRITE_TOOLS` constant here.
// The value is one vendor's tool names, so #23 moved it onto the spec and #31
// made it a **list** rather than a matcher string — because a matcher is the
// settings document's syntax and the tool names are what arrives in a payload,
// and codex is the harness that made the difference visible. `hook_command` below
// hands the list to the script on `--tool`, which is where `write_guardrail.py`'s
// own second copy of it went (#23's carry-forward). There is one spelling now.
// `HOOK_FILE` above stayed, because the script is the fleet's own and every
// harness gets the same one.

/// Where refusals accumulate for the Activity feed. One file for the whole
/// fleet, under `_shell` like everything else the running fleet owns.
pub fn journal_path(shell: &Path) -> PathBuf {
    shell.join("guardrail.jsonl")
}

/// The one directory inside the roots that is still off limits: every pane's
/// config dir, because that is where this guardrail's own rules live.
///
/// The name is [`crate::placement::pane_config_root`]'s, not a second spelling of
/// it (D-075): the directory this refuses writes to and the directory the layout
/// puts pane configs in are the same fact, and a rename that reached one and not
/// the other would leave every pane able to rewrite its own hook policy.
pub fn policy_dir(shell: &Path) -> PathBuf {
    crate::placement::pane_config_root(shell)
}

/// The roots this pane may write under.
///
/// `cwd` is the pane's own working directory as `spawn_pane` computed it — a
/// worker's worktree, or (in the shared-checkout fallback) the target itself,
/// which degrades the guardrail exactly the way it degrades peer review rather
/// than inventing a directory that is not there.
pub fn roots_for(cwd: &Path, shell: &Path, extra: &[String]) -> Vec<PathBuf> {
    let mut roots = vec![cwd.to_path_buf(), shell.to_path_buf()];
    roots.extend(extra.iter().map(PathBuf::from));
    roots
}

/// Everything installing one pane's guardrail is allowed to look at.
///
/// **A struct rather than five arguments, for [`crate::placement::harness::Seed`]'s
/// reason** (C39): checkpoint 7's installer is a [`Harness`] method now, so every
/// harness's body takes this and a field added later is one edit rather than one
/// per implementation.
///
/// **Tier 1.7 lives here and nowhere else.** [`roots`](Self::roots) is
/// `placement::guardrail_notices`' own computation from the pane's cwd, and no
/// field of [`GuardrailInstall`] can reach it — a harness supplies where its
/// refusal is registered, never what it refuses (C32).
///
/// [`Harness`]: crate::placement::harness::Harness
#[derive(Debug, Clone, Copy)]
pub struct GuardrailPlacement<'a> {
    /// **Whether this is the operator's own pane** — the same question
    /// [`Seed::operators_own_seat`] asks, one layer over, and deliberately the
    /// same field rather than a second seat concept (#46, C49, C50).
    ///
    /// A harness whose hook trust cannot be established per-hook may answer it by
    /// **owning the pane's whole hook table on a seat FLEETOR drives** and
    /// trusting the result — which is only sound because what is then trusted is
    /// the fleet's own artifact. On the operator's own seat their hooks are
    /// inherited untouched and nothing is trusted on their behalf.
    ///
    /// **`false` is the default answer** for the same reason it is on `Seed`: a
    /// seat nobody thought about is fenced rather than trusted.
    ///
    /// [`Seed::operators_own_seat`]: crate::placement::harness::Seed::operators_own_seat
    pub operators_own_seat: bool,
    /// Which pane this is, for the refusal text and the journal line.
    pub pane: PaneId,
    /// The pane's own configuration directory. The script and the settings file
    /// both land under here and nowhere else.
    pub config_dir: &'a Path,
    /// **The roots this pane may write under**, already computed. Tier 1.7's
    /// whole content, and deliberately not a harness's to supply.
    pub roots: &'a [PathBuf],
    /// The one directory inside the roots that is still refused: every pane's
    /// config dir, where this guardrail's own rules live.
    pub policy: &'a Path,
    /// Where a refusal is appended for the Activity feed.
    pub journal: &'a Path,
}

/// Write the hook script and build the command line that runs it — everything
/// checkpoint 7 does that is the *fleet's* rather than the vendor's.
///
/// **This is the half C32 said stays a free function.** The script, the
/// interpreter, the arguments and the journal are identical for every harness; a
/// harness's [`Harness::install_guardrail`] calls this, then writes the one thing
/// only it can write — its own settings document.
///
/// **A missing interpreter is loud and does not stop the pane.** A guardrail that
/// quietly does nothing is worse than no guardrail, because the operator believes
/// in it; a pane that refuses to spawn over a hook is worse than both. So the hook
/// is installed either way and the operator is told, in the same spirit as the
/// missing-`fleet`-binary warning.
///
/// [`Harness::install_guardrail`]: crate::placement::harness::Harness::install_guardrail
pub fn prepare(
    spec: &GuardrailInstall,
    at: &GuardrailPlacement<'_>,
) -> Result<(String, Vec<(NoticeLevel, String)>), String> {
    std::fs::create_dir_all(at.config_dir)
        .map_err(|e| format!("create config dir {}: {e}", at.config_dir.display()))?;

    let script = at.config_dir.join(spec.hook_file);
    // Overwritten on every spawn, unlike the gitconfig seed: this file ships
    // with the binary and a stale copy from an older build would be a guardrail
    // enforcing last version's rules.
    std::fs::write(&script, HOOK_SCRIPT)
        .map_err(|e| format!("write {}: {e}", script.display()))?;

    let command = hook_command(&script, spec, at);

    let mut notices = Vec::new();
    if !Path::new(INTERPRETER).exists() {
        notices.push((
            NoticeLevel::Error,
            format!(
                "{}: the write guardrail needs {INTERPRETER} and it is not there, so this \
                 pane is running with NO write guardrail — it can write anywhere you can. \
                 Install the macOS command line tools (`xcode-select --install`) and restart \
                 the fleet before pointing it at anything you care about.",
                at.pane,
            ),
        ));
    }
    Ok((command, notices))
}

/// Install `text` as this pane's settings document, through a temp file and a
/// rename.
///
/// The fleet's, not a harness's, and for the reason every other seeded document
/// goes down this way: a half-written settings file is a pane that boots without
/// its guardrail while looking perfectly healthy.
pub fn install_document(
    spec: &GuardrailInstall,
    config_dir: &Path,
    text: &str,
) -> Result<(), String> {
    let settings = config_dir.join(spec.settings_file);
    let tmp = config_dir.join(format!("{}.tmp", spec.settings_file));
    std::fs::write(&tmp, text).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &settings)
        .map_err(|e| format!("install {}: {e}", settings.display()))
}

/// This pane's existing settings document, if it has one.
pub fn existing_document(spec: &GuardrailInstall, config_dir: &Path) -> Option<String> {
    std::fs::read_to_string(config_dir.join(spec.settings_file)).ok()
}

/// Is this command string one of ours, from a previous launch?
///
/// Recognized by the hook file's name, which is the fleet's for every harness, so
/// a relaunch replaces our entry and leaves the operator's alone. Shared because
/// "which entry is FLEETOR's" is the fleet's question in every document format.
pub fn is_our_command(command: &str, spec: &GuardrailInstall) -> bool {
    command.contains(spec.hook_file)
}

/// The shell command the harness runs for every matched tool call.
///
/// Everything variable is an argument, so `write_guardrail.py` is byte-identical
/// in every pane and the *policy* is the command line — one place per pane where
/// what that pane may write is written down in full.
fn hook_command(script: &Path, spec: &GuardrailInstall, at: &GuardrailPlacement<'_>) -> String {
    let mut parts = vec![
        quote(INTERPRETER),
        quote(&script.to_string_lossy()),
        "--pane".into(),
        quote(&at.pane.to_string()),
    ];
    // **The tool names travel on the command line, and that is #23's carry-forward
    // closed.** The script used to bake its own `WRITE_TOOLS` set — a second,
    // independent spelling of `GuardrailInstall::write_tools` that could drift
    // from it silently, which is exactly the failure these tests were built to
    // catch, since they run the artifact a real placement installed. There is one
    // spelling now and this is where it is handed over.
    for tool in spec.write_tools {
        parts.push("--tool".into());
        parts.push(quote(tool));
    }
    // **The event name, so the script never spells a vendor's.** It goes back out
    // in the denial's `hookEventName`, and it is the spec's answer rather than a
    // literal in a Python file no Rust tripwire can scan.
    parts.push("--event".into());
    parts.push(quote(spec.hook_event));
    for root in at.roots {
        parts.push("--root".into());
        parts.push(quote(&root.to_string_lossy()));
    }
    parts.push("--deny".into());
    parts.push(quote(&at.policy.to_string_lossy()));
    parts.push("--journal".into());
    parts.push(quote(&at.journal.to_string_lossy()));
    parts.join(" ")
}

/// Single-quote for `sh`. Paths under `~/.fleetor` are ours, but the target repo
/// is the operator's and may contain anything a macOS path can.
fn quote(raw: &str) -> String {
    format!("'{}'", raw.replace('\'', r"'\''"))
}

// The JSON merge that used to live here — a top-level `hooks` object keyed by
// event, holding `{matcher, hooks:[{type:"command", command}]}` entries — was
// **Claude Code's settings document and nothing general**, which is precisely
// what C32 named as the gap and what this ticket closes. It moved into
// `placement::harness`'s own `install_guardrail`, beside the harness whose format
// it is, the way the document format moved to `seed_config_dir` for checkpoint 4
// (C16, C31). Codex's `config.toml` is TOML and its merge lives in
// `placement::codex`. What stayed here is what is the same in both: the script,
// the interpreter, the command line, the temp-file install and the journal.

// --- the Activity feed ---------------------------------------------------------

/// A cursor over the refusal journal.
///
/// The hook appends; this reads forward from where it last stopped. Polling a
/// file rather than watching it, and from a task of its own rather than from
/// anything on the message path — a refusal is at most a second late reaching
/// the feed, and nothing about delivery ever waits on it.
pub struct Journal {
    path: PathBuf,
    offset: u64,
}

impl Journal {
    /// Start a run's journal empty. Refusals from a previous run belong to that
    /// run's archived log, not to this one's feed.
    pub fn fresh(path: PathBuf) -> Self {
        let _ = std::fs::write(&path, "");
        Self { path, offset: 0 }
    }

    /// Every refusal appended since the last call, as feed notices.
    pub fn drain(&mut self) -> Vec<(NoticeLevel, String)> {
        let Ok(text) = std::fs::read_to_string(&self.path) else { return Vec::new() };
        let len = text.len() as u64;
        if len < self.offset {
            self.offset = 0; // truncated under us; start over rather than skip
        }
        let fresh = text[self.offset as usize..].to_string();
        // Only whole lines: a half-written record is the next call's business.
        let complete = fresh.rfind('\n').map(|i| i + 1).unwrap_or(0);
        self.offset += complete as u64;
        fresh[..complete].lines().filter_map(notice_for).collect()
    }
}

/// One journal line as a feed notice. `None` for anything unparseable — a pane
/// can write into this file (it is under `_shell`), and a malformed line must
/// not stop the real refusals behind it from being reported.
fn notice_for(line: &str) -> Option<(NoticeLevel, String)> {
    let record: serde_json::Value = serde_json::from_str(line).ok()?;
    let pane = record.get("pane")?.as_str()?;
    let tool = record.get("tool")?.as_str()?;
    let paths: Vec<&str> =
        record.get("paths")?.as_array()?.iter().filter_map(|p| p.as_str()).collect();

    if record.get("level").and_then(|l| l.as_str()) == Some("error") {
        let detail = record.get("detail").and_then(|d| d.as_str()).unwrap_or("no detail");
        return Some((
            NoticeLevel::Error,
            format!(
                "{pane}: the write guardrail failed and refused a tool call rather than letting \
                 it through unchecked ({detail}). That pane is stuck until this is fixed."
            ),
        ));
    }
    Some((
        NoticeLevel::Warn,
        format!(
            "{pane}: the write guardrail refused a {tool} write to {} — outside the roots that \
             pane may write to. The pane was told which path and why, and can carry on.",
            paths.join(", "),
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::placement::harness::Harness;

    /// Checkpoint 7 for the one registered harness, reached the way production
    /// reaches it — through the harness, not through a literal typed here.
    fn cc() -> &'static dyn Harness {
        crate::placement::harness::claude_code()
    }

    fn spec() -> &'static GuardrailInstall {
        &cc().spec().guardrail
    }

    /// Install through the harness method, which is the only way production
    /// installs anything now.
    fn install_at(
        dir: &Path,
        pane: PaneId,
        roots: &[PathBuf],
        shell: &Path,
    ) -> Vec<(NoticeLevel, String)> {
        cc().install_guardrail(&GuardrailPlacement {
            operators_own_seat: false,
            pane,
            config_dir: dir,
            roots,
            policy: &policy_dir(shell),
            journal: &journal_path(shell),
        })
        .unwrap()
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "fleetor-guardrail-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn settings(dir: &Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap()
    }

    fn installed_command(dir: &Path) -> String {
        settings(dir)["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .to_string()
    }

    /// The whole install, at the seam: the script is on disk and `settings.json`
    /// names it with this pane's own roots.
    #[test]
    fn installing_writes_the_script_and_a_hook_that_names_this_panes_roots() {
        let dir = temp_dir("install");
        let shell = PathBuf::from("/fleetor/_shell");
        let cwd = shell.join("worktrees/worker-1");
        let roots = roots_for(&cwd, &shell, &[]);

        install_at(&dir, PaneId::Worker(1), &roots, &shell);

        assert!(dir.join(HOOK_FILE).exists(), "the decision itself must be on disk");
        assert_eq!(
            std::fs::read_to_string(dir.join(HOOK_FILE)).unwrap(),
            HOOK_SCRIPT,
            "the installed copy is the one baked into this binary",
        );

        let command = installed_command(&dir);
        assert!(command.contains(INTERPRETER), "{command}");
        assert!(command.contains(HOOK_FILE), "{command}");
        assert!(command.contains("--pane 'worker-1'"), "{command}");
        assert!(command.contains("--root '/fleetor/_shell/worktrees/worker-1'"), "{command}");
        assert!(command.contains("--root '/fleetor/_shell'"), "{command}");
        assert!(command.contains("--deny '/fleetor/_shell/pane-config'"), "{command}");
        assert_eq!(
            settings(&dir)["hooks"]["PreToolUse"][0]["matcher"],
            serde_json::json!(spec().write_tools.join("|")),
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The per-pane half of the package: `orch` gets the target repo, a worker
    /// gets its own worktree, and neither gets the other's.
    #[test]
    fn orch_and_a_worker_get_different_roots_and_share_only_the_shell() {
        let shell = PathBuf::from("/fleetor/_shell");
        let target = PathBuf::from("/Users/me/code/thing");
        let worktree = shell.join("worktrees/worker-2");

        let orch = roots_for(&target, &shell, &[]);
        let worker = roots_for(&worktree, &shell, &[]);

        assert_eq!(orch, vec![target.clone(), shell.clone()]);
        assert_eq!(worker, vec![worktree.clone(), shell.clone()]);
        assert!(!worker.contains(&target), "a worker may not write in the target repo itself");
        assert!(!orch.contains(&worktree), "orch has no business in a worker's worktree");
    }

    /// The operator's extension, from `[fence] allow` in `launch.conf`, reaches
    /// the command line every pane's hook is invoked with.
    #[test]
    fn the_operators_extra_roots_reach_the_hook_command() {
        let dir = temp_dir("extra");
        let shell = PathBuf::from("/fleetor/_shell");
        let roots = roots_for(
            Path::new("/wt"),
            &shell,
            &["/Users/me/scratch".to_string(), "/Volumes/data".to_string()],
        );
        assert_eq!(roots.len(), 4);

        install_at(&dir, PaneId::Worker(3), &roots, &shell);
        let command = installed_command(&dir);
        assert!(command.contains("--root '/Users/me/scratch'"), "{command}");
        assert!(command.contains("--root '/Volumes/data'"), "{command}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// D-062 invites the operator to put their own files in `pane-config/orch/`,
    /// and their own hooks are the obvious thing to put there. Ours joins them
    /// rather than replacing the file.
    #[test]
    fn installing_keeps_the_operators_own_settings_and_their_own_hooks() {
        let dir = temp_dir("merge");
        std::fs::write(
            dir.join("settings.json"),
            r#"{"model":"opus","hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"/usr/bin/true"}]}],"Stop":[{"hooks":[]}]}}"#,
        )
        .unwrap();

        let shell = PathBuf::from("/fleetor/_shell");
        install_at(&dir, PaneId::Orch, &roots_for(Path::new("/target"), &shell, &[]), &shell);

        let config = settings(&dir);
        assert_eq!(config["model"], serde_json::json!("opus"), "an unrelated setting survived");
        assert!(config["hooks"]["Stop"].is_array(), "another event's hooks survived");
        let pre = config["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 2, "the operator's hook and ours: {pre:#?}");
        assert_eq!(pre[0]["hooks"][0]["command"], serde_json::json!("/usr/bin/true"));
        assert!(pre[1]["hooks"][0]["command"].as_str().unwrap().contains(HOOK_FILE));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A relaunch must not accumulate a copy of our hook per launch — every one
    /// of them would run, and the operator would read five identical refusals.
    #[test]
    fn installing_twice_leaves_exactly_one_of_our_hooks() {
        let dir = temp_dir("twice");
        let shell = PathBuf::from("/fleetor/_shell");
        let roots = roots_for(Path::new("/wt"), &shell, &[]);
        for _ in 0..3 {
            install_at(&dir, PaneId::Worker(1), &roots, &shell);
        }
        let pre = settings(&dir)["hooks"]["PreToolUse"].as_array().unwrap().len();
        assert_eq!(pre, 1, "three launches, one hook");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A settings file nobody can parse must not stop a pane from spawning, and
    /// must not leave it unguarded either.
    #[test]
    fn a_corrupt_settings_file_is_replaced_rather_than_fatal() {
        let dir = temp_dir("corrupt");
        std::fs::write(dir.join("settings.json"), "{not json").unwrap();
        let shell = PathBuf::from("/fleetor/_shell");
        install_at(&dir, PaneId::Worker(1), &roots_for(Path::new("/wt"), &shell, &[]), &shell);
        assert!(installed_command(&dir).contains(HOOK_FILE));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A path with a quote in it would otherwise end the hook command early and
    /// leave the rest of the roots as separate shell words.
    #[test]
    fn a_root_with_a_quote_in_it_survives_the_shell() {
        assert_eq!(quote("/Users/me/it's mine"), r"'/Users/me/it'\''s mine'");
        assert_eq!(quote("/plain/path"), "'/plain/path'");
    }

    // --- the feed ---------------------------------------------------------------

    /// The operator sees attempts. A silent denial is the wrong answer: a pane
    /// that cannot tell a guardrail from a broken path retries forever, and an
    /// operator who never learns it happened cannot fix the mission.
    #[test]
    fn a_refusal_becomes_a_notice_naming_the_pane_the_tool_and_the_path() {
        let dir = temp_dir("journal");
        let file = dir.join("guardrail.jsonl");
        let mut journal = Journal::fresh(file.clone());
        assert!(journal.drain().is_empty(), "an empty journal says nothing");

        std::fs::write(
            &file,
            "{\"ts\":1,\"pane\":\"worker-2\",\"tool\":\"Bash\",\"paths\":[\"/Users/me/notes\"],\"level\":\"warn\"}\n",
        )
        .unwrap();
        let notices = journal.drain();
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].0, NoticeLevel::Warn);
        assert!(notices[0].1.contains("worker-2"), "{}", notices[0].1);
        assert!(notices[0].1.contains("Bash"), "{}", notices[0].1);
        assert!(notices[0].1.contains("/Users/me/notes"), "{}", notices[0].1);

        assert!(journal.drain().is_empty(), "a drained line is not reported twice");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The guardrail failing is a different event from the guardrail working,
    /// and the operator has to be able to tell them apart: one is a pane being
    /// corrected, the other is a pane that cannot proceed.
    #[test]
    fn a_guardrail_of_its_own_failure_is_an_error_not_a_warning() {
        let dir = temp_dir("journal-error");
        let file = dir.join("guardrail.jsonl");
        let mut journal = Journal::fresh(file.clone());
        std::fs::write(
            &file,
            "{\"ts\":1,\"pane\":\"orch\",\"tool\":\"guardrail\",\"paths\":[],\"level\":\"error\",\"detail\":\"KeyError: x\"}\n",
        )
        .unwrap();

        let notices = journal.drain();
        assert_eq!(notices[0].0, NoticeLevel::Error);
        assert!(notices[0].1.contains("KeyError: x"), "{}", notices[0].1);
        assert!(notices[0].1.contains("stuck"), "the operator needs the consequence: {}", notices[0].1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The journal lives under `_shell`, which panes may write to. A line a pane
    /// mangled must not cost the operator the real refusals around it, and a
    /// half-written line must not be reported as a truncated one.
    #[test]
    fn a_mangled_or_half_written_line_does_not_swallow_the_real_ones() {
        let dir = temp_dir("journal-mangled");
        let file = dir.join("guardrail.jsonl");
        let mut journal = Journal::fresh(file.clone());

        std::fs::write(
            &file,
            "not json at all\n\
             {\"pane\":\"worker-1\",\"tool\":\"Write\",\"paths\":[\"/x\"],\"level\":\"warn\"}\n\
             {\"pane\":\"worker-1\",\"tool\":\"Bash\"",
        )
        .unwrap();

        let notices = journal.drain();
        assert_eq!(notices.len(), 1, "the good line, and only it: {notices:#?}");
        assert!(notices[0].1.contains("/x"));

        // The half-written record completes; it is reported once, now.
        std::fs::write(
            &file,
            "not json at all\n\
             {\"pane\":\"worker-1\",\"tool\":\"Write\",\"paths\":[\"/x\"],\"level\":\"warn\"}\n\
             {\"pane\":\"worker-1\",\"tool\":\"Bash\",\"paths\":[\"/y\"],\"level\":\"warn\"}\n",
        )
        .unwrap();
        let notices = journal.drain();
        assert_eq!(notices.len(), 1);
        assert!(notices[0].1.contains("/y"), "{}", notices[0].1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A run starts with an empty feed. Last run's refusals belong to last run's
    /// archived log (D-058), not to this one's Activity view.
    #[test]
    fn a_new_run_does_not_replay_the_previous_runs_refusals() {
        let dir = temp_dir("journal-fresh");
        let file = dir.join("guardrail.jsonl");
        std::fs::write(
            &file,
            "{\"pane\":\"worker-1\",\"tool\":\"Bash\",\"paths\":[\"/old\"],\"level\":\"warn\"}\n",
        )
        .unwrap();

        let mut journal = Journal::fresh(file);
        assert!(journal.drain().is_empty(), "a previous run's journal is not this run's feed");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// D-069, pinned here rather than only in `placement::spawn`: the fleet's toolchain
    /// homes are inside `_shell`, so a build's cache writes are already allowed
    /// and **no root was added for them**.
    ///
    /// WP-17's open question 1 came with an instruction — do not widen this
    /// allowlist to make a worker's `cargo build` work — and the reason it was
    /// never in danger of being needed is structural: `_shell` is the second root
    /// every pane gets. This test exists so that if someone later reaches for
    /// `[fence] allow` to "fix" a Rust build, they find out here that the
    /// directory was never refused and the problem is somewhere else.
    #[test]
    fn the_fleets_toolchain_homes_are_inside_a_pane_root_so_a_build_cache_write_is_never_refused() {
        let shell = PathBuf::from("/Users/operator/.fleetor/_shell");
        let roots = roots_for(Path::new("/Users/operator/.fleetor/_shell/worktrees/x/worker-1"), &shell, &[]);

        for dir in ["cargo", "rustup"] {
            let home = shell.join(dir);
            assert!(
                roots.iter().any(|r| home.starts_with(r)),
                "the fleet's {dir} home must already be under a root: {roots:?}",
            );
        }
        assert_eq!(roots.len(), 2, "D-069 added no root of its own: {roots:?}");

        // And the one carve-out still applies to config, not to the toolchain.
        assert_eq!(policy_dir(&shell), shell.join("pane-config"));
    }
}
