//! **Codex's typing profile, measured against a live vendor pane** (WP-25 phase
//! 2, #32; C10, C22, C26, C28, C35, C37, C44; D-034, Tier 1.4, Tier 1.5).
//!
//! Checkpoint 9 is four bytes-level answers — is the body bracketed, what opens
//! and closes the bracket, what submits it, and how long sits between closing and
//! submitting — and it is the one checkpoint whose subject is not on `Placed` at
//! all (C28). `tests/harness_conformance_8_14.rs` asserts it against a stand-in
//! pane program that echoes what it was given as a submitted line, which is what
//! makes it free and portable. **This file asserts the half a stand-in cannot
//! reach: what the *vendor's own* TUI does with those bytes.**
//!
//! ## The two arms, and why neither is the other's evidence
//!
//! **`one_paste_becomes_one_submitted_turn`** delivers a *multi-line* message
//! into a woken pane and requires every line to arrive in **one** request body.
//! Multi-line is the whole reason `bracketed_paste` is a field: unbracketed, the
//! first newline submits, and a three-line message becomes three turns — three
//! separate requests, the first carrying one line. The single body carrying all
//! three is `paste_start`, `paste_end` and `submit_bytes` all asserted at once,
//! at the profile's shipped 30 ms gap.
//!
//! **`a_paste_with_no_submit_byte_sits_in_the_composer_and_never_reaches_the
//! _wire`** is the control, and without it the arm above is vacuous. Codex ships
//! a `disable_paste_burst` key, so a TUI that submitted a bracketed paste on
//! `paste_end` by itself would produce the same reading with any submit byte at
//! all — including none. One field differs between the two specs. The control
//! must show the body **painted into the composer** and **absent from the wire**,
//! which is precisely the lie the typing-profile seam exists to refuse: bytes
//! delivered, `write_paste` returning `Ok`, `accepted` reported, and nothing
//! submitted (Tier 1.5, D-034).
//!
//! ## What this file does not do
//!
//! It adds nothing between `fleet send` and a pty. Both arms call
//! `PaneRegistry::write_paste`, the exact call `deliver` makes, and the only
//! delay anywhere in the path is still `TypingProfile::submit_gap_ms` — D-034's
//! one sanctioned delay, per-harness since #14 (C26). There is no startup wait
//! here and none on checkpoint 9; a codex pane's readiness is `BringUp` on the
//! spawn path, which is #42's and is asserted next door in `codex_bringup.rs`.
//!
//! ## What C22's failing row turns out to have been
//!
//! C22 measured 6 s startup + 0.6 s post-paste submitting nothing and 12 s + 1.5
//! s submitting, and #24 withdrew any claim about which of the two was
//! load-bearing (C37). The arm below submits at the profile's **30 ms** gap —
//! twenty times shorter than the gap in C22's *failing* row — on a pane woken by
//! #42. So the post-paste gap was not the discriminator, and C22's table needs no
//! number encoded from it. That is a reading, not a re-run of C22: this file does
//! not reproduce its 6 s arm.
//!
//! ## Cost, stated (D-081)
//!
//! Two more real interactive panes on a machine with `codex` installed. The
//! control is the cheaper of the two — it stops as soon as the composer paints —
//! and the skip is loud for C13's reason, because a tier that skips quietly rots
//! into decoration.

use std::time::Duration;

use fleetor_shell::placement::codex::CODEX_SPEC;
use fleetor_shell::placement::harness::TypingProfile;
use fleetor_shell::placement::HarnessSpec;

mod codex_pane;
use codex_pane::{announce, on_path, CodexPane, RECORDED_BUILD, VENDOR_BIN};

/// How long a submitted message is given to reach the wire. A budget for the
/// measurement; nothing in `src/` waits on this.
const ON_THE_WIRE: Duration = Duration::from_secs(25);

/// How long the composer gets to paint a body that was pasted into it.
const INTO_THE_COMPOSER: Duration = Duration::from_secs(15);

/// After the composer has painted it, how long the wire must stay clear.
///
/// A pane that was going to submit on `paste_end` alone would already have done
/// it — the paste is complete before the composer can paint it — so this is a
/// margin rather than a race the control could lose by being impatient.
const STAYS_UNSUBMITTED: Duration = Duration::from_secs(5);

/// The three lines. Whitespace-free and unmistakable, because
/// `CodexPane::screen` collapses whitespace and a common word would match the
/// vendor's own chrome.
const ALPHA: &str = "TYPING-SENTINEL-ALPHA";
const BETA: &str = "TYPING-SENTINEL-BETA";
const GAMMA: &str = "TYPING-SENTINEL-GAMMA";

