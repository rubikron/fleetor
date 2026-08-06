//! `fleet cmd` — a slash command run in a pane's terminal, and why (D-045).
//!
//! **A command is not a message, and nothing here is a transform on one.** The
//! original ask was phrased "the message router must chop the prefix off when
//! typing into workers", which is a content-inspecting rewrite *inside* the
//! delivery path — the shape Tier 1.4 has rejected twice. This is the other
//! design: a separate verb, a separate op, a separate delivery arm, and a body
//! that was never framed in the first place. [`message`](crate::message) is
//! untouched by this module and must stay that way.
//!
//! Two things make that safe to point at a live TUI:
//!
//!  1. **An allowlist, checked at accept time.** `/clear` and `/compact` and
//!     nothing else. The check is a fact against a constant — the same refusal
//!     class as the self-send guard, which D-034 explicitly keeps. Once a command
//!     is accepted nothing may delay, drop or alter it.
//!  2. **Control characters are refused, not stripped.** `Message::sanitize`
//!     drops them, because a mangled message that lands beats a clean one that
//!     doesn't. A command is the opposite: the vocabulary is two words long, and
//!     a mangled command is a *different* command. So a body carrying an escape
//!     — the bracketed-paste breakout `\x1b[201~` above all — is refused with the
//!     reason on the sender's stderr rather than quietly repaired.
//!
//! The `why` is mandatory and travels into the log. That is the point of the
//! verb as much as the command is: the record of *when and why* the fleet decided
//! to clear or compact is the reasoning chain a later self-improvement pass reads.
//!
//! What the bytes do at the other end is measured in
//! `docs/command-channel-notes.md`, not assumed. The short version: the command
//! is delivered by the same bracketed paste every message uses, **unframed**, so
//! `/` lands in column 0 — and `accepted` still means only that the bytes reached
//! a live pty.

use crate::event::FleetEvent;
use crate::pane::PaneId;
use serde::{Deserialize, Serialize};

/// Every slash command a pane may run in another pane's terminal.
///
/// Tier 2: widen it in a `decisions.md` entry, not in passing. The boundary is
/// the whole safety argument — `/model` would silently change what a pane costs,
/// and `/exit` would kill it while every `fleet send` still reported success.
pub const ALLOWED_COMMANDS: [&str; 2] = ["/clear", "/compact"];

/// One command sent to a pane's terminal. The sibling of
/// [`Message`](crate::message::Message), deliberately not a variant of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Command {
    pub id: String,
    pub from: PaneId,
    pub to: PaneId,
    /// The exact command, trimmed and already checked against
    /// [`ALLOWED_COMMANDS`] — this is both what is logged and what is typed.
    pub command: String,
    /// Why the sender decided to send it. Never empty: the CLI requires it and
    /// [`Command::new`] refuses without it.
    pub why: String,
    pub ts: i64,
}

impl Command {
    /// Build a command, or say why it cannot be one.
    ///
    /// **This is the accept-time gate and the only one.** Everything downstream —
    /// the hub arm, the app command, the writer, the pty — takes what it is given
    /// and neither re-checks nor alters it.
    ///
    /// Self-targeting is allowed here, and only here: `from == to` is the *normal*
    /// case for `fleet cmd self "/compact …"`. The message self-send guard in the
    /// hub is untouched, because a pane messaging itself is still nonsense.
    pub fn new(
        from: PaneId,
        to: PaneId,
        command: impl AsRef<str>,
        why: impl AsRef<str>,
    ) -> Result<Self, String> {
        let command = check_command(command.as_ref())?;
        let why = check_why(why.as_ref())?;
        Ok(Self {
            id: crate::ids::new_id("cmd"),
            from,
            to,
            command,
            why,
            ts: crate::time::now_ms(),
        })
    }

