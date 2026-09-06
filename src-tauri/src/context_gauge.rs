//! The live context gauge: a read-only sampler over a worker's own transcript
//! (WP-04, `docs/notes/context-gauge-notes.md`).
//!
//! **Observer-only, by construction.** This module reads files; it writes
//! nothing, sends nothing, and nothing in [`crate::deliver`]'s message path
//! calls into it. It has no [`fleetor_core::Store`] dependency at all — every
//! function here is pure or touches only its own `Mutex<HashMap>`, so the
//! caller decides what, if anything, to persist (`crate::fleet` does, via the
//! same `note()` helper every other Activity line already goes through).
//!
//! Two instruments, two different honesty rules:
//!
//!  - **The spawn-time estimate** ([`spawn_estimate_notice_text`]) is a
//!    chars/4 guess over text we already hold in memory — the rendered brief
//!    — labeled `≈` and named for exactly what it measures (the brief, not
//!    the pane's total starting context: tool defs, memory files and CC's own
//!    fixed scaffolding are real and roughly constant per launch, but this
//!    package does not add a tokenizer to count them — requirements doc,
//!    Out of scope).
//!  - **The live gauge** ([`GaugeSources::sample`]) never guesses. It reads a
//!    real `usage` object from the pane's own transcript or reports nothing —
//!    no chars/4 fallback here, even though the requirements doc's design
//!    sketch floats one, because a transcript with no completed turn yet has
//!    nothing honest to approximate (`docs/notes/context-gauge-notes.md` §4).
//!
//! The orchestrator is never sampled. [`GaugeSources`] is only ever `record`ed
//! for a worker (`crate::fleet::spawn_pane`); an orch entry simply has no
//! source, so [`GaugeSources::sample`] returns `None` for it the same way it
//! would for a pane that has not spawned yet — reading the operator's own
//! `~/.claude` transcript is an ownership call above this builder
//! (requirements doc, Invariant guardrails).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use fleetor_core::pane::{ContextGauge, PaneId};

/// Tier 2: the worker window the fleet operates to (`decisions.md` D-054) —
/// deliberately **not** the 200k Claude Code silently assumes for a model
/// name it does not recognize (WP-02 finding, `docs/notes/system-prompt-notes.md`
/// §4). Operator-set at 500k on 2026-08-06, after that assumption surfaced
/// live as a premature auto-compact warning.
///
/// One number, two consumers: this gauge's denominator, and the
/// `CLAUDE_CODE_MAX_CONTEXT_TOKENS` env `placement::spawn::worker_command_with` exports so
/// CC's own auto-compact bookkeeping works to the same window. They must
/// never diverge — a gauge reading 100% while CC believes 40% (or the
/// reverse) is exactly the quiet lie this product exists to avoid.
///
/// **Both consumers now reach it through checkpoint 11** (WP-25, C11): it is
/// [`GaugeSource::window_tokens`](crate::placement::harness::GaugeSource::window_tokens)
/// and [`GaugeSource::window_env`](crate::placement::harness::GaugeSource::window_env)
/// that this gauge and the worker command read, and Claude Code's spec names
/// this constant rather than repeating its value. It stays defined here because
/// it is Claude Code's *answer*, not the seam's — a harness that publishes its
/// own window answers `None` and is not measured against this number at all.
pub const WORKER_WINDOW_TOKENS: u32 = 500_000;

/// Where a pane's percent first earns an informational Notice (WP-04
/// performance criteria: "at most one Notice on first crossing of ~80%").
/// Never acted on: nothing here refuses, warns-and-blocks, or auto-compacts —
/// it only tells an agent, or the operator, that a decision is theirs.
pub const NOTICE_THRESHOLD_PCT: u8 = 80;

/// Rough chars-per-token, used **only** for the spawn-time estimate over text
/// already in memory. Never for the live gauge — see the module doc.
const CHARS_PER_TOKEN_ESTIMATE: usize = 4;

// --- where a worker's transcript lives ------------------------------------------