/// The same spec codex ships with, one field changed: this profile frames a
/// paste and then submits nothing.
///
/// **Built from `..CODEX_SPEC.typing` rather than written out**, so the control
/// cannot drift into being a different profile. If it ever differs in a second
/// field, the arm below stops being a control and nobody would be told.
static NEVER_SUBMITS: HarnessSpec = HarnessSpec {
    typing: TypingProfile { submit_bytes: &[], ..CODEX_SPEC.typing },
    ..CODEX_SPEC
};

/// **The ticket: a message sent to a live codex pane is submitted, and one paste
/// is one turn** (#32).
#[test]
fn one_paste_becomes_one_submitted_turn() {
    let Some(vendor) = on_path(VENDOR_BIN) else {
        announce(&[
            format!("SKIPPED: codex typing profile (#32) — `{VENDOR_BIN}` is not on PATH."),
            format!("  Recorded against {RECORDED_BUILD}. Nothing below was measured:"),
            "    a bracketed multi-line paste reaches the model as ONE turn".into(),
            "    the same paste with no submit byte sits in the composer forever".into(),
            "  The conformance suite still asserts checkpoint 9 against a stand-in pane,".into(),
            "  which proves the bytes leave FLEETOR framed as the profile says. It cannot".into(),
            "  prove the vendor's own composer submits on them.".into(),
        ]);
        return;
    };

    let message = format!("{ALPHA}\n{BETA}\n{GAMMA}");
    let pane = CodexPane::brought_up(&vendor, &CODEX_SPEC, "typing");
    pane.deliver(&message).expect("the delivery");

    assert!(
        pane.reached_the_wire(ALPHA, ON_THE_WIRE),
        "a message delivered into a live codex pane never reached the model. The profile \
         framed it and wrote {:?} to submit it, {}ms after closing the paste — so either \
         those are no longer the bytes this vendor submits on, or the pane was announced \
         before it could receive (#42). The pane painted {} bytes and its screen ends: {}",
        String::from_utf8_lossy(CODEX_SPEC.typing.submit_bytes),
        CODEX_SPEC.typing.submit_gap_ms,
        pane.bytes_painted(),
        tail(&pane.screen()),
    );

    let body = pane.capture.body_carrying(ALPHA).expect("the request that carried the first line");
    assert!(
        body.contains(BETA) && body.contains(GAMMA),
        "the first request carried only part of the message, so the paste was submitted \
         line by line rather than as one turn — which is what an unbracketed body does, \
         and what `bracketed_paste` is a field to prevent. The fleet's own delivery would \
         reach a worker as {} separate turns.",
        pane.capture.requests(),
    );

    assert!(
        !pane.screen().to_lowercase().contains("trustthecontents"),
        "the pane put up the trust dialog, so the keypress that woke it may have been \
         answering *that* rather than dismissing a splash, and this arm would pass for the \
         wrong reason. Checkpoint 14 seeds the trust record precisely so a pane never meets \
         this question (C6, C17, C51)."
    );
}

/// **The control: the same paste with no submit byte is delivered, painted, and
/// never submitted.**
///
/// Without this the arm above would pass against a vendor that submits on
/// `paste_end` by itself, and checkpoint 9's `submit_bytes` would be a field
/// nothing depends on.
#[test]
fn a_paste_with_no_submit_byte_sits_in_the_composer_and_never_reaches_the_wire() {
    let Some(vendor) = on_path(VENDOR_BIN) else {
        return; // the arm above owns the announcement; two banners would be noise
    };

    let pane = CodexPane::brought_up(&vendor, &NEVER_SUBMITS, "unsubmitted");
    pane.deliver(ALPHA).expect("the delivery");

    assert!(
        pane.painted_on_screen(ALPHA, INTO_THE_COMPOSER),
        "the control's body never appeared in the composer at all, so this arm is measuring \
         a pane that did not receive rather than one that did not submit — and it would \
         report `not submitted` for a pane that was never sent anything. The pane painted \
         {} bytes and its screen ends: {}",
        pane.bytes_painted(),
        tail(&pane.screen()),
    );

    assert!(
        !pane.reached_the_wire(ALPHA, STAYS_UNSUBMITTED),
        "a paste framed with no submit byte reached the model anyway, so this vendor now \
         submits on `paste_end` alone. The arm next door proves nothing about \
         `submit_bytes` while that is true — re-measure against {RECORDED_BUILD} and read \
         `disable_paste_burst` before changing the profile."
    );
}

/// The last of a collapsed screen, for a failure message. The interesting part of
/// a pane that went wrong is what it painted most recently.
fn tail(screen: &str) -> String {
    // By character, not by byte: a collapsed screen keeps the vendor's
    // box-drawing, and slicing 240 bytes back lands mid-codepoint.
    let from = screen.chars().count().saturating_sub(240);
    format!("…{}", screen.chars().skip(from).collect::<String>())
}
