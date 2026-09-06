//! **The contract step's tripwire** (WP-25, issue #23; C25, C31, C33, C36, C38).
//!
//! Phase 1 was expand–migrate–contract. The expand step wrote every vendor answer
//! down once as [`CLAUDE_CODE_SPEC`]; five migrate batches pointed the call sites
//! at it; this file is what stops a later session pointing one back.
//!
//! It reads source text rather than running anything, the same move
//! `tests/dev_mode.rs` makes for Tier 1.4 and the veil, and for the identical
//! reason: the property worth pinning is *which files spell a thing*, and a test
//! that runs the code can only observe that the right string arrived, never where
//! it came from. A call site that re-hardcodes `CLAUDE_CONFIG_DIR` produces byte
//! for byte the same command as one that reads `config_dir.env_var` — the whole
//! conformance suite stays green, and the seam is quietly gone.
//!
//! ## What it catches
//!
//! A vendor string the spec owns, spelled as a literal in production code in one
//! of the [`MIGRATED`] files. The needles are not a hand-written list of things
//! that look Claude-shaped: every one is [pinned](NEEDLES) to the spec field it
//! came from, so a needle that stops being the spec's answer fails this file
//! instead of silently matching nothing.
//!
//! ## What it permits, deliberately
//!
//! 1. **`placement/harness.rs`.** It is where the answers live; it is not scanned.
//! 2. **Everything below the first `#[cfg(test)]`.** A unit test asserting
//!    `get_env("CLAUDE_CONFIG_DIR")` is asserting the observable, which is exactly
//!    what a migration must not change — pinning it is the point, not a relapse.
//! 3. **Comment lines.** `spawn.rs` and `context_gauge.rs` explain *why* a
//!    variable is what it is, and prose that may not name its own subject is prose
//!    nobody can maintain.
//! 4. **The one anchored exception in [`NAMED_NOT_REPEATED`]** — a constant the
//!    spec *names* rather than copies, which is the opposite of the failure here.
//! 5. **`program.bin` (`claude`), `transcript.subdir` (`projects`),
//!    `transcript.file_ext` (`jsonl`) and the command spellings (`/clear`,
//!    `/compact`).** They are real answers and they are also ordinary English; a
//!    needle for them fires on `claude_code()`, on every path join, and on the
//!    fleet's own canonical command names. **A tripwire that must be suppressed on
//!    its first real use is worse than none**, so those four are left to the
//!    conformance suite, which asserts them end to end through `place`.
//! 6. **The archive manifest's prose** in `runs.rs`, which says a transcript is a
//!    *"Claude Code session .jsonl file"*. That sentence becomes wrong when a
//!    second harness registers, and the fix is `transcript_format` in
//!    `manifest.json` (#39, checkpoint 13's [`Transcript::format`]) rather than a
//!    grep — the words are for a human reading a run cold, not a call site.
//!
//! ## Two things it cannot catch, named so nobody assumes it does
//!
//! **`spawn.rs`'s `CLAUDE_CODE_CHILD_SESSION` removal** is a real vendor literal
//! and there is no needle for it, because a needle has to be pinned to a spec
//! field and no checkpoint owns it (C38). It is not checkpoint 5: that scrub is
//! about credentials and is reported on [`Placed::scrubbed`], and this is a
//! session marker removed from *every* pane in `apply_terminal_env`. Moving it
//! would have meant either changing a caller-observable list in the batch whose
//! job is to change nothing, or adding a fifteenth checkpoint in the step that
//! deletes rather than defines. Phase 2 answers it.
//!
//! **`src-tauri/src/write_guardrail.py` used to spell the tool names a second
//! time** — its own `WRITE_TOOLS` set, which could drift from the spec's. #31
//! closed it the way this file said it had to be closed: the script is *handed*
//! its tools on `--tool` rather than baking them, so there is one spelling and no
//! needle is needed. What still cannot be scanned here is the script generally,
//! because it is not Rust; what pins it now is the conformance suite, which
//! asserts every `write_tools` name reaches the installed hook command.
//!
//! [`Placed::scrubbed`]: fleetor_shell::placement::Placed
//!
//! [`CLAUDE_CODE_SPEC`]: fleetor_shell::placement::harness::CLAUDE_CODE_SPEC
//! [`Transcript::format`]: fleetor_shell::placement::harness::Transcript

use std::path::{Path, PathBuf};

use fleetor_shell::placement::harness::{claude_code, HarnessSpec};