/// Where to look for one worker's own transcript: the harness the pane runs, its
/// isolated config dir and the cwd it was launched in. Transcripts are keyed by
/// the resolved absolute cwd (`docs/notes/context-gauge-notes.md` §1), so the
/// config dir alone does not say which of its `projects/*` subdirectories is this
/// pane's.
///
/// **The harness is carried rather than assumed** (WP-25, M23). Resolving a cwd
/// into a project key was one shared function with two consumers — the trust flag
/// and this gauge — and each harness now declares its own answer, so the gauge
/// asks the pane's own harness instead of calling Claude Code's canonicalization
/// by name. It is `place` that decides which harness a pane runs, and this is
/// recorded from the same `Placed` that carries the command, so the key the gauge
/// looks under and the key the pane's trust record was written under cannot come
/// from different harnesses.
#[derive(Debug, Clone)]
pub struct TranscriptSource {
    /// The harness this pane runs, for the project key it is looked up under.
    pub harness: &'static dyn crate::placement::harness::Harness,
    pub config_dir: PathBuf,
    pub cwd: PathBuf,
}

/// The pane → transcript-source map: `record`ed once per worker at spawn,
/// `sample`d fresh on every `fleet roster` (CLI or UI poll alike — both
/// converge on `crate::deliver`'s single `AppCommand::Roster` handler, so
/// there is exactly one place this is consulted). A pane with no entry —
/// not yet spawned, or the orchestrator, never recorded here at all —
/// samples as absent, never a placeholder.
#[derive(Default)]
pub struct GaugeSources(Mutex<HashMap<PaneId, TranscriptSource>>);

impl GaugeSources {
    pub fn record(&self, pane: PaneId, source: TranscriptSource) {
        if let Ok(mut map) = self.0.lock() {
            map.insert(pane, source);
        }
    }

    /// Sample this pane's transcript right now, or `None` if it was never
    /// recorded, has no transcript on disk yet, or the transcript has no
    /// completed turn yet. Re-reads the directory every call rather than
    /// caching a filename — see [`latest_transcript`] for why.
    ///
    /// **The window comes from the pane's own harness** (WP-25, checkpoint 11),
    /// and the two ways it can be absent are the reason this reads as three
    /// early returns rather than one call. A harness that reads usage from a
    /// store this module knows nothing about
    /// ([`reads_transcript`](crate::placement::harness::GaugeSource::reads_transcript)
    /// `== false`), and a harness that publishes its own window instead of
    /// letting the fleet assert one
    /// ([`window_tokens`](crate::placement::harness::GaugeSource::window_tokens)
    /// `== None`), both sample as **absent**. That is D-054's rule applied to
    /// the seam rather than to one vendor: an unavailable gauge says
    /// unavailable, and the one thing it may never do is divide by a window
    /// borrowed from a harness that is not this pane's — a number that looks
    /// right and is wrong.
    pub fn sample(&self, pane: PaneId) -> Option<ContextGauge> {
        let source = self.0.lock().ok()?.get(&pane).cloned()?;
        let gauge = &source.harness.spec().gauge;
        if !gauge.reads_transcript {
            return None;
        }
        sample_transcript(&source, gauge.window_tokens?)
    }
}

// --- the transcript sampler ------------------------------------------------------

