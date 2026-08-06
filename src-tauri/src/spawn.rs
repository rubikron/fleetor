//! How a pane's `claude` is launched (D-030, Phase 3).
//!
//! Two commands, deliberately different postures:
//!
//!  - [`orch_command`] — the operator's **own** `claude`. Full environment
//!    inherit, their login, their Opus, their config dir. We override only what
//!    the fleet needs, and ours must win, so every override lands *after* the
//!    inherit.
//!  - [`worker_command`] — an isolated Flash worker: its own `CLAUDE_CONFIG_DIR`,
//!    its own worktree, `--permission-mode auto`, and the DeepSeek endpoint.
//!
//! Everything here that looks like a detail was measured in Phase 0
//! (`docs/tui-spawn-notes.md`) against a real interactive `claude`. The three that
//! each silently wedge a pane forever:
//!
//!  - **[`seed_config_dir`] is not optional (L1).** A virgin config dir does not
//!    "sometimes" hit onboarding — it never reaches a prompt at all, and every
//!    `fleet send` into it reports success into a theme picker.
//!  - **`ANTHROPIC_API_KEY` must not be set (L2).** Headless didn't care;
//!    interactive `claude` asks "use this API key?" and never reaches the input
//!    box. It is `env_remove`d rather than merely not-set, because the worker
//!    inherits the operator's environment and they may well have one.
//!  - **`--permission-mode auto`.** The default is `manual`, which wedges on the
//!    first tool call. It is `prompts/launch.conf`'s `permission_mode`, which
//!    ships as `auto` and is documented there with that consequence attached.
//!
//! What a pane is *told* is not here — it is in `prompts/`, resolved once at
//! bootstrap by [`crate::prompts`] and handed in as a [`PaneContext`]. This module
//! decides how a process is shaped; that one decides what goes in its head. The
//! two `env_remove` calls below are the deliberate exception: they are not
//! settings, they are the three ways a pane wedges forever (D-042).

use std::path::{Path, PathBuf};

use fleetor_core::brief::{render_orch, render_worker};
use fleetor_core::pane::{PaneId, WORKER_SLOTS};
use portable_pty::CommandBuilder;

use crate::prompts::PaneContext;

/// Overrides the program every pane runs. Set by `src-tauri/tests/panes.rs` to
/// `tests/fake-pane/fake-pane.sh` so the registry is exercised end-to-end without
/// spending a token. When set, the `claude` flags are dropped — a stand-in is not
/// obliged to understand them.
const ENV_PANE_CMD: &str = "FLEETOR_PANE_CMD";
/// First rung of the `fleet` binary ladder (L4).
const ENV_FLEET_BIN: &str = "FLEETOR_FLEET_BIN";

/// The full pane roster the briefs describe. Every pane is told about every other
/// one, whether or not it has been spawned yet — a brief is written once at spawn
/// and the fleet fills in around it.
fn roster() -> Vec<PaneId> {
    PaneId::roster(&WORKER_SLOTS)
}

// --- the orchestrator ---------------------------------------------------------

/// The operator's `claude`, as the fleet orchestrator, in `cwd`.
///
/// `CommandBuilder::new` already seeds the parent environment, so this is an
/// inherit-then-override — their login, their config dir, their model, plus the
/// five things that make it a pane.
pub fn orch_command(cwd: &Path, socket: &Path, ctx: &PaneContext) -> CommandBuilder {
    let mut cmd = base_command(&[
        "--append-system-prompt".to_string(),
        render_orch(&ctx.orch_template, &roster()),
    ]);
    cmd.cwd(cwd);
    apply_pane_env(&mut cmd, PaneId::Orch, socket);
    cmd
}

// --- the workers --------------------------------------------------------------