/// The files the five migrate batches touched, plus the two that were already
/// spec-driven — every production file that has ever had a reason to spell one of
/// these strings.
///
/// A file is on this list because a *call site* in it reaches a harness fact. It
/// is not the list of files that mention Claude Code: `prompts.rs` carries the
/// fleet's default worker endpoint (a provider choice, C9) and `fleet.rs` carries
/// the `.env` walk that fills it, and neither is a harness answer.
const MIGRATED: [&str; 12] = [
    "src-tauri/src/placement/spawn.rs",
    "src-tauri/src/placement/mod.rs",
    "src-tauri/src/guardrail.rs",
    "src-tauri/src/pty.rs",
    "src-tauri/src/deliver.rs",
    "src-tauri/src/context_gauge.rs",
    "src-tauri/src/orphans.rs",
    "src-tauri/src/runs.rs",
    "src-tauri/src/fleet.rs",
    "src-tauri/src/evaluator.rs",
    "src-tauri/src/prompts.rs",
    "src-tauri/src/lib.rs",
];

/// A needle, and the checkpoint field it is the current answer to.
///
/// The second element is what makes this list maintainable rather than
/// decorative: [`the_needles_are_still_the_specs_own_answers`] resolves every one
/// against the live spec, so a needle can never drift into meaning nothing. A
/// harness whose answer changes gets one failing test naming the field, not a
/// grep that silently stops guarding it.
type Needle = (&'static str, &'static str, fn(&HarnessSpec) -> Vec<String>);

/// Every distinctive string the spec owns. See the module header for the five
/// answers deliberately absent — they are the ones that are also ordinary
/// English, and a needle for them would fire on the fleet's own vocabulary.
const NEEDLES: &[Needle] = &[
    ("cp 2 — brief.argv_flag", "--system-prompt", |s| opt(s.brief.argv_flag)),
    ("cp 3 — posture.model_env", "ANTHROPIC_MODEL", |s| opt(s.posture.model_env)),
    ("cp 3 — posture.permission_flag", "--permission-mode", |s| opt(s.posture.permission_flag)),
    ("cp 4 — config_dir.env_var", "CLAUDE_CONFIG_DIR", |s| one(s.config_dir.env_var)),
    ("cp 4 — config_dir.seed_file", ".claude.json", |s| one(s.config_dir.seed_file)),
    ("cp 4 — config_dir.seed_keys", "hasCompletedOnboarding", |s| list(s.config_dir.seed_keys)),
    ("cp 5 — credentials.base_url_env", "ANTHROPIC_BASE_URL", |s| opt(s.credentials.base_url_env)),
    ("cp 5 — credentials.token_env", "ANTHROPIC_AUTH_TOKEN", |s| opt(s.credentials.token_env)),
    ("cp 5 — credentials.scrubbed_env", "ANTHROPIC_API_KEY", |s| list(s.credentials.scrubbed_env)),
    ("cp 6 — isolation.credential_env", "CLAUDE_SECURESTORAGE_CONFIG_DIR", |s| {
        opt(s.isolation.credential_env)
    }),
    ("cp 7 — guardrail.hook_event", "PreToolUse", |s| one(s.guardrail.hook_event)),
    // One name out of the list, not the joined matcher: the failure worth catching
    // is a second copy of the vendor's *tool names*, and a second copy would not
    // necessarily be spelled as the same alternation. This row used to split the
    // matcher on `|` to recover the list, which is the evidence that the list was
    // the real fact and the matcher a rendering of it — #31 made the spec say so.
    ("cp 7 — guardrail.write_tools", "MultiEdit", |s| list(s.guardrail.write_tools)),
    ("cp 9 — typing.paste_start", "\x1b[200~", |s| bytes(s.typing.paste_start)),
    ("cp 9 — typing.paste_end", "\x1b[201~", |s| bytes(s.typing.paste_end)),
    ("cp 11 — gauge.window_env", "CLAUDE_CODE_MAX_CONTEXT_TOKENS", |s| opt(s.gauge.window_env)),
    ("cp 13 — transcript.format", "claude-code-jsonl", |s| one(s.transcript.format)),
    ("cp 14 — project_identity.trust_file", ".claude.json", |s| one(s.project_identity.trust_file)),
    ("cp 14 — project_identity.trust_keys", "hasTrustDialogAccepted", |s| {
        list(s.project_identity.trust_keys)
    }),
];

/// **The convention this tripwire must not fight** (`harness.rs`, `spawn.rs:95`).
///
/// Where a public constant already holds a value, the spec *names the item*
/// rather than repeating its string, so the two cannot drift by editing one side.
/// That is the opposite of the failure here: there is exactly one spelling, and it
/// is the one the spec points at. The anchor is the definition itself, so a
/// *second* use of the string anywhere in the file still fires.
///
/// `guardrail::HOOK_FILE` follows the same convention and needs no row: the hook
/// script is the fleet's rather than the vendor's (checkpoint 7's own doc), so
/// the spec points at the item rather than repeating its string.
const NAMED_NOT_REPEATED: &[(&str, &str, &str)] = &[(
    "src-tauri/src/placement/spawn.rs",
    "pub(super) const ENV_CC_SECURESTORAGE_DIR",
    "checkpoint 6's `credential_env` *is* this item — `harness.rs` names it rather than \
     repeating its string, so the two cannot drift. Anything else in this file spelling it \
     is a second source of truth.",
)];