/// `<config_dir>/projects/<slug>/`, where `<slug>` is the pane's project key with
/// every `/` and `.` replaced by `-` — empirically verified character-for-character
/// in `docs/notes/context-gauge-notes.md` §1.
///
/// **The key comes from the pane's own harness** (WP-25, checkpoint 14), which is
/// what makes "the gauge and the trust flag agree on one spelling of a cwd" a
/// property of the seam rather than of two call sites happening to name the same
/// function. For Claude Code that answer is still `std::fs::canonicalize` — the
/// same resolved path it itself sees as its cwd, since macOS resolves `/tmp` and
/// `/var` symlinks on `getcwd` — and it is the key
/// [`Harness::seed_config_dir`](crate::placement::harness::Harness::seed_config_dir)
/// wrote the pane's trust record under.
///
/// **The subdirectory is checkpoint 13's** (WP-25,
/// [`Transcript::subdir`](crate::placement::harness::Transcript::subdir)), read
/// off the same harness as the key.
///
/// **The slug rule is not, and this is now the only place that shows it.** How a
/// harness names the *per-project directory* underneath that subdirectory is not
/// one of the fourteen answers, so the `/`-and-`.`-to-`-` mapping below is still
/// Claude Code's own literal — the last one in this function, where there were
/// two. Nothing else in the fleet re-derives it: the harvest walks whatever
/// directories it finds rather than computing their names, so this is the single
/// consumer and the single point of drift. A second harness whose transcripts are
/// per-project cannot be read by this gauge until that becomes a checkpoint
/// answer or a method on [`Harness`](crate::placement::harness::Harness); a
/// harness whose store is not per-project at all is unaffected. Left for #39,
/// which is the ticket that has a second layout to generalize against — inventing
/// the shape from one example is how a seam ends up describing one vendor.
fn project_dir(source: &TranscriptSource) -> PathBuf {
    let resolved = source.harness.project_key(&source.cwd);
    let slug: String =
        resolved.chars().map(|c| if c == '/' || c == '.' { '-' } else { c }).collect();
    source.config_dir.join(source.harness.spec().transcript.subdir).join(slug)
}

/// The pane's current transcript: the most-recently-modified file with this
/// harness's transcript extension (WP-25, checkpoint 13's
/// [`Transcript::file_ext`](crate::placement::harness::Transcript::file_ext))
/// under its project directory.
///
/// Read fresh on every call, never cached from spawn. A `/clear` resets a
/// pane's session (`docs/notes/command-channel-notes.md`); whether Claude Code
/// opens a new `<uuid>.jsonl` for that or keeps writing the same one was not
/// worth a fifth spike run to settle — picking "most recently modified" is
/// correct under either answer, at the cost of one directory read per sample.
fn latest_transcript(dir: &Path, file_ext: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some(file_ext))
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
}

fn sample_transcript(source: &TranscriptSource, window_tokens: u32) -> Option<ContextGauge> {
    let path =
        latest_transcript(&project_dir(source), source.harness.spec().transcript.file_ext)?;
    let text = std::fs::read_to_string(&path).ok()?;
    let used = last_turn_usage(&text)?;
    Some(ContextGauge::new(used, window_tokens))
}

/// The last `assistant` line's total prompt usage, or `None` if the
/// transcript has no completed turn yet.
///
/// **`input_tokens + cache_creation_input_tokens + cache_read_input_tokens`,
/// never `input_tokens` alone.** Prompt caching moves most of a turn's tokens
/// out of `input_tokens` and into `cache_read_input_tokens` the moment the
/// endpoint recognizes repeated context — reading `input_tokens` alone would
/// report a pane's usage *dropping* the instant caching kicked in, which is
/// the opposite of what happened (`docs/notes/context-gauge-notes.md` §3, measured
/// live: 27,723 → 96 on `input_tokens` alone across two turns of the same
/// growing conversation; 27,723 → 27,744 on the sum).
///
/// Scans from the end: two identical `usage` lines are written per real turn
/// (same `message.id`, byte-identical usage), so "last" and "last real turn"
/// agree without de-duplicating.
fn last_turn_usage(transcript: &str) -> Option<u32> {
    transcript.lines().rev().find_map(|line| {
        let entry: serde_json::Value = serde_json::from_str(line).ok()?;
        if entry.get("type")?.as_str()? != "assistant" {
            return None;
        }
        let usage = entry.get("message")?.get("usage")?;
        let field = |name: &str| usage.get(name).and_then(|v| v.as_u64()).unwrap_or(0);
        let total = field("input_tokens")
            + field("cache_creation_input_tokens")
            + field("cache_read_input_tokens");
        Some(total as u32)
    })
}