/// One worker pane: isolated config dir, its own worktree, Flash on DeepSeek.
///
/// `config_dir` must already have been through [`seed_config_dir`] for this exact
/// `cwd` — the trust flag is keyed by absolute project path, so a worker pointed
/// at a new target with an old seed sits on a trust dialog while `fleet send`
/// reports success (L1).
pub fn worker_command(
    slot: u8,
    cwd: &Path,
    config_dir: &Path,
    socket: &Path,
    api_key: &str,
    ctx: &PaneContext,
) -> CommandBuilder {
    let pane = PaneId::Worker(slot);
    let mut cmd = base_command(&[
        "--permission-mode".to_string(),
        ctx.launch.worker_permission_mode.clone(),
        "--append-system-prompt".to_string(),
        render_worker(&ctx.worker_template, pane, &roster()),
    ]);
    cmd.cwd(cwd);
    apply_pane_env(&mut cmd, pane, socket);

    cmd.env("CLAUDE_CONFIG_DIR", config_dir);
    cmd.env("ANTHROPIC_BASE_URL", &ctx.launch.worker_base_url);
    cmd.env("ANTHROPIC_AUTH_TOKEN", api_key);
    cmd.env("ANTHROPIC_MODEL", &ctx.launch.worker_model);
    // Not "don't set it" — *unset* it. The worker inherits the operator's
    // environment, and an `ANTHROPIC_API_KEY` sitting in their shell profile is
    // enough to park the pane on an api-key approval prompt forever (L2).
    cmd.env_remove("ANTHROPIC_API_KEY");
    cmd.env_remove("ANTHROPIC_DEFAULT_OPUS_MODEL");
    cmd.env_remove("ANTHROPIC_DEFAULT_SONNET_MODEL");
    cmd
}

// --- shared -------------------------------------------------------------------

/// The program plus its arguments, honoring the test override.
fn base_command(args: &[String]) -> CommandBuilder {
    if let Some(stand_in) = std::env::var(ENV_PANE_CMD).ok().filter(|s| !s.trim().is_empty()) {
        return CommandBuilder::new(stand_in);
    }
    let mut cmd = CommandBuilder::new("claude");
    for arg in args {
        cmd.arg(arg);
    }
    cmd
}

/// What makes any process a pane: a truecolor terminal, a PATH that can find both
/// `claude` and `fleet`, its own name, and the socket to reach the fleet on.
fn apply_pane_env(cmd: &mut CommandBuilder, pane: PaneId, socket: &Path) {
    cmd.env("PATH", augmented_path());
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("FLEETOR_PANE", pane.to_string());
    cmd.env("FLEET_SOCKET", socket);
    // Phase 0 saw this on every spike run: launching the shell from inside a
    // `claude` session leaks the marker through the environment inherit and
    // silently disables transcript saving in every pane below it.
    cmd.env_remove("CLAUDE_CODE_CHILD_SESSION");
}

/// A PATH that finds `claude` and `fleet` even when the app was launched from a
/// GUI context whose environment never saw the login shell's additions.
pub fn augmented_path() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let existing = std::env::var("PATH").unwrap_or_default();
    let mut prefix = String::new();
    if let Some(dir) = fleet_bin_path().and_then(|p| p.parent().map(Path::to_path_buf)) {
        prefix.push_str(&dir.to_string_lossy());
        prefix.push(':');
    }
    format!("{prefix}{home}/.local/bin:{home}/.bun/bin:/opt/homebrew/bin:/usr/local/bin:{existing}")
}

/// Where the `fleet` binary is, if it exists — an explicit ladder, checked for
/// existence at every rung.
///
/// The shim this replaces resolved its binary through a **hand-made symlink**
/// under `src-tauri/target/`, untracked and reproducible by nothing, behind a doc
/// comment claiming cargo put it there. It did not: these are two separate cargo
/// workspaces with two target directories (L4). So: no symlinks, no guessing, and
/// a caller that can tell the operator when the answer is "nowhere".
pub fn fleet_bin_path() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os(ENV_FLEET_BIN).map(PathBuf::from) {
        return explicit.is_file().then_some(explicit);
    }
    let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
    let mut candidates: Vec<PathBuf> = Vec::new();
    // Bundled: `fleet` ships next to the shell binary.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("fleet"));
        }
    }
    // `tauri dev` runs with cwd = `src-tauri/`; a bare `cargo run` from the repo
    // root does not. Both root-workspace target dirs, checked explicitly.
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("..").join("target").join(profile).join("fleet"));
        candidates.push(cwd.join("target").join(profile).join("fleet"));
    }
    candidates.into_iter().find(|p| p.is_file())
}

// --- the config seed (L1) -----------------------------------------------------