fn one(s: &str) -> Vec<String> {
    vec![s.to_string()]
}
fn opt(s: Option<&str>) -> Vec<String> {
    s.into_iter().map(str::to_string).collect()
}
fn list(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_string()).collect()
}
fn bytes(b: &[u8]) -> Vec<String> {
    vec![String::from_utf8_lossy(b).into_owned()]
}

/// The repo root — this crate's manifest dir is `src-tauri/`.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("src-tauri has a parent").to_path_buf()
}

/// A file's production half: everything above the first `#[cfg(test)]`, with
/// comment lines dropped, as `(line number, text)`.
///
/// Both exclusions are in the module header. The `#[cfg(test)]` rule holds because
/// every file on [`MIGRATED`] keeps its unit tests in one trailing module — and if
/// one ever stops doing that, this reads *less* and cannot produce a false pass on
/// the production half.
fn production_lines(file: &Path) -> Vec<(usize, String)> {
    let text = std::fs::read_to_string(file)
        .unwrap_or_else(|e| panic!("{} must exist to be checked: {e}", file.display()));
    text.lines()
        .take_while(|line| line.trim() != "#[cfg(test)]")
        .enumerate()
        .map(|(i, line)| (i + 1, line.to_string()))
        .filter(|(_, line)| {
            let t = line.trim_start();
            !t.starts_with("//") && !t.starts_with("/*") && !t.starts_with('*')
        })
        .collect()
}

/// **No migrated call site spells a harness answer.**
///
/// This is the contract half of expand–migrate–contract, asserted rather than
/// promised. Read the module header before adding a row to
/// [`NAMED_NOT_REPEATED`]: an exception that exists so a new literal can compile
/// is this test being deleted one line at a time.
#[test]
fn no_migrated_call_site_spells_a_harness_answer() {
    let root = repo_root();
    let mut hits: Vec<String> = Vec::new();

    for rel in MIGRATED {
        let file = root.join(rel);
        for (n, line) in production_lines(&file) {
            for (field, needle, _) in NEEDLES {
                if !line.contains(needle) {
                    continue;
                }
                let excused = NAMED_NOT_REPEATED
                    .iter()
                    .any(|(f, anchor, _)| *f == rel && line.contains(anchor));
                if excused {
                    continue;
                }
                hits.push(format!("  {rel}:{n} [{field}] {}", line.trim()));
            }
        }
    }

    assert!(
        hits.is_empty(),
        "a harness answer is spelled at a call site again. Every one of these has a field on \
         `HarnessSpec` that says it, reached through `Placed::harness` or the pane's own spec — \
         a literal here is the seam removed while the conformance suite stays green, because a \
         re-hardcoded string produces the identical command. Read the value off the spec that \
         is already in scope.\n{}",
        hits.join("\n"),
    );
}

/// **The needles are still the spec's own answers**, so this file cannot rot into
/// a list of strings that match nothing.
///
/// The sibling of `dev_mode.rs`'s
/// `the_evaluators_needles_still_mean_the_evaluator_and_never_the_critic`, and the
/// same worry: a grep whose needles have quietly stopped describing their subject
/// passes forever. If a harness's answer changes, this fails naming the
/// checkpoint, and the fix is to update the needle — never to drop the row.
#[test]
fn the_needles_are_still_the_specs_own_answers() {
    let spec = claude_code().spec();
    for (field, needle, current) in NEEDLES {
        let answers = current(spec);
        assert!(
            answers.iter().any(|a| a == needle),
            "`{needle}` is no longer what {field} says — it is {answers:?}. The needle stopped \
             guarding anything the moment the answer moved.",
        );
    }
}

/// **Every excused line is still the one line it was excused for.**
///
/// An anchor that no longer matches anything is an exception nobody can see is
/// dead, and the next reader inherits it as licence. This asserts each row of
/// [`NAMED_NOT_REPEATED`] excuses exactly one line, and that the line really does
/// carry the string it is excused for.
#[test]
fn each_excused_line_still_exists_and_is_still_a_single_line() {
    let root = repo_root();
    for (rel, anchor, why) in NAMED_NOT_REPEATED {
        let matched: Vec<(usize, String)> = production_lines(&root.join(rel))
            .into_iter()
            .filter(|(_, line)| line.contains(anchor))
            .collect();
        assert_eq!(matched.len(), 1, "{rel}: `{anchor}` matches {} lines — {why}", matched.len());
        assert!(
            NEEDLES.iter().any(|(_, needle, _)| matched[0].1.contains(needle)),
            "{rel}:{} is excused from a needle it no longer carries. Delete the row.\n{why}",
            matched[0].0,
        );
    }
}