// --- the spawn-time estimate (the Loadout counter) -------------------------------

/// A rough token count over text already in memory — chars/4, the estimate
/// the requirements doc explicitly sanctions rather than adding a tokenizer
/// dependency ("Out of scope: tokenizer dependencies — estimate is fine,
/// Loadout's own verdict"). Public so `crate::fleet` and this module's own
/// tests share one implementation.
pub fn estimate_tokens(text: &str) -> u32 {
    text.len().div_ceil(CHARS_PER_TOKEN_ESTIMATE) as u32
}

/// The Activity line for one pane launch: its rendered brief's size, and — for
/// a worker, whose window this fleet does track — what fraction of it that
/// is. `window_tokens` is `None` for the orchestrator: this fleet does not
/// assume a window for the operator's own model, the same reason it never
/// samples orch's transcript.
///
/// Pure — returns the sentence; `crate::fleet` appends it as a `Notice`
/// through the same `note()` helper every other bootstrap/spawn announcement
/// already uses, so this module needs no `Store` of its own.
pub fn spawn_estimate_notice_text(pane: PaneId, rendered_brief: &str, window_tokens: Option<u32>) -> String {
    let tokens = estimate_tokens(rendered_brief);
    match window_tokens {
        Some(window) => {
            let pct = ContextGauge::new(tokens, window).pct;
            format!(
                "{pane} spawned with a ≈{tokens}-token brief (≈{pct}% of its {window}-token window)"
            )
        }
        None => format!("{pane} spawned with a ≈{tokens}-token brief"),
    }
}

// --- the 80% notice ---------------------------------------------------------------

/// Whether `pct` has reached the informational threshold. A pure predicate:
/// the "already told this pane once" bookkeeping is a plain `HashSet` owned
/// by `crate::deliver`'s single roster-answering loop, not by this module —
/// keeping it there means the state that must not double-fire lives with the
/// one place that could double-fire it.
pub fn crosses_notice_threshold(pct: u8) -> bool {
    pct >= NOTICE_THRESHOLD_PCT
}

