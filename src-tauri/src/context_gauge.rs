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
use std::path::PathBuf;
use std::sync::Mutex;

use fleetor_core::pane::{ContextGauge, PaneId};

/// The context window Claude Code runs a model slug at, on the Anthropic API
/// (measured against its docs and the 2.1.307 binary, 2026-10-21). Keyed by
/// window class, not by a version list: aliases move with Claude Code releases.
///
/// A seat with no model runs the plan's default model at 1M; a slug Claude Code
/// does not recognise runs at 200k (D-125).
pub fn model_window_tokens(model: Option<&str>) -> u32 {
    const LARGE: u32 = 1_000_000;
    const STANDARD: u32 = 200_000;
    let Some(slug) = model else { return LARGE };
    if slug.contains("[1m]") || slug.contains("1m-") {
        return LARGE;
    }
    let Some(rest) = slug.strip_prefix("claude-") else {
        return if crate::placement::claude_code::claude_code_recognises(slug) {
            LARGE
        } else {
            STANDARD
        };
    };
    let mut parts = rest.split('-');
    let family = parts.next().unwrap_or_default();
    let mut version = parts.map_while(|part| part.parse::<u32>().ok().filter(|n| *n < 100));
    let major = version.next().unwrap_or(0);
    let minor = version.next().unwrap_or(0);
    match family {
        "fable" => LARGE,
        "opus" if major >= 5 || (major == 4 && minor >= 7) => LARGE,
        "sonnet" | "haiku" if major >= 5 => LARGE,
        _ => STANDARD,
    }
}

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
    /// Per-seat window override, from `model_window_tokens` at spawn.
    pub window_tokens: Option<u32>,
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
    /// **Both numbers come from the pane's own harness** (WP-25, checkpoint 11;
    /// #41, C61, C65), and every way either can be absent ends in the same
    /// answer. A harness whose usage this module has no reader for
    /// ([`reads_transcript`](crate::placement::harness::GaugeSource::reads_transcript)
    /// `== false`); a harness that has a reader but finds nothing the vendor
    /// wrote ([`Harness::read_usage`](crate::placement::harness::Harness::read_usage)
    /// `== None`); and a harness with a real numerator but no window from
    /// either the vendor's own record or
    /// ([`window_tokens`](crate::placement::harness::GaugeSource::window_tokens))
    /// the fleet — all three sample as **absent**. That is D-054's rule applied
    /// to the seam rather than to one vendor: an unavailable gauge says
    /// unavailable, and the one thing it may never do is divide by a window
    /// borrowed from a harness that is not this pane's — a number that looks
    /// right and is wrong.
    ///
    /// **The window the vendor reported wins over the one the fleet asserts.**
    /// Codex publishes `context_window = 1048576` in its catalog and then tells
    /// its own pane `model_context_window = 996147` for the same model in the
    /// same run — exactly 95% of it (C61). Dividing by the larger figure would
    /// put a percentage in the rail that disagrees with the percentage the
    /// operator can see inside codex, which is the two-numbers disagreement
    /// D-054 exists to prevent. So [`UsageReading::window_tokens`] is consulted
    /// first and the spec's constant is only the fallback.
    pub fn sample(&self, pane: PaneId) -> Option<ContextGauge> {
        let source = self.0.lock().ok()?.get(&pane).cloned()?;
        let gauge = &source.harness.spec().gauge;
        if !gauge.reads_transcript {
            return None;
        }
        let reading = source.harness.read_usage(&source)?;
        let window = reading.window_tokens.or(source.window_tokens).or(gauge.window_tokens)?;
        Some(ContextGauge::new(reading.used_tokens, window))
    }
}

/// One harness's own accounting of one of its own turns — the numerator, and
/// the window the vendor said it was measured against.
///
/// **Everything on it was written to disk by the vendor.** Nothing here is
/// derived from the prompt FLEETOR assembled; a harness with no such record
/// returns `None` rather than constructing one of these (D-054, M22, C24).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsageReading {
    /// How full the pane's context is, as the vendor itself counted it.
    pub used_tokens: u32,
    /// The window *the vendor reported for this turn*, when it reports one.
    ///
    /// `None` means the harness stated no window and the fleet's own
    /// [`GaugeSource::window_tokens`](crate::placement::harness::GaugeSource::window_tokens)
    /// applies — which is Claude Code's case, and the reason D-054's constant
    /// still exists. It is never filled in with another harness's number.
    pub window_tokens: Option<u32>,
}