    /// The bytes to type into the receiving pane: the command, **unframed**.
    ///
    /// No `[fleet · …]` prefix, and nothing else prepended, because a slash
    /// command is only a command when `/` is the first character in the input
    /// box (`docs/command-channel-notes.md` §3). This is the one place in the
    /// product where a delivery is deliberately not attributed to its sender —
    /// the attribution lives in the log instead.
    pub fn keystrokes(&self) -> &str {
        &self.command
    }

    /// Turn the record into its log entry. `accepted` means the target pane was
    /// live and the bytes were queued to its pty — never that the command *ran*.
    /// It may have been queued behind a turn, or landed after unsubmitted text
    /// and been swallowed as prose; neither is knowable from this side (L3).
    pub fn into_event(self, accepted: bool, detail: Option<String>) -> FleetEvent {
        FleetEvent::Command {
            id: self.id,
            from: self.from,
            to: self.to,
            command: self.command,
            why: self.why,
            accepted,
            detail,
        }
    }
}

/// The first word of a command — `/compact focus on X` is `/compact`.
fn command_word(command: &str) -> &str {
    command.split_whitespace().next().unwrap_or("")
}

/// Trim, then check: non-empty, control-free, slash-prefixed, allowlisted. Each
/// refusal is a sentence the sending model reads on its own stderr and can act
/// on, so each says what is allowed rather than only what was wrong.
fn check_command(raw: &str) -> Result<String, String> {
    let command = raw.trim();
    if command.is_empty() {
        return Err(format!("a command cannot be empty — {}", allowed()));
    }
    if command.chars().any(char::is_control) {
        return Err(format!(
            "the command contains a control character and was refused rather than repaired — \
             send it as plain text ({})",
            allowed()
        ));
    }
    if !command.starts_with('/') {
        return Err(format!(
            "{command:?} is not a slash command — {}. \
             To say something to a pane, use `fleet send` instead",
            allowed()
        ));
    }
    if !ALLOWED_COMMANDS.contains(&command_word(command)) {
        return Err(format!("`{}` is not an allowed command — {}", command_word(command), allowed()));
    }
    Ok(command.to_string())
}

fn check_why(raw: &str) -> Result<String, String> {
    let why = raw.trim();
    if why.is_empty() {
        return Err(
            "--why cannot be empty — say what made this the right moment, so the decision \
             is on the record and not just its effect"
                .to_string(),
        );
    }
    Ok(why.to_string())
}

