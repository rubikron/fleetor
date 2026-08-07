//! How a pane's `claude` is launched (D-030, Phase 3).
//!
//! Two commands, deliberately different postures:
//!
//!  - [`orch_command`] — the operator's **own** `claude`. Full environment
//!    inherit, their login, their Opus, their `HOME`. We override only what
//!    the fleet needs, and ours must win, so every override lands *after* the
//!    inherit. Since WP-14 that list includes a fleet-owned `CLAUDE_CONFIG_DIR`
//!    — see [`orch_command`] for the pair of variables that keeps the login.
//!  - [`worker_command`] — an isolated Flash worker: its own `CLAUDE_CONFIG_DIR`,
//!    its own worktree, its own private `HOME` (WP-08), `--permission-mode
//!    auto`, and the DeepSeek endpoint.
//!
//! Everything here that looks like a detail was measured in Phase 0
//! (`docs/notes/tui-spawn-notes.md`) against a real interactive `claude`. The four that
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
//!  - **[`seed_worker_home`] is not optional either (WP-08, the Fence).** Once a
//!    worker's `HOME` is a private dir instead of the operator's real one, the
//!    global `user.name`/`user.email` git used to find there are gone too — a
//!    worker's first commit fails outright, and `fleet done`'s receipt (WP-06)
//!    points at a commit that was never made.
//!
//! What a pane is *told* is not here — it is in `prompts/`, resolved once at
//! bootstrap by [`crate::prompts`] and handed in as a [`PaneContext`]. This module
//! decides how a process is shaped; that one decides what goes in its head. The
//! two `env_remove` calls below are the deliberate exception: they are not
//! settings, they are the three ways a pane wedges forever (D-042).
//!
//! **The brief is `--system-prompt`, not `--append-system-prompt` (D-043).** It
//! *replaces* Claude Code's own prompt rather than following it, so the rendered
//! file is the whole of what a pane is told. `docs/notes/system-prompt-notes.md` is the
//! measurement behind that switch, against CC 2.1.223. Note the one thing it made
//! this module responsible for: `cwd` is passed into the renderer as well as onto
//! the command, because CC's `# Environment` section carried the working directory
//! and that section is gone.

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
/// Which macOS Keychain entry `claude` reads its OAuth credential from (WP-14).
///
/// Set to the empty string it selects the **unsuffixed** service name — the entry
/// the operator's own `/login` already wrote. Left unset while `CLAUDE_CONFIG_DIR`
/// is set, `claude` hashes the config dir into the service name instead and finds
/// an empty namespace. `docs/notes/orch-config-dir-notes.md` §2 reads the
/// derivation out of the CC 2.1.224 binary and measures both outcomes.
const ENV_CC_SECURESTORAGE_DIR: &str = "CLAUDE_SECURESTORAGE_CONFIG_DIR";

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
/// inherit-then-override — their login, their `HOME`, their model, plus the
/// things that make it a pane.
///
/// **`config_dir` is WP-14, and it is the one place `orch` is deliberately
/// *unlike* the operator's daily `claude`.** It must already have been through
/// [`seed_config_dir`] for this exact `cwd`: once `orch` stops using the
/// operator's already-onboarded directory, L1 applies to it exactly as it applies
/// to a worker — an unseeded dir lands on the theme picker and never reaches a
/// prompt (`docs/notes/orch-config-dir-notes.md` §1). What this buys is that
/// `orch`'s session transcript lands under `~/.fleetor`, where rotation archives
/// it with the run instead of leaving the deciding pane's reasoning unreadable
/// (D-059's named gap, closed by D-061).
///
/// **The two variables move together or `orch` is silently logged out.**
/// `claude` namespaces its Keychain service name by a hash of `CLAUDE_CONFIG_DIR`,
/// so setting that alone points it at an empty credential namespace: the pane
/// still reaches its input box, every `fleet send` still reports `accepted`, and
/// the first turn fails. `CLAUDE_SECURESTORAGE_CONFIG_DIR`, **defined and empty**,
/// selects the unsuffixed service name — the entry the operator's own `/login`
/// already wrote. Nothing is read out of the operator's config dir to achieve
/// that; `claude` performs the identical keychain read it performs today.
///
/// What `orch` still does **not** get, and must not: a private `HOME`, a worker's
/// PATH, `--permission-mode`, or any `ANTHROPIC_*` override. The asymmetry with
/// [`worker_command`] is the product (D-030, D-052), not an oversight.
pub fn orch_command(
    cwd: &Path,
    socket: &Path,
    config_dir: &Path,
    ctx: &PaneContext,
) -> CommandBuilder {
    let mut cmd = base_command(&[
        "--system-prompt".to_string(),
        render_orch(&ctx.orch_template, &roster(), &cwd.display().to_string()),
    ]);
    cmd.cwd(cwd);
    apply_pane_env(&mut cmd, PaneId::Orch, socket, augmented_path());
    cmd.env("CLAUDE_CONFIG_DIR", config_dir);
    cmd.env(ENV_CC_SECURESTORAGE_DIR, "");
    cmd
}