// --- the transcript sampler ------------------------------------------------------

/// The pre-#41 shape of the sampler — transcript reading and division in one
/// call — kept because six tests written against it are the ones that pin the
/// honest-sum finding, the `/clear` rule and the two absent cases, and a test
/// rewritten alongside the code it guards guards less.
///
/// Test-only now: production divides in [`GaugeSources::sample`], where the
/// vendor's own reported window gets to win over the fleet's constant.
#[cfg(test)]
fn sample_transcript(source: &TranscriptSource, window_tokens: u32) -> Option<ContextGauge> {
    let reading = crate::placement::claude_code::jsonl_usage(source)?;
    Some(ContextGauge::new(reading.used_tokens, reading.window_tokens.unwrap_or(window_tokens)))
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
pub fn spawn_estimate_notice_text(
    pane: PaneId,
    rendered_brief: &str,
    window_tokens: Option<u32>,
) -> String {
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
    use std::path::Path;

    const TEST_WINDOW: u32 = 1_000_000;

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
        serde_json::json!({"type": "user", "message": {"role": "user", "content": text}})
            .to_string()
    }

    fn seed_transcript(config_dir: &Path, cwd: &Path, lines: &[String]) {
        let source = TranscriptSource {
            harness: crate::placement::harness::claude_code(),
            config_dir: config_dir.to_path_buf(),
            cwd: cwd.to_path_buf(),
            window_tokens: None,
        };
        let dir = crate::placement::claude_code::project_dir(&source);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("session-a.jsonl"), lines.join("\n")).unwrap();
    }

    // --- project_dir / the slug rule -------------------------------------------

    // --- sample_transcript / latest_transcript ----------------------------------

    #[test]
    fn no_transcript_directory_at_all_samples_as_absent() {
        let source = TranscriptSource {
            harness: crate::placement::harness::claude_code(),
            config_dir: temp_dir("no-dir-cfg"),
            cwd: temp_dir("no-dir-cwd"),
            window_tokens: None,
        };
        assert_eq!(sample_transcript(&source, TEST_WINDOW), None);
    }

    #[test]
    fn a_transcript_with_no_completed_turn_samples_as_absent_not_zero() {
        let cwd = temp_dir("empty-turn-cwd");
        let config_dir = temp_dir("empty-turn-cfg");
        seed_transcript(&config_dir, &cwd, &[user_line("are you there")]);

        let source = TranscriptSource {
            harness: crate::placement::harness::claude_code(),
            config_dir,
            cwd,
            window_tokens: None,
        };
        assert_eq!(sample_transcript(&source, TEST_WINDOW), None, "no assistant reply yet");
    }

    #[test]
    fn a_real_transcript_samples_a_gauge_against_the_window() {
        let cwd = temp_dir("real-cwd");
        let config_dir = temp_dir("real-cfg");
        // A tenth of the test window, so the pct assertion derives from it.
        let tenth = TEST_WINDOW / 10;
        seed_transcript(
            &config_dir,
            &cwd,
            &[user_line("hi"), assistant_line(u64::from(tenth), 0, 0)],
        );

        let source = TranscriptSource {
            harness: crate::placement::harness::claude_code(),
            config_dir,
            cwd,
            window_tokens: None,
        };
        let gauge = sample_transcript(&source, TEST_WINDOW).expect("a completed turn exists");
        assert_eq!(gauge.used_tokens, tenth);
        assert_eq!(gauge.window_tokens, TEST_WINDOW);
        assert_eq!(gauge.pct, 10);
    }

    /// When more than one session file exists under a pane's project
    /// directory (the `/clear` case this sampler is deliberately defensive
    /// about), the most-recently-written one wins.
    #[test]
    fn the_most_recently_modified_transcript_file_wins() {
        let cwd = temp_dir("multi-cwd");
        let config_dir = temp_dir("multi-cfg");
        let source = TranscriptSource {
            harness: crate::placement::harness::claude_code(),
            config_dir: config_dir.clone(),
            cwd: cwd.clone(),
            window_tokens: None,
        };
        let dir = crate::placement::claude_code::project_dir(&source);
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(dir.join("older.jsonl"), assistant_line(90_000, 0, 0)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(dir.join("newer.jsonl"), assistant_line(1_000, 0, 0)).unwrap();

        let gauge = sample_transcript(&source, TEST_WINDOW).expect("a transcript exists");
        assert_eq!(
            gauge.used_tokens, 1_000,
            "the newer session's usage must win, not the larger older one"
        );
    }

    #[test]
    fn a_non_jsonl_file_in_the_project_dir_is_ignored() {
        let cwd = temp_dir("stray-cwd");
        let config_dir = temp_dir("stray-cfg");
        let source = TranscriptSource {
            harness: crate::placement::harness::claude_code(),
            config_dir: config_dir.clone(),
            cwd: cwd.clone(),
            window_tokens: None,
        };
        let dir = crate::placement::claude_code::project_dir(&source);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.txt"), "not a transcript").unwrap();

        assert_eq!(sample_transcript(&source, TEST_WINDOW), None);
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
        use crate::placement::claude_code::CLAUDE_CODE_SPEC;
        use crate::placement::harness::{claude_code, GaugeSource, Harness, HarnessSpec, Seed};
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
            gauge: GaugeSource { reads_transcript: true, window_tokens: None },
            ..CLAUDE_CODE_SPEC
        });

        /// A harness whose usage lives somewhere this module has no reader for.
        #[derive(Debug)]
        pub struct KeepsUsageElsewhere;
        static USAGE_ELSEWHERE: LazyLock<HarnessSpec> = LazyLock::new(|| HarnessSpec {
            name: "stand-in-keeps-usage-elsewhere",
            gauge: GaugeSource { reads_transcript: false, window_tokens: Some(super::TEST_WINDOW) },
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
                    fn seed_config_dir(
                        &self,
                        seed: &Seed<'_>,
                    ) -> Result<Vec<(fleetor_core::event::NoticeLevel, String)>, String> {
                        claude_code().seed_config_dir(seed)
                    }
                    fn install_guardrail(
                        &self,
                        at: &crate::guardrail::GuardrailPlacement<'_>,
                    ) -> Result<Vec<(fleetor_core::event::NoticeLevel, String)>, String> {
                        claude_code().install_guardrail(at)
                    }
                    fn command_args(
                        &self,
                        seat: &crate::placement::harness::Seat<'_>,
                    ) -> Vec<String> {
                        claude_code().command_args(seat)
                    }
                    fn resume_args(
                        &self,
                        seat: &crate::placement::harness::Seat<'_>,
                        session_id: &str,
                    ) -> Option<Vec<String>> {
                        claude_code().resume_args(seat, session_id)
                    }
                    fn session_id(&self, seat_dir: &Path) -> Option<String> {
                        claude_code().session_id(seat_dir)
                    }
                    fn has_session(&self, seat_dir: &Path, session_id: &str) -> bool {
                        claude_code().has_session(seat_dir, session_id)
                    }
                    fn diagnose(
                        &self,
                        facts: &crate::placement::harness::MachineFacts<'_>,
                    ) -> crate::placement::harness::HarnessReadiness {
                        claude_code().diagnose(facts)
                    }
                    fn read_usage(
                        &self,
                        source: &super::TranscriptSource,
                    ) -> Option<super::UsageReading> {
                        claude_code().read_usage(source)
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
        let tenth = TEST_WINDOW / 10;
        seed_transcript(&config_dir, &cwd, &[assistant_line(u64::from(tenth), 0, 0)]);

        let sources = GaugeSources::default();
        sources.record(
            PaneId::Worker(1),
            TranscriptSource {
                harness: &stand_ins::PUBLISHES_ITS_OWN_WINDOW,
                config_dir: config_dir.clone(),
                cwd: cwd.clone(),
                window_tokens: None,
            },
        );
        sources.record(
            PaneId::Worker(2),
            TranscriptSource {
                harness: crate::placement::harness::claude_code(),
                config_dir,
                cwd,
                window_tokens: None,
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
        seed_transcript(&config_dir, &cwd, &[assistant_line(u64::from(TEST_WINDOW / 10), 0, 0)]);

        let sources = GaugeSources::default();
        sources.record(
            PaneId::Worker(1),
            TranscriptSource {
                harness: &stand_ins::KEEPS_USAGE_ELSEWHERE,
                config_dir,
                cwd,
                window_tokens: None,
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
        seed_transcript(&config_dir, &cwd, &[assistant_line(u64::from(TEST_WINDOW / 20), 0, 0)]);

        let sources = GaugeSources::default();
        sources.record(
            PaneId::Worker(2),
            TranscriptSource {
                harness: crate::placement::harness::claude_code(),
                config_dir,
                cwd,
                window_tokens: None,
            },
        );

        assert_eq!(sources.sample(PaneId::Worker(2)).map(|g| g.pct), Some(5));
        assert_eq!(sources.sample(PaneId::Worker(3)), None, "an unrecorded peer stays absent");
    }

    // --- codex: the vendor's own number, or unavailable ----------------------------
    //
    // Fabricated end to end. Nothing here spends a token, launches a binary or
    // reads the operator's own `~/.codex` — the layout is planted in a scratch
    // directory in the shape `docs/notes/codex-usage-notes.md` measured, and #38
    // is the only arm in this arc that was ever authorised to spend.

    /// The window codex reported to its own pane in the measured run.
    const REPORTED_WINDOW: u32 = 996_147;
    /// What the catalog published for the same model in the same run — 95% larger,
    /// and the figure a gauge must *not* silently divide by (C61).
    const CATALOG_WINDOW: u32 = 1_048_576;

    /// A live `state_5.sqlite` naming one rollout, plus the running thread total
    /// that sits in the very same row and must never become a numerator.
    ///
    /// **The connection is returned and the caller must hold it** — #39's fixture
    /// rule. Closing the last one checkpoints the write-ahead log into the main
    /// file and unlinks the `-shm`, and a live WAL database with its `-shm` beside
    /// it is precisely the state a codex pane's home is in while the gauge samples
    /// it. It is also what makes the read-only open in [`latest_rollout`] the real
    /// case rather than a convenient one.
    fn plant_state_db(home: &Path, rollout: &Path, running_total: u64) -> rusqlite::Connection {
        let conn = rusqlite::Connection::open(home.join("state_5.sqlite")).unwrap();
        conn.pragma_update(None, "journal_mode", "WAL").unwrap();
        conn.execute("CREATE TABLE threads (id TEXT, tokens_used INTEGER, rollout_path TEXT)", [])
            .unwrap();
        conn.execute(
            "INSERT INTO threads (id, tokens_used, rollout_path) VALUES (?1, ?2, ?3)",
            rusqlite::params![
                "01a0779e-9ae7-7181-ad8f-596038c2a862",
                running_total,
                rollout.to_string_lossy()
            ],
        )
        .unwrap();
        conn
    }

    /// One `token_usage_record`, in the shape the note quotes verbatim — including
    /// the `thread_token_usage` sibling, which is what makes the running-total
    /// trap assertable at all.
    fn token_usage_record(turn_total: u64, thread_total: u64) -> String {
        serde_json::json!({
            "type": "token_usage_record",
            "payload": {
                "thread_id": "01a0779e-9ae7-7181-ad8f-596038c2a862",
                "turn_id": "01a0779e-9af8-7903-a7c8-e00ae99ae884",
                "usage": {
                    "input_tokens": turn_total - 15, "cached_input_tokens": 0,
                    "cache_write_input_tokens": 0, "output_tokens": 15,
                    "reasoning_output_tokens": 13, "total_tokens": turn_total
                },
                "turn_token_usage": { "total_tokens": turn_total },
                "thread_token_usage": { "total_tokens": thread_total }
            }
        })
        .to_string()
    }

    /// The `token_count` event codex writes one line later, carrying the window it
    /// told the pane about.
    fn token_count_event(window: u32) -> String {
        serde_json::json!({
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {
                    "total_token_usage": { "total_tokens": 0 },
                    "last_token_usage": { "total_tokens": 0 },
                    "model_context_window": window
                }
            }
        })
        .to_string()
    }

    /// A rollout at the vendor's own dated depth, so the fixture exercises the
    /// join rather than a path this module could have guessed.
    fn plant_rollout(home: &Path, name: &str, lines: &[String]) -> PathBuf {
        let dir = home.join("sessions/2026/09/06");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("rollout-{name}.jsonl"));
        std::fs::write(&path, lines.join("\n")).unwrap();
        path
    }

    fn plant_catalog(home: &Path, windows: &[u32]) {
        let models: Vec<serde_json::Value> = windows
            .iter()
            .enumerate()
            .map(|(i, w)| serde_json::json!({"id": format!("model-{i}"), "context_window": w}))
            .collect();
        std::fs::write(
            home.join("models.json"),
            serde_json::json!({ "models": models }).to_string(),
        )
        .unwrap();
    }

    fn codex_pane(home: &Path) -> GaugeSources {
        let sources = GaugeSources::default();
        sources.record(
            PaneId::Worker(1),
            TranscriptSource {
                harness: crate::placement::codex::codex(),
                config_dir: home.to_path_buf(),
                cwd: home.join("worktree"),
                window_tokens: None,
            },
        );
        sources
    }

    /// The whole of #41's positive answer, through the seam: a codex pane's gauge
    /// is the vendor's own per-turn figure over the vendor's own reported window.
    ///
    /// **And the second assertion is the one C61 warned about.** The same
    /// numerator over the catalog's `context_window` is a different percentage —
    /// 47 rather than 50 — and it is the one an operator looking at codex's own
    /// display would not recognize. Pinning both is what makes "the reported
    /// window divides" a property rather than a coincidence of one fixture.
    #[test]
    fn a_codex_pane_divides_by_the_window_the_vendor_reported_not_the_one_it_published() {
        let home = temp_dir("codex-window");
        let rollout = plant_rollout(
            &home,
            "one",
            &[token_usage_record(500_000, 500_000), token_count_event(REPORTED_WINDOW)],
        );
        plant_catalog(&home, &[CATALOG_WINDOW]);
        let _state = plant_state_db(&home, &rollout, 500_000);

        let gauge = codex_pane(&home).sample(PaneId::Worker(1)).expect("the vendor wrote a turn");
        assert_eq!(gauge.used_tokens, 500_000, "the numerator is the vendor's own total_tokens");
        assert_eq!(gauge.window_tokens, REPORTED_WINDOW, "the window codex told its own pane");
        assert_eq!(gauge.pct, 50);

        let by_catalog = ContextGauge::new(500_000, CATALOG_WINDOW);
        assert_eq!(by_catalog.pct, 47, "the fixture only pins anything if the two figures differ");
        assert_ne!(
            gauge.pct, by_catalog.pct,
            "the catalog's 1048576 is what divided — a rail that disagrees with the vendor's \
             own display by 5% is the two-numbers failure D-054 forbids (C61)",
        );
    }

    /// The catalog is the fallback and reachable, so the acceptance criterion
    /// "the window comes from the harness's own published catalog, not a FLEETOR
    /// constant" is satisfied by a path that runs rather than by an argument.
    #[test]
    fn with_no_reported_window_the_harnesss_own_catalog_divides_and_no_fleet_constant_does() {
        let home = temp_dir("codex-catalog");
        let rollout = plant_rollout(&home, "one", &[token_usage_record(500_000, 500_000)]);
        plant_catalog(&home, &[CATALOG_WINDOW, CATALOG_WINDOW]);
        let _state = plant_state_db(&home, &rollout, 500_000);

        let gauge =
            codex_pane(&home).sample(PaneId::Worker(1)).expect("a turn with no token_count");
        assert_eq!(gauge.window_tokens, CATALOG_WINDOW);
        assert_eq!(gauge.pct, 47);
        assert_ne!(
            gauge.window_tokens, TEST_WINDOW,
            "a claude window must never divide a codex reading",
        );
    }

    /// A catalog that publishes different windows for different models cannot be
    /// resolved to this pane's, because nothing on [`TranscriptSource`] says which
    /// model the seat runs. Picking one would be a guess wearing a measurement's
    /// clothes.
    #[test]
    fn a_catalog_whose_models_disagree_reads_unavailable_rather_than_picking_one() {
        let home = temp_dir("codex-catalog-split");
        let rollout = plant_rollout(&home, "one", &[token_usage_record(500_000, 500_000)]);
        plant_catalog(&home, &[CATALOG_WINDOW, 200_000]);
        let _state = plant_state_db(&home, &rollout, 500_000);

        assert_eq!(
            codex_pane(&home).sample(PaneId::Worker(1)),
            None,
            "two published windows and no way to say which is this pane's",
        );
    }

    /// **The running-total trap.** `threads.tokens_used` and the rollout's
    /// `thread_token_usage` are both cumulative across a thread; a gauge that used
    /// either would climb past its window while the pane's actual context stayed
    /// small. Only the last turn's own figure is a context reading.
    #[test]
    fn the_numerator_is_the_last_turns_figure_never_the_threads_running_total() {
        let home = temp_dir("codex-running-total");
        let rollout = plant_rollout(
            &home,
            "one",
            &[
                token_usage_record(120_000, 120_000),
                token_usage_record(200_000, 320_000),
                token_count_event(REPORTED_WINDOW),
            ],
        );
        plant_catalog(&home, &[CATALOG_WINDOW]);
        // The same running total the vendor keeps in the row this reads
        // `rollout_path` out of — deliberately a number that would look plausible.
        let _state = plant_state_db(&home, &rollout, 320_000);

        let gauge = codex_pane(&home).sample(PaneId::Worker(1)).expect("two completed turns");
        assert_eq!(
            gauge.used_tokens, 200_000,
            "the second turn's own total, not 320000 — a thread total is not an occupancy",
        );
    }

    /// A thread the vendor never wrote a usage record for reads **unavailable**,
    /// with a plausible running total sitting right there in the row the join went
    /// through. That figure is not synthesized from what FLEETOR sent — it is the
    /// vendor's — and it is still refused, because it does not answer the question
    /// the gauge asks.
    #[test]
    fn no_token_usage_record_reads_unavailable_even_though_a_plausible_number_is_at_hand() {
        let home = temp_dir("codex-no-record");
        let rollout = plant_rollout(&home, "one", &[token_count_event(REPORTED_WINDOW)]);
        plant_catalog(&home, &[CATALOG_WINDOW]);
        let _state = plant_state_db(&home, &rollout, 12_548);

        assert_eq!(
            codex_pane(&home).sample(PaneId::Worker(1)),
            None,
            "12548 is on disk and would have rendered as 1% — an unavailable gauge says \
             unavailable (D-054, M22, C24)",
        );
    }

    #[test]
    fn a_codex_home_with_no_state_database_reads_unavailable() {
        let home = temp_dir("codex-no-state");
        assert_eq!(codex_pane(&home).sample(PaneId::Worker(1)), None);
    }

    /// The vendor names the file; if that file is not there, there is nothing to
    /// read and nothing to guess. The `sessions/<yyyy>/<mm>/<dd>/` layout is never
    /// walked as a substitute.
    #[test]
    fn a_rollout_path_naming_a_file_that_is_not_there_reads_unavailable() {
        let home = temp_dir("codex-dangling");
        plant_catalog(&home, &[CATALOG_WINDOW]);
        let _state = plant_state_db(&home, &home.join("sessions/2026/09/06/gone.jsonl"), 12_548);

        assert_eq!(codex_pane(&home).sample(PaneId::Worker(1)), None);
    }

    /// A `/clear` can leave a second thread in one pane's home, and the ordering
    /// columns that would say which is current are not among the ones C61
    /// measured. Most-recently-written wins — the same rule, and the same reason,
    /// as [`latest_transcript`].
    #[test]
    fn the_most_recently_written_rollout_wins_when_a_home_holds_two() {
        let home = temp_dir("codex-two-threads");
        plant_catalog(&home, &[CATALOG_WINDOW]);
        let older = plant_rollout(
            &home,
            "older",
            &[token_usage_record(900_000, 900_000), token_count_event(REPORTED_WINDOW)],
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
        let newer = plant_rollout(
            &home,
            "newer",
            &[token_usage_record(100_000, 100_000), token_count_event(REPORTED_WINDOW)],
        );

        let conn = plant_state_db(&home, &older, 900_000);
        conn.execute(
            "INSERT INTO threads (id, tokens_used, rollout_path) VALUES (?1, ?2, ?3)",
            rusqlite::params!["second-thread", 100_000, newer.to_string_lossy()],
        )
        .unwrap();

        let gauge = codex_pane(&home).sample(PaneId::Worker(1)).expect("two threads, one current");
        assert_eq!(
            gauge.used_tokens, 100_000,
            "the newer thread's usage must win, not the larger older one",
        );
    }

    /// Absence is a decision this module makes, not a wire that was never
    /// connected: the identical fixture with and without the vendor's record
    /// samples a real number and then nothing.
    #[test]
    fn a_codex_gauge_is_present_or_absent_for_a_reason_and_the_same_home_shows_both() {
        let home = temp_dir("codex-both-ways");
        plant_catalog(&home, &[CATALOG_WINDOW]);
        let rollout = plant_rollout(&home, "one", &[token_count_event(REPORTED_WINDOW)]);
        let _state = plant_state_db(&home, &rollout, 500_000);
        assert_eq!(
            codex_pane(&home).sample(PaneId::Worker(1)),
            None,
            "no turn record yet, so nothing honest to report",
        );

        std::fs::write(
            &rollout,
            [token_usage_record(500_000, 500_000), token_count_event(REPORTED_WINDOW)].join("\n"),
        )
        .unwrap();
        assert_eq!(
            codex_pane(&home).sample(PaneId::Worker(1)).map(|g| g.pct),
            Some(50),
            "the same home, once the vendor has written a turn",
        );
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
        let text =
            spawn_estimate_notice_text(PaneId::Worker(2), &"x".repeat(4_000), Some(TEST_WINDOW));
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
        assert!(
            !text.contains('%'),
            "orch's window is not tracked, so no percent may appear: {text}"
        );
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
        // Derived: exactly 80% of the test window.
        let gauge = ContextGauge::new(TEST_WINDOW * 8 / 10, TEST_WINDOW);
        let text = notice_text(PaneId::Worker(4), &gauge);
        assert!(text.contains("worker-4"));
        assert!(text.contains("80%") || text.contains(&gauge.pct.to_string()));
        assert!(text.contains("consider"), "it proposes, it does not command: {text}");
        assert!(
            text.contains("may lag"),
            "the honesty label the requirements doc requires: {text}"
        );
    }

    // --- model_window_tokens ---------------------------------------------------------

    #[test]
    fn aliases_and_current_models_get_the_window_claude_code_runs_them_at() {
        for large in [
            "sonnet",
            "opus",
            "default",
            "opusplan",
            "haiku",
            "fable",
            "claude-sonnet-5-5",
            "claude-opus-5",
            "claude-opus-5-5",
            "claude-opus-4-7",
            "claude-fable-5-1",
            "claude-haiku-5-5",
        ] {
            assert_eq!(model_window_tokens(Some(large)), 1_000_000, "{large}");
        }
        for standard in
            ["claude-opus-4-6", "claude-sonnet-4-6", "claude-sonnet-4-5", "claude-haiku-4-5"]
        {
            assert_eq!(model_window_tokens(Some(standard)), 200_000, "{standard}");
        }
    }

    #[test]
    fn known_claude_models_get_200k() {
        assert_eq!(model_window_tokens(Some("claude-sonnet-4-20250514")), 200_000);
        assert_eq!(model_window_tokens(Some("claude-opus-4-20250514")), 200_000);
        assert_eq!(model_window_tokens(Some("claude-haiku-4-5-20251001")), 200_000);
    }

    #[test]
    fn extended_context_models_get_1m() {
        assert_eq!(model_window_tokens(Some("claude-opus-4-20250514[1m]")), 1_000_000);
    }

    #[test]
    fn unknown_models_fall_back_to_the_default_constant() {
        assert_eq!(model_window_tokens(Some("deepseek-v4-flash-preview")), 200_000);
        assert_eq!(model_window_tokens(Some("deepseek-v4-flash")), 200_000);
    }

    #[test]
    fn no_model_runs_the_plan_default_at_one_million() {
        assert_eq!(model_window_tokens(None), 1_000_000);
    }

    #[test]
    fn per_seat_window_overrides_the_spec_constant_in_the_gauge() {
        let cwd = temp_dir("override-cwd");
        let config_dir = temp_dir("override-cfg");
        let tokens = 200_000u32;
        let used = tokens / 5; // 20%
        seed_transcript(&config_dir, &cwd, &[assistant_line(u64::from(used), 0, 0)]);

        let sources = GaugeSources::default();
        sources.record(
            PaneId::Worker(1),
            TranscriptSource {
                harness: crate::placement::harness::claude_code(),
                config_dir,
                cwd,
                window_tokens: Some(tokens),
            },
        );

        let gauge = sources.sample(PaneId::Worker(1)).expect("a completed turn");
        assert_eq!(gauge.window_tokens, tokens);
        assert_eq!(gauge.pct, 20);
    }
}