fn allowed() -> String {
    format!("the fleet allows {}", ALLOWED_COMMANDS.join(" and "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(command: &str) -> Result<Command, String> {
        Command::new(PaneId::Orch, PaneId::Worker(2), command, "context is stale exploration")
    }

    /// The allowlist is matched on the command *word*, so arguments are free —
    /// which matters, because `/compact <what to keep>` is the form both briefs
    /// teach and the form that bypasses CC's menu ranking entirely.
    #[test]
    fn an_allowed_command_keeps_its_arguments() {
        let cmd = make("/compact focus on the current task").expect("allowed");
        assert_eq!(cmd.command, "/compact focus on the current task");
        assert_eq!(cmd.keystrokes(), "/compact focus on the current task");
        assert!(make("/clear").is_ok());
        assert!(make("/compact").is_ok(), "a bare command word is still the command word");
    }

    /// The boundary. Every one of these would be a live keystroke sequence at a
    /// `claude` prompt if it were let through, and each refusal names the fix.
    #[test]
    fn anything_outside_the_allowlist_is_refused_with_a_reason() {
        for refused in ["/model opus", "/exit", "/add-dir /", "/clearly", "/compactify"] {
            let why = make(refused).expect_err("must be refused");
            assert!(why.contains("/clear") && why.contains("/compact"), "{refused}: {why}");
        }
    }

    /// A body that is not a command at all is refused *and* pointed at the verb
    /// that would have worked — the model reads its own stderr and self-corrects.
    #[test]
    fn a_body_that_is_not_a_slash_command_is_pointed_at_fleet_send() {
        let why = make("please compact yourself").expect_err("must be refused");
        assert!(why.contains("fleet send"), "{why}");
        assert!(make("").is_err(), "an empty command would submit a bare newline");
        assert!(make("   ").is_err());
    }

    /// The one that matters most. `Message::sanitize` *drops* control characters
    /// because a mangled message that lands beats a clean one that doesn't; a
    /// command is the opposite, so this refuses instead. A `\x1b[201~` here would
    /// end the paste carrying it and turn its own tail into keystrokes.
    #[test]
    fn a_control_character_is_refused_rather_than_stripped() {
        for hostile in
            ["/clear\x1b[201~/exit", "/compact keep\rthis", "/compact a\nb", "/compact\tkeep this"]
        {
            let why = make(hostile).expect_err("must be refused");
            assert!(why.contains("control character"), "{hostile:?}: {why}");
        }
        // Only *inside* the command. Trailing whitespace is trimmed, not refused
        // — see the trimming test below.
        assert!(make("/clear\t").is_ok(), "a trailing tab is whitespace, not an attack");
    }

    /// Surrounding whitespace is forgiven, because a model quotes inconsistently
    /// and a refusal over a trailing space teaches nothing. What is trimmed is
    /// trimmed *before* acceptance, so the record and the bytes still agree.
    #[test]
    fn surrounding_whitespace_is_trimmed_before_the_command_is_accepted() {
        let cmd = make("  /compact keep the parser  ").expect("allowed");
        assert_eq!(cmd.command, "/compact keep the parser");
        assert_eq!(cmd.keystrokes(), cmd.command, "what is logged is what is typed");
    }

    /// The why is the reason the verb exists, so an empty one is a refusal like
    /// any other rather than a silently blank column in the log.
    #[test]
    fn a_command_with_no_why_is_refused() {
        let why = Command::new(PaneId::Orch, PaneId::Worker(1), "/clear", "   ")
            .expect_err("must be refused");
        assert!(why.contains("--why"), "the refusal names the flag to fix: {why}");
    }

    /// Self-targeting is the *ordinary* case for this verb: a worker deciding its
    /// own context is stale is the whole self-maintenance move the brief teaches.
    #[test]
    fn a_pane_may_command_itself() {
        let cmd = Command::new(PaneId::Worker(3), PaneId::Worker(3), "/compact keep T-4", "done")
            .expect("self-targeting is allowed for cmd");
        assert_eq!((cmd.from, cmd.to), (PaneId::Worker(3), PaneId::Worker(3)));
    }

    /// The command is never framed. A `[fleet · orch] /clear` would put the slash
    /// in column 11 and the TUI would read the whole line as prose.
    #[test]
    fn the_keystrokes_carry_no_fleet_prefix() {
        let cmd = make("/clear").expect("allowed");
        assert!(!cmd.keystrokes().contains("[fleet"), "{}", cmd.keystrokes());
        assert!(cmd.keystrokes().starts_with('/'), "the slash must be column 0");
    }

    #[test]
    fn the_event_carries_the_why_and_the_honest_outcome() {
        let cmd = make("/compact keep the parser").expect("allowed");
        let id = cmd.id.clone();
        let event = cmd.into_event(false, Some("worker-2 has exited".into()));
        assert_eq!(event.kind(), "command");
        let FleetEvent::Command { id: got, command, why, accepted, detail, .. } = event else {
            panic!("expected a command event");
        };
        assert_eq!(got, id);
        assert_eq!(command, "/compact keep the parser");
        assert_eq!(why, "context is stale exploration", "the reasoning chain is in the log");
        assert!(!accepted);
        assert_eq!(detail.as_deref(), Some("worker-2 has exited"));
    }

    #[test]
    fn ids_are_unique_and_say_what_they_are() {
        let a = make("/clear").unwrap();
        let b = make("/clear").unwrap();
        assert_ne!(a.id, b.id);
        assert!(a.id.starts_with("cmd"), "{}", a.id);
    }
}