// --- the workers --------------------------------------------------------------

/// One worker pane: isolated config dir, its own worktree, its own private
/// `HOME`, Flash on DeepSeek.
///
/// `config_dir` must already have been through [`seed_config_dir`] for this exact
/// `cwd` — the trust flag is keyed by absolute project path, so a worker pointed
/// at a new target with an old seed sits on a trust dialog while `fleet send`
/// reports success (L1). `home` must already have been through
/// [`seed_worker_home`] — see this module's doc comment for what an unseeded one
/// costs.
pub fn worker_command(
    slot: u8,
    cwd: &Path,
    home: &Path,
    config_dir: &Path,
    socket: &Path,
    api_key: &str,
    ctx: &PaneContext,
) -> CommandBuilder {
    let pane = PaneId::Worker(slot);
    let mut cmd = base_command(&[
        "--permission-mode".to_string(),
        ctx.launch.worker_permission_mode.clone(),
        "--system-prompt".to_string(),
        render_worker(&ctx.worker_template, pane, &roster(), &cwd.display().to_string()),
    ]);
    cmd.cwd(cwd);
    apply_pane_env(&mut cmd, pane, socket, worker_augmented_path());

    // The Fence (WP-08): a private HOME so `~/.ssh`, the operator's real Claude
    // config and shell profiles stop being reachable *by name*. Set after the
    // parent-environment inherit like every other override here, so ours wins.
    // `home` must already exist and carry a seeded `.gitconfig` — see
    // `seed_worker_home` — or a worker's first commit fails with no
    // `user.name`/`user.email` and WP-06's receipt points at nothing.
    cmd.env("HOME", home);
    cmd.env("CLAUDE_CONFIG_DIR", config_dir);
    cmd.env("ANTHROPIC_BASE_URL", &ctx.launch.worker_base_url);
    cmd.env("ANTHROPIC_AUTH_TOKEN", api_key);
    cmd.env("ANTHROPIC_MODEL", &ctx.launch.worker_model);
    // CC does not recognize the worker model name and would assume a 200k
    // window, auto-compacting early (WP-02 finding). Export the fleet's own
    // stated window instead — the same constant the context gauge divides by,
    // so CC's bookkeeping and our display can never disagree (D-054).
    cmd.env(
        "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
        crate::context_gauge::WORKER_WINDOW_TOKENS.to_string(),
    );
    // Not "don't set it" — *unset* it. The worker inherits the operator's
    // environment, and an `ANTHROPIC_API_KEY` sitting in their shell profile is
    // enough to park the pane on an api-key approval prompt forever (L2).
    cmd.env_remove("ANTHROPIC_API_KEY");
    cmd.env_remove("ANTHROPIC_DEFAULT_OPUS_MODEL");
    cmd.env_remove("ANTHROPIC_DEFAULT_SONNET_MODEL");
    // The Fence again (WP-14): orch sets this to reach the operator's own Keychain
    // entry, and a worker inherits the app's environment. Unset rather than
    // not-set, so a worker can never be handed the key to the operator's login —
    // it has `ANTHROPIC_AUTH_TOKEN` and needs nothing from the keychain.
    cmd.env_remove(ENV_CC_SECURESTORAGE_DIR);
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
///
/// `path` is the caller's to choose (WP-08, the Fence): [`augmented_path`] for
/// orch, [`worker_augmented_path`] for a worker. Both resolve `fleet`'s location
/// the identical way; they differ only in whether the operator's own HOME
/// contributes rungs.
fn apply_pane_env(cmd: &mut CommandBuilder, pane: PaneId, socket: &Path, path: String) {
    cmd.env("PATH", path);
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
///
/// **Orch only.** This reads `$HOME` from the *app's own* environment — the
/// operator's real HOME, since orch is their own `claude` (D-030's "orch is
/// untouched"). [`worker_augmented_path`] is the worker's version and
/// deliberately does not call this: it must not bake the operator's HOME into a
/// worker's PATH, which is the other half of the fix this module's doc comment
/// promises alongside the private `HOME` itself.
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

/// The worker's PATH (WP-08, the Fence): the fleet-bin rung and the system
/// dirs, none of the operator-HOME rungs `augmented_path` adds. Before this fix,
/// every worker's PATH carried `{operator's real $HOME}/.local/bin` and
/// `.../.bun/bin` regardless of the worker's own (now private) `HOME` — a name
/// pointed straight at the operator's tooling, defeating the point of fencing
/// `HOME` at all.
///
/// `existing` — the PATH inherited from the app's own process — is left alone.
/// It is not an operator-HOME rung by construction (it is whatever launched the
/// app), and stripping it is a sandboxing decision this package's spec rules
/// out; see `docs/notes/fence-notes.md` for what that leaves reachable.
pub fn worker_augmented_path() -> String {
    let existing = std::env::var("PATH").unwrap_or_default();
    let mut prefix = String::new();
    if let Some(dir) = fleet_bin_path().and_then(|p| p.parent().map(Path::to_path_buf)) {
        prefix.push_str(&dir.to_string_lossy());
        prefix.push(':');
    }
    format!("{prefix}/opt/homebrew/bin:/usr/local/bin:{existing}")
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

// --- the private HOME (WP-08, the Fence) ---------------------------------------

/// Give a worker's private `HOME` the one file its commits need: a minimal
/// `.gitconfig` naming it as the commit author.
///
/// This is the WP-06 interaction `00-index.md` records: once `HOME` stops
/// pointing at the operator's real one, the global `user.name`/`user.email` git
/// used to inherit are gone too, and a worker's first `git commit` (`fleet
/// done`'s first step) fails outright — no name, no receipt, nothing for a
/// reviewer to read.
///
/// Unlike [`seed_config_dir`], this does **not** merge-write on every spawn: a
/// worker's private HOME is not a shared, evolving config dir the way
/// `CLAUDE_CONFIG_DIR` is (nothing else legitimately writes into it), so
/// touching an existing file on every relaunch would only risk clobbering
/// something a future breakage-catalogue entry seeded on purpose. Written once,
/// left alone after that.
pub fn seed_worker_home(dir: &Path, slot: u8) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("create worker home {}: {e}", dir.display()))?;
    let file = dir.join(".gitconfig");
    if file.exists() {
        return Ok(());
    }
    let text = format!("[user]\n\tname = fleet worker-{slot}\n\temail = worker-{slot}@fleetor.local\n");
    let tmp = dir.join(".gitconfig.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &file).map_err(|e| format!("install {}: {e}", file.display()))
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
        let orch = orch_command(&cwd, &socket, Path::new("/tmp/orch-cfg"), &ctx);
        assert_eq!(orch.get_env("FLEETOR_PANE").unwrap(), "orch");
        assert_eq!(orch.get_env("FLEET_SOCKET").unwrap(), socket.as_os_str());
        assert!(orch.get_env("CLAUDE_CODE_CHILD_SESSION").is_none());

        let worker =
            worker_command(3, &cwd, Path::new("/tmp/home"), Path::new("/tmp/cfg"), &socket, "sk-test", &ctx);
        assert_eq!(worker.get_env("FLEETOR_PANE").unwrap(), "worker-3");
        assert_eq!(worker.get_env("FLEET_SOCKET").unwrap(), socket.as_os_str());
        assert!(worker.get_env("CLAUDE_CODE_CHILD_SESSION").is_none());
    }

    /// L2, as a test rather than a comment: the auth token is set, the api key is
    /// not. With both set, the interactive TUI never reaches its input box.
    #[test]
    fn a_worker_carries_the_auth_token_and_never_the_api_key() {
        let worker = worker_command(
            1,
            Path::new("/tmp"),
            Path::new("/tmp/home"),
            Path::new("/tmp/cfg"),
            Path::new("/tmp/s.sock"),
            "sk-secret",
            &PaneContext::baked(),
        );
        assert_eq!(worker.get_env("ANTHROPIC_AUTH_TOKEN").unwrap(), "sk-secret");
        assert!(worker.get_env("ANTHROPIC_API_KEY").is_none(), "L2: this wedges the pane on approval");
        assert_eq!(
            worker.get_env("ANTHROPIC_BASE_URL").unwrap(),
            PaneContext::baked().launch.worker_base_url.as_str(),
        );
        assert_eq!(worker.get_env("CLAUDE_CONFIG_DIR").unwrap(), "/tmp/cfg");
    }

    /// D-054: a worker is told its real window, and it is the gauge's number —
    /// spelled once. Orch never gets the override; its model is recognized and
    /// its window is not ours to state.
    #[test]
    fn a_worker_is_told_the_window_the_gauge_divides_by() {
        let ctx = PaneContext::baked();
        let worker = worker_command(
            2,
            Path::new("/tmp"),
            Path::new("/tmp/home"),
            Path::new("/tmp/cfg"),
            Path::new("/tmp/s.sock"),
            "sk-test",
            &ctx,
        );
        assert_eq!(
            worker.get_env("CLAUDE_CODE_MAX_CONTEXT_TOKENS").unwrap(),
            crate::context_gauge::WORKER_WINDOW_TOKENS.to_string().as_str(),
        );
        let orch =
            orch_command(Path::new("/tmp"), Path::new("/tmp/s.sock"), Path::new("/tmp/orch-cfg"), &ctx);
        assert!(orch.get_env("CLAUDE_CODE_MAX_CONTEXT_TOKENS").is_none());
    }

    /// `--permission-mode auto` is load-bearing: the default is `manual`, which
    /// wedges on the first tool call. And the brief goes in as a system prompt,
    /// never as a `CLAUDE.md` the worker could see in `git status` and delete —
    /// now *replacing* CC's own rather than appending to it (D-043).
    #[test]
    fn a_worker_runs_prompt_free_and_is_briefed_through_its_system_prompt() {
        let worker = worker_command(
            2,
            Path::new("/tmp"),
            Path::new("/tmp/home"),
            Path::new("/tmp/cfg"),
            Path::new("/tmp/s.sock"),
            "k",
            &PaneContext::baked(),
        );
        let args: Vec<String> =
            worker.get_argv().iter().map(|a| a.to_string_lossy().into_owned()).collect();
        assert!(args.windows(2).any(|w| w == ["--permission-mode", "auto"]), "{args:?}");
        assert!(
            !args.iter().any(|a| a == "--append-system-prompt"),
            "D-043: the brief replaces CC's system prompt, it no longer appends to it",
        );
        let brief_at = args.iter().position(|a| a == "--system-prompt").expect("briefed");
        assert!(args[brief_at + 1].contains("You are `worker-2`"), "the brief names the pane");
        assert!(args[brief_at + 1].contains("/tmp"), "and says where the pane is working");
    }

    // --- WP-08: the Fence ------------------------------------------------------

    /// The whole point of the fence: a worker's `HOME` is the private dir it was
    /// handed, never the operator's real one it would otherwise inherit — and orch
    /// is untouched, because it is the operator's own `claude` (D-030).
    ///
    /// `CommandBuilder::new` seeds the *parent* environment at construction
    /// (`get_base_env`), so orch's `HOME` is never literally absent — it is
    /// whatever this test process's own `HOME` is, exactly as inherited. The
    /// assertion is that `spawn.rs` never overrides it, not that the key is
    /// unset.
    #[test]
    fn a_worker_gets_a_private_home_and_orch_keeps_its_own() {
        let socket = PathBuf::from("/tmp/s.sock");
        let ctx = PaneContext::baked();
        let real_home = std::env::var_os("HOME");

        let orch = orch_command(Path::new("/tmp"), &socket, Path::new("/tmp/orch-cfg"), &ctx);
        assert_eq!(
            orch.get_env("HOME").map(|s| s.to_os_string()),
            real_home,
            "orch must inherit the operator's real HOME untouched"
        );

        let worker = worker_command(
            1,
            Path::new("/tmp"),
            Path::new("/tmp/private-home"),
            Path::new("/tmp/cfg"),
            &socket,
            "k",
            &ctx,
        );
        assert_eq!(worker.get_env("HOME").unwrap(), "/tmp/private-home");
    }

    /// The other half of the fix, in the same commit as the private HOME: a
    /// worker's PATH must not carry the rungs `augmented_path` derives from the
    /// operator's real HOME (`~/.local/bin`, `~/.bun/bin`) — otherwise a fenced
    /// `HOME` still leaves the operator's own tooling reachable by name through
    /// PATH instead. Orch keeps today's PATH unchanged.
    #[test]
    fn worker_path_drops_the_operator_home_rungs_orch_keeps_them() {
        let previous = std::env::var("HOME").ok();
        std::env::set_var("HOME", "/Users/operator");

        let orch_path = augmented_path();
        assert!(orch_path.contains("/Users/operator/.local/bin"), "{orch_path}");
        assert!(orch_path.contains("/Users/operator/.bun/bin"), "{orch_path}");
        assert!(orch_path.contains("/opt/homebrew/bin"), "{orch_path}");

        let worker_path = worker_augmented_path();
        assert!(
            !worker_path.contains("/Users/operator"),
            "the operator's HOME must not appear anywhere in a worker's PATH: {worker_path}"
        );
        assert!(worker_path.contains("/opt/homebrew/bin"), "{worker_path}");
        assert!(worker_path.contains("/usr/local/bin"), "{worker_path}");

        match previous {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }

    /// The WP-06 interaction: a worker's private HOME must come seeded with a
    /// gitconfig, or its first `git commit` — `fleet done`'s first step — fails
    /// with no `user.name`/`user.email` once the global one stops being reachable.
    #[test]
    fn seeding_a_worker_home_gives_it_a_gitconfig_naming_the_worker() {
        let dir = temp_dir("worker-home");
        seed_worker_home(&dir, 2).unwrap();

        let text = std::fs::read_to_string(dir.join(".gitconfig")).unwrap();
        assert!(text.contains("fleet worker-2"), "{text}");
        assert!(text.contains("worker-2@fleetor.local"), "{text}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- WP-14: orch's own config dir ------------------------------------------

    /// The whole of WP-14 at the spawn seam: `orch` gets a fleet-owned config
    /// dir, and the variable that keeps its login travels with it.
    ///
    /// The pair is the test, not either half. `CLAUDE_CONFIG_DIR` alone points
    /// `claude` at a Keychain namespace derived from that path, which has never
    /// been logged into — the pane reaches its input box, every `fleet send`
    /// reports `accepted`, and the first turn fails
    /// (`docs/notes/orch-config-dir-notes.md` §1–2 measured both).
    #[test]
    fn orch_gets_its_own_config_dir_and_keeps_the_operators_login() {
        let orch = orch_command(
            Path::new("/tmp"),
            Path::new("/tmp/s.sock"),
            Path::new("/tmp/fleetor/pane-config/orch"),
            &PaneContext::baked(),
        );
        assert_eq!(orch.get_env("CLAUDE_CONFIG_DIR").unwrap(), "/tmp/fleetor/pane-config/orch");
        assert_eq!(
            orch.get_env(ENV_CC_SECURESTORAGE_DIR).expect("without this orch is silently logged out"),
            "",
            "defined and EMPTY selects the operator's own keychain entry; any value is a namespace",
        );
    }

    /// The asymmetry is the product (D-030, D-052). `orch` moving onto its own
    /// config dir must not drag any of the worker posture along with it — a
    /// private `HOME`, a DeepSeek endpoint or an auth-token override would each
    /// take its login away by a different route.
    #[test]
    fn orch_takes_a_config_dir_and_none_of_the_worker_isolation() {
        let ctx = PaneContext::baked();
        let orch =
            orch_command(Path::new("/tmp"), Path::new("/tmp/s.sock"), Path::new("/tmp/orch-cfg"), &ctx);

        for key in [
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_MODEL",
            "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
        ] {
            assert!(orch.get_env(key).is_none(), "orch must not carry the worker's {key}");
        }
        assert_eq!(
            orch.get_env("HOME").map(|s| s.to_os_string()),
            std::env::var_os("HOME"),
            "the Fence is worker-only: orch keeps the operator's real HOME",
        );

        let args: Vec<String> =
            orch.get_argv().iter().map(|a| a.to_string_lossy().into_owned()).collect();
        assert!(
            !args.iter().any(|a| a == "--permission-mode"),
            "orch keeps the operator's own permission posture: {args:?}",
        );
    }

    /// The other direction of the same seam: a worker inherits the app's
    /// environment, and the app is about to set the variable that unlocks the
    /// operator's own login. Unset it, the way `ANTHROPIC_API_KEY` is unset.
    #[test]
    fn a_worker_never_inherits_the_key_to_the_operators_keychain_entry() {
        let worker = worker_command(
            1,
            Path::new("/tmp"),
            Path::new("/tmp/home"),
            Path::new("/tmp/cfg"),
            Path::new("/tmp/s.sock"),
            "sk-test",
            &PaneContext::baked(),
        );
        assert!(worker.get_env(ENV_CC_SECURESTORAGE_DIR).is_none());
    }

    /// L1 is not worker-only any more. `orch`'s dir is a fleet-owned directory
    /// like any other, so it needs the same two keys for the same reason — an
    /// unseeded one lands on the theme picker and never reaches a prompt
    /// (`docs/notes/orch-config-dir-notes.md` §1, arm `virgin`).
    #[test]
    fn orchs_config_dir_needs_the_same_two_keys_a_workers_does() {
        let dir = temp_dir("orch-cfg");
        let cwd = temp_dir("orch-target");

        seed_config_dir(&dir, &cwd).unwrap();

        let config = read_config(&dir);
        assert_eq!(config["hasCompletedOnboarding"], serde_json::json!(true));
        assert_eq!(
            config["projects"][project_key(&cwd)]["hasTrustDialogAccepted"],
            serde_json::json!(true),
        );

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&cwd);
    }

    /// Written once, left alone after that (unlike `seed_config_dir`'s
    /// merge-on-every-spawn): a second seed on an existing gitconfig must not
    /// clobber whatever is already there.
    #[test]
    fn seeding_a_worker_home_a_second_time_does_not_clobber_an_existing_gitconfig() {
        let dir = temp_dir("worker-home-existing");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".gitconfig"), "[user]\n\tname = hand-edited\n").unwrap();

        seed_worker_home(&dir, 4).unwrap();

        let text = std::fs::read_to_string(dir.join(".gitconfig")).unwrap();
        assert_eq!(text, "[user]\n\tname = hand-edited\n", "an existing gitconfig must survive re-seeding");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