/// Give `dir` the two keys an interactive `claude` needs to reach its prompt in
/// `cwd`, without touching anything else that is already there.
///
/// **Merge, never clobber.** The dir may hold a real `machineID`, cached
/// experiment data, and — after the operator switches targets — trust flags for
/// other project paths. Overwriting the file would work exactly once.
///
/// **Re-run this for every pane cwd whenever the target changes.**
/// `hasTrustDialogAccepted` is keyed by absolute project path, not global; Phase 0
/// bisected this. It is the single most likely way to reintroduce L1 immediately
/// after fixing it.
pub fn seed_config_dir(dir: &Path, cwd: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("create config dir {}: {e}", dir.display()))?;
    let file = dir.join(".claude.json");

    let mut root = match std::fs::read_to_string(&file) {
        Ok(text) => serde_json::from_str::<serde_json::Value>(&text).unwrap_or_else(|_| json_object()),
        Err(_) => json_object(),
    };
    if !root.is_object() {
        root = json_object();
    }

    let object = root.as_object_mut().expect("just ensured it is an object");
    object.insert("hasCompletedOnboarding".into(), true.into());

    let project = project_key(cwd);
    let projects = object
        .entry("projects")
        .or_insert_with(json_object)
        .as_object_mut()
        .ok_or("existing \"projects\" in .claude.json is not an object")?;
    let entry = projects
        .entry(project)
        .or_insert_with(json_object)
        .as_object_mut()
        .ok_or("existing project entry in .claude.json is not an object")?;
    entry.insert("hasTrustDialogAccepted".into(), true.into());
    entry.insert("hasCompletedProjectOnboarding".into(), true.into());

    // Write via a sibling temp file: a half-written `.claude.json` is a pane that
    // boots into onboarding, which is the failure this whole function exists to
    // prevent.
    let tmp = file.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(&root).map_err(|e| format!("encode config: {e}"))?;
    std::fs::write(&tmp, text).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &file).map_err(|e| format!("install {}: {e}", file.display()))
}