/// The Notice text for a pane's first ~80% crossing. Informs, and says what an
/// agent can do about it — it does not do either of those things itself
/// (requirements doc, Invariant guardrails: "No thresholds that act").
pub fn notice_text(pane: PaneId, gauge: &ContextGauge) -> String {
    format!(
        "{pane} is ≈{}% of its context window (from its transcript, may lag) — \
         consider `fleet cmd {pane} \"/compact <what to keep>\"` or `/clear` (then re-brief it).",
        gauge.pct
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "fleetor-context-gauge-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The exact shape a real `assistant` line has, per
    /// `docs/notes/context-gauge-notes.md` §2 — trimmed to the fields the sampler
    /// actually reads, plus a couple of neighbors that must be ignored.
    fn assistant_line(input: u64, cache_creation: u64, cache_read: u64) -> String {
        serde_json::json!({
            "type": "assistant",
            "message": {
                "role": "assistant", "model": "deepseek-v4-flash",
                "usage": {
                    "input_tokens": input,
                    "cache_creation_input_tokens": cache_creation,
                    "cache_read_input_tokens": cache_read,
                    "output_tokens": 2
                }
            }
        })
        .to_string()
    }

    fn user_line(text: &str) -> String {
        serde_json::json!({"type": "user", "message": {"role": "user", "content": text}}).to_string()
    }

    fn seed_transcript(config_dir: &Path, cwd: &Path, lines: &[String]) {
        let source = TranscriptSource { harness: crate::placement::harness::claude_code(), config_dir: config_dir.to_path_buf(), cwd: cwd.to_path_buf() };
        let dir = project_dir(&source);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("session-a.jsonl"), lines.join("\n")).unwrap();
    }

    // --- project_dir / the slug rule -------------------------------------------

    /// Character-for-character, the rule `docs/notes/context-gauge-notes.md` §1
    /// verified against a real Claude Code run: every `/` and every `.` in
    /// the resolved cwd becomes `-`, nothing else changes.
    #[test]
    fn project_dir_matches_claude_codes_own_encoding() {
        let cwd = temp_dir("slug-cwd");
        let config_dir = temp_dir("slug-config");
        let source = TranscriptSource { harness: crate::placement::harness::claude_code(), config_dir: config_dir.clone(), cwd: cwd.clone() };

        let resolved = crate::placement::spawn::project_key(&cwd);
        let expected_slug: String =
            resolved.chars().map(|c| if c == '/' || c == '.' { '-' } else { c }).collect();

        assert_eq!(project_dir(&source), config_dir.join("projects").join(&expected_slug));
        assert!(!expected_slug.contains('/'), "a slug with a slash would be a nested directory");
    }

    // --- last_turn_usage: the honest-sum finding --------------------------------

    #[test]
    fn no_assistant_line_means_no_reading_at_all() {
        let transcript = [user_line("hello")].join("\n");
        assert_eq!(last_turn_usage(&transcript), None);
    }

    /// The load-bearing finding from the spike: `input_tokens` alone would
    /// *drop* the instant prompt caching kicks in, which reads as the
    /// conversation shrinking when it only grew. The sum must keep climbing.
    #[test]
    fn usage_is_the_sum_of_all_three_input_fields_not_input_tokens_alone() {
        let turn_one = assistant_line(27_723, 0, 0);
        let turn_two = assistant_line(96, 0, 27_648);

        let after_turn_one = last_turn_usage(&turn_one).unwrap();
        let after_turn_two = last_turn_usage(&[turn_one, turn_two].join("\n")).unwrap();

        assert_eq!(after_turn_one, 27_723);
        assert_eq!(after_turn_two, 27_744, "must track the growing conversation, not just input_tokens (96)");
        assert!(after_turn_two > after_turn_one, "a second turn must never look like it shrank the context");
    }

    /// Two identical usage lines per real turn (same content, as Claude Code
    /// actually writes them) must not be double-counted or picked
    /// inconsistently — "last line" and "last real turn" have to agree.
    #[test]
    fn a_duplicated_usage_line_per_turn_does_not_change_the_reading() {
        let line = assistant_line(500, 0, 0);
        let once = last_turn_usage(&line).unwrap();
        let twice = last_turn_usage(&[line.clone(), line].join("\n")).unwrap();
        assert_eq!(once, twice);
    }

    /// Only the *last* assistant line counts — an earlier turn's usage must
    /// never win over a later, larger one.
    #[test]
    fn only_the_last_assistant_lines_usage_is_read() {
        let transcript =
            [assistant_line(1_000, 0, 0), user_line("more"), assistant_line(1_200, 0, 800)].join("\n");
        assert_eq!(last_turn_usage(&transcript), Some(2_000));
    }

    #[test]
    fn a_line_that_is_not_json_is_skipped_rather_than_fatal() {
        let transcript = ["not json at all".to_string(), assistant_line(50, 0, 0)].join("\n");
        assert_eq!(last_turn_usage(&transcript), Some(50));
    }

    // --- sample_transcript / latest_transcript ----------------------------------

    #[test]
    fn no_transcript_directory_at_all_samples_as_absent() {
        let source =
            TranscriptSource { harness: crate::placement::harness::claude_code(), config_dir: temp_dir("no-dir-cfg"), cwd: temp_dir("no-dir-cwd") };
        assert_eq!(sample_transcript(&source, WORKER_WINDOW_TOKENS), None);
    }

    #[test]
    fn a_transcript_with_no_completed_turn_samples_as_absent_not_zero() {
        let cwd = temp_dir("empty-turn-cwd");
        let config_dir = temp_dir("empty-turn-cfg");
        seed_transcript(&config_dir, &cwd, &[user_line("are you there")]);

        let source = TranscriptSource { harness: crate::placement::harness::claude_code(), config_dir, cwd };
        assert_eq!(sample_transcript(&source, WORKER_WINDOW_TOKENS), None, "no assistant reply yet");
    }

    #[test]
    fn a_real_transcript_samples_a_gauge_against_the_worker_window() {
        let cwd = temp_dir("real-cwd");
        let config_dir = temp_dir("real-cfg");
        // A tenth of whatever the window constant says — derived, so the pct
        // assertion below stays meaningful if D-054's number moves again.
        let tenth = WORKER_WINDOW_TOKENS / 10;
        seed_transcript(&config_dir, &cwd, &[user_line("hi"), assistant_line(u64::from(tenth), 0, 0)]);

        let source = TranscriptSource { harness: crate::placement::harness::claude_code(), config_dir, cwd };
        let gauge = sample_transcript(&source, WORKER_WINDOW_TOKENS).expect("a completed turn exists");
        assert_eq!(gauge.used_tokens, tenth);
        assert_eq!(gauge.window_tokens, WORKER_WINDOW_TOKENS);
        assert_eq!(gauge.pct, 10);
    }

    /// When more than one session file exists under a pane's project
    /// directory (the `/clear` case this sampler is deliberately defensive
    /// about), the most-recently-written one wins.
    #[test]
    fn the_most_recently_modified_transcript_file_wins() {
        let cwd = temp_dir("multi-cwd");
        let config_dir = temp_dir("multi-cfg");
        let source = TranscriptSource { harness: crate::placement::harness::claude_code(), config_dir: config_dir.clone(), cwd: cwd.clone() };
        let dir = project_dir(&source);
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(dir.join("older.jsonl"), assistant_line(90_000, 0, 0)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(dir.join("newer.jsonl"), assistant_line(1_000, 0, 0)).unwrap();

        let gauge = sample_transcript(&source, WORKER_WINDOW_TOKENS).expect("a transcript exists");
        assert_eq!(gauge.used_tokens, 1_000, "the newer session's usage must win, not the larger older one");
    }

    #[test]
    fn a_non_jsonl_file_in_the_project_dir_is_ignored() {
        let cwd = temp_dir("stray-cwd");
        let config_dir = temp_dir("stray-cfg");
        let source = TranscriptSource { harness: crate::placement::harness::claude_code(), config_dir: config_dir.clone(), cwd: cwd.clone() };
        let dir = project_dir(&source);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.txt"), "not a transcript").unwrap();

        assert_eq!(sample_transcript(&source, WORKER_WINDOW_TOKENS), None);
    }

    // --- GaugeSources ------------------------------------------------------------

    // --- checkpoint 11's two unavailable cases ------------------------------------
    //
    // Neither is reachable through a registered harness — Claude Code reads its
    // own transcript and the fleet asserts its window — so they are driven
    // through stand-ins that answer checkpoint 11 differently and are otherwise
    // Claude Code. They are deliberately **not** registered: `registered()` still
    // holds exactly one, and what these stand for is a later vendor's answer.
    // Without them D-054's rule is only asserted for the harness that happens to
    // satisfy it, which is not an assertion about the seam at all.

    mod stand_ins {
        use crate::placement::harness::{
            claude_code, GaugeSource, Harness, HarnessSpec, CLAUDE_CODE_SPEC,
        };
        use std::path::Path;
        use std::sync::LazyLock;

        /// A harness that publishes its own context window instead of letting the
        /// fleet assert one — a strictly better answer than D-054's, and the one
        /// [`GaugeSource::window_tokens`](crate::placement::harness::GaugeSource::window_tokens)
        /// documents `None` as meaning.
        #[derive(Debug)]
        pub struct PublishesItsOwnWindow;
        static NO_ASSERTED_WINDOW: LazyLock<HarnessSpec> = LazyLock::new(|| HarnessSpec {
            name: "stand-in-publishes-its-own-window",
            gauge: GaugeSource {
                reads_transcript: true,
                window_tokens: None,
                window_env: None,
            },
            ..CLAUDE_CODE_SPEC
        });

        /// A harness whose usage lives somewhere this module has no reader for.
        #[derive(Debug)]
        pub struct KeepsUsageElsewhere;
        static USAGE_ELSEWHERE: LazyLock<HarnessSpec> = LazyLock::new(|| HarnessSpec {
            name: "stand-in-keeps-usage-elsewhere",
            gauge: GaugeSource {
                reads_transcript: false,
                window_tokens: Some(super::WORKER_WINDOW_TOKENS),
                window_env: Some("A_WINDOW_IT_DOES_TELL_THE_PANE_ABOUT"),
            },
            ..CLAUDE_CODE_SPEC
        });

        macro_rules! delegate {
            ($ty:ty, $spec:ident) => {
                impl Harness for $ty {
                    fn spec(&self) -> &'static HarnessSpec {
                        &$spec
                    }
                    fn project_key(&self, cwd: &Path) -> String {
                        claude_code().project_key(cwd)
                    }
                    fn seed_config_dir(&self, dir: &Path, cwd: &Path) -> Result<(), String> {
                        claude_code().seed_config_dir(dir, cwd)
                    }
                    fn command_args(&self, brief: &str, mode: Option<&str>) -> Vec<String> {
                        claude_code().command_args(brief, mode)
                    }
                }
            };
        }
        delegate!(PublishesItsOwnWindow, NO_ASSERTED_WINDOW);
        delegate!(KeepsUsageElsewhere, USAGE_ELSEWHERE);

        pub static PUBLISHES_ITS_OWN_WINDOW: PublishesItsOwnWindow = PublishesItsOwnWindow;
        pub static KEEPS_USAGE_ELSEWHERE: KeepsUsageElsewhere = KeepsUsageElsewhere;
    }

    /// A harness that does not let the fleet assert a window has **no window to
    /// divide by**, and the gauge says unavailable rather than borrowing the one
    /// number it happens to have lying around.
    ///
    /// The transcript is real and the usage in it is readable — read through
    /// Claude Code the identical files sample at 10% — so what is being asserted
    /// is not "nothing was found". It is that a reading the gauge could produce
    /// is withheld because the denominator would be another harness's. That
    /// number would look right and be wrong, which is the one thing D-054 forbids
    /// absolutely.
    #[test]
    fn a_harness_that_asserts_no_window_samples_as_absent_rather_than_borrowing_one() {
        let cwd = temp_dir("no-window-cwd");
        let config_dir = temp_dir("no-window-cfg");
        let tenth = WORKER_WINDOW_TOKENS / 10;
        seed_transcript(&config_dir, &cwd, &[assistant_line(u64::from(tenth), 0, 0)]);

        let sources = GaugeSources::default();
        sources.record(
            PaneId::Worker(1),
            TranscriptSource {
                harness: &stand_ins::PUBLISHES_ITS_OWN_WINDOW,
                config_dir: config_dir.clone(),
                cwd: cwd.clone(),
            },
        );
        sources.record(
            PaneId::Worker(2),
            TranscriptSource {
                harness: crate::placement::harness::claude_code(),
                config_dir,
                cwd,
            },
        );

        assert_eq!(
            sources.sample(PaneId::Worker(2)).map(|g| g.pct),
            Some(10),
            "the same transcript is readable, so the assertion below is about the window",
        );
        assert_eq!(
            sources.sample(PaneId::Worker(1)),
            None,
            "a pane whose harness asserts no window was given one anyway — an unavailable \
             gauge says unavailable (D-054)",
        );
    }

    /// A harness whose usage is not in the pane's own transcript samples as
    /// absent too. There is a file at the path this module would read and it
    /// parses, which is exactly the trap: a gauge that read it anyway would
    /// report a confident number sourced from something that is not this
    /// harness's usage record.
    #[test]
    fn a_harness_that_keeps_usage_elsewhere_samples_as_absent_rather_than_reading_a_transcript() {
        let cwd = temp_dir("elsewhere-cwd");
        let config_dir = temp_dir("elsewhere-cfg");
        seed_transcript(&config_dir, &cwd, &[assistant_line(u64::from(WORKER_WINDOW_TOKENS / 10), 0, 0)]);

        let sources = GaugeSources::default();
        sources.record(
            PaneId::Worker(1),
            TranscriptSource {
                harness: &stand_ins::KEEPS_USAGE_ELSEWHERE,
                config_dir,
                cwd,
            },
        );

        assert_eq!(
            sources.sample(PaneId::Worker(1)),
            None,
            "this harness's usage is not in that file, and reading it anyway is a number \
             that looks right and is wrong",
        );
    }

    #[test]
    fn an_unrecorded_pane_samples_as_absent() {
        let sources = GaugeSources::default();
        assert_eq!(sources.sample(PaneId::Worker(1)), None);
        assert_eq!(sources.sample(PaneId::Orch), None, "orch is never recorded, ever");
    }

    #[test]
    fn a_recorded_pane_samples_its_own_transcript_and_no_other_panes() {
        let cwd = temp_dir("gs-cwd");
        let config_dir = temp_dir("gs-cfg");
        seed_transcript(&config_dir, &cwd, &[assistant_line(u64::from(WORKER_WINDOW_TOKENS / 20), 0, 0)]);

        let sources = GaugeSources::default();
        sources.record(PaneId::Worker(2), TranscriptSource { harness: crate::placement::harness::claude_code(), config_dir, cwd });

        assert_eq!(sources.sample(PaneId::Worker(2)).map(|g| g.pct), Some(5));
        assert_eq!(sources.sample(PaneId::Worker(3)), None, "an unrecorded peer stays absent");
    }

    // --- estimate_tokens / spawn_estimate_notice_text -----------------------------

    #[test]
    fn estimate_tokens_is_chars_over_four_rounded_up() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2, "a fifth char must round up, not truncate to 1");
    }

    #[test]
    fn a_worker_spawn_notice_names_the_pane_the_tokens_and_the_window_percent() {
        let text = spawn_estimate_notice_text(PaneId::Worker(2), &"x".repeat(4_000), Some(WORKER_WINDOW_TOKENS));
        assert!(text.contains("worker-2"));
        assert!(text.contains("≈1000-token brief") || text.contains("1000-token"), "{text}");
        assert!(text.contains('%'), "a worker's notice states a window percent: {text}");
    }

    /// Orch's window is deliberately unknown to this fleet, so its notice
    /// states the brief size and stops — no percent it would have to invent.
    #[test]
    fn an_orch_spawn_notice_has_no_window_percent() {
        let text = spawn_estimate_notice_text(PaneId::Orch, &"x".repeat(400), None);
        assert!(text.contains("orch"));
        assert!(!text.contains('%'), "orch's window is not tracked, so no percent may appear: {text}");
    }

    // --- the 80% notice ------------------------------------------------------------

    #[test]
    fn crosses_notice_threshold_is_exact_at_the_boundary() {
        assert!(!crosses_notice_threshold(79));
        assert!(crosses_notice_threshold(80));
        assert!(crosses_notice_threshold(100));
    }

    #[test]
    fn notice_text_names_the_pane_and_proposes_without_acting() {
        // Derived, not hardcoded: exactly 80% of whatever the window constant
        // says today, so this test keeps testing the threshold if D-054's
        // number moves again.
        let gauge = ContextGauge::new(WORKER_WINDOW_TOKENS * 8 / 10, WORKER_WINDOW_TOKENS);
        let text = notice_text(PaneId::Worker(4), &gauge);
        assert!(text.contains("worker-4"));
        assert!(text.contains("80%") || text.contains(&gauge.pct.to_string()));
        assert!(text.contains("consider"), "it proposes, it does not command: {text}");
        assert!(text.contains("may lag"), "the honesty label the requirements doc requires: {text}");
    }
}