/// The key `claude` will look itself up under: its resolved working directory.
/// `canonicalize` matters on macOS, where `/tmp` and `/var` are symlinks and a
/// child's own `cwd` comes back resolved — an unresolved key would never match.
pub fn project_key(cwd: &Path) -> String {
    std::fs::canonicalize(cwd)
        .unwrap_or_else(|_| cwd.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn json_object() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("fleetor-spawn-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn read_config(dir: &Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(dir.join(".claude.json")).unwrap()).unwrap()
    }

    /// The exact two keys Phase 0 bisected to, on a dir that had nothing.
    #[test]
    fn a_virgin_config_dir_gets_the_two_keys_that_clear_onboarding() {
        let dir = temp_dir("virgin");
        let cwd = temp_dir("virgin-cwd");
        seed_config_dir(&dir, &cwd).unwrap();

        let config = read_config(&dir);
        assert_eq!(config["hasCompletedOnboarding"], serde_json::json!(true));
        let project = &config["projects"][project_key(&cwd)];
        assert_eq!(project["hasTrustDialogAccepted"], serde_json::json!(true));
        assert_eq!(project["hasCompletedProjectOnboarding"], serde_json::json!(true));

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&cwd);
    }

    /// Merge, never clobber: the four real worker config dirs on disk carry a
    /// `machineID` and a `userID`, and a pane that loses them is a pane that
    /// re-registers itself on every launch.
    #[test]
    fn seeding_keeps_everything_that_was_already_in_the_file() {
        let dir = temp_dir("merge");
        let cwd = temp_dir("merge-cwd");
        std::fs::write(
            dir.join(".claude.json"),
            r#"{"machineID":"abc","projects":{"/somewhere/else":{"hasTrustDialogAccepted":true}}}"#,
        )
        .unwrap();

        seed_config_dir(&dir, &cwd).unwrap();

        let config = read_config(&dir);
        assert_eq!(config["machineID"], serde_json::json!("abc"), "machineID survived");
        assert_eq!(
            config["projects"]["/somewhere/else"]["hasTrustDialogAccepted"],
            serde_json::json!(true),
            "an unrelated project's trust flag survived"
        );
        assert_eq!(config["projects"][project_key(&cwd)]["hasTrustDialogAccepted"], serde_json::json!(true));

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&cwd);
    }

    /// The L1-reintroduction test. One config dir, two targets: seeding for the
    /// second must not cost the first its trust flag, because a pane switched
    /// back would then sit on a dialog while `fleet send` reports success.
    #[test]
    fn seeding_a_second_target_leaves_the_first_targets_trust_intact() {
        let dir = temp_dir("two-targets");
        let first = temp_dir("target-a");
        let second = temp_dir("target-b");

        seed_config_dir(&dir, &first).unwrap();
        seed_config_dir(&dir, &second).unwrap();

        let config = read_config(&dir);
        for cwd in [&first, &second] {
            assert_eq!(
                config["projects"][project_key(cwd)]["hasTrustDialogAccepted"],
                serde_json::json!(true),
                "{} lost its trust flag",
                cwd.display()
            );
        }

        for dir in [&dir, &first, &second] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    /// A corrupt `.claude.json` must not stop a pane from booting. Onboarding is
    /// the failure we are preventing; refusing to spawn is a worse one.
    #[test]
    fn a_corrupt_config_is_replaced_rather_than_fatal() {
        let dir = temp_dir("corrupt");
        let cwd = temp_dir("corrupt-cwd");
        std::fs::write(dir.join(".claude.json"), "{not json").unwrap();

        seed_config_dir(&dir, &cwd).unwrap();
        assert_eq!(read_config(&dir)["hasCompletedOnboarding"], serde_json::json!(true));

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&cwd);
    }

    /// Both spawn paths must set the pane's own name and the socket, and must
    /// clear the child-session marker. Asserted through `CommandBuilder`'s own
    /// view so it covers the environment the child will actually get.
    #[test]
    fn every_pane_knows_its_name_and_where_the_fleet_is() {
        let socket = PathBuf::from("/tmp/fleetor-test.sock");
        let cwd = PathBuf::from("/tmp");
        let ctx = PaneContext::baked();
        let orch = orch_command(&cwd, &socket, &ctx);
        assert_eq!(orch.get_env("FLEETOR_PANE").unwrap(), "orch");
        assert_eq!(orch.get_env("FLEET_SOCKET").unwrap(), socket.as_os_str());
        assert!(orch.get_env("CLAUDE_CODE_CHILD_SESSION").is_none());

        let worker = worker_command(3, &cwd, Path::new("/tmp/cfg"), &socket, "sk-test", &ctx);
        assert_eq!(worker.get_env("FLEETOR_PANE").unwrap(), "worker-3");
        assert_eq!(worker.get_env("FLEET_SOCKET").unwrap(), socket.as_os_str());
        assert!(worker.get_env("CLAUDE_CODE_CHILD_SESSION").is_none());
    }

    /// L2, as a test rather than a comment: the auth token is set, the api key is
    /// not. With both set, the interactive TUI never reaches its input box.
    #[test]
    fn a_worker_carries_the_auth_token_and_never_the_api_key() {
        let worker =
            worker_command(1, Path::new("/tmp"), Path::new("/tmp/cfg"), Path::new("/tmp/s.sock"), "sk-secret", &PaneContext::baked());
        assert_eq!(worker.get_env("ANTHROPIC_AUTH_TOKEN").unwrap(), "sk-secret");
        assert!(worker.get_env("ANTHROPIC_API_KEY").is_none(), "L2: this wedges the pane on approval");
        assert_eq!(
            worker.get_env("ANTHROPIC_BASE_URL").unwrap(),
            PaneContext::baked().launch.worker_base_url.as_str(),
        );
        assert_eq!(worker.get_env("CLAUDE_CONFIG_DIR").unwrap(), "/tmp/cfg");
    }

    /// `--permission-mode auto` is load-bearing: the default is `manual`, which
    /// wedges on the first tool call. And the brief goes in as a system prompt,
    /// never as a `CLAUDE.md` the worker could see in `git status` and delete.
    #[test]
    fn a_worker_runs_prompt_free_and_is_briefed_through_its_system_prompt() {
        let worker =
            worker_command(2, Path::new("/tmp"), Path::new("/tmp/cfg"), Path::new("/tmp/s.sock"), "k", &PaneContext::baked());
        let args: Vec<String> =
            worker.get_argv().iter().map(|a| a.to_string_lossy().into_owned()).collect();
        assert!(args.windows(2).any(|w| w == ["--permission-mode", "auto"]), "{args:?}");
        let brief_at = args.iter().position(|a| a == "--append-system-prompt").expect("briefed");
        assert!(args[brief_at + 1].contains("You are `worker-2`"), "the brief names the pane");
    }
}
