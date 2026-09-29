//! `fleet reply` refuses a recipient, and the refusal is a real process exit
//! (D-077).
//!
//! The unit tests beside `reply_text` pin the discriminator and the sentence.
//! These two run the actual `fleet` binary, because the two things a pane
//! actually observes are its exit code and its stderr — the briefs promise
//! "non-zero means not delivered" verbatim, and a check that returned `Err`
//! into a `0` exit would be worse than the bug it replaced.
//!
//! **The second test is the important one.** It proves the refusal is *narrow*:
//! a message that merely begins with a peer's name is not stopped here, it goes
//! on to dial the socket and fails there for the ordinary reason. Replacing a
//! silent misroute with a silent refusal would be the worse trade.

use std::process::{Command, Output};

/// The binary this crate builds, run with a clean fleet environment so neither
/// test can accidentally reach a hub the developer happens to be running.
fn fleet(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fleet"))
        .args(args)
        .env_remove("FLEETOR_PANE")
        .env_remove("FLEET_SOCKET")
        .output()
        .expect("running the fleet binary")
}

#[test]
fn naming_a_recipient_exits_non_zero_and_says_so_on_stderr() {
    let out = fleet(&["reply", "worker-3", "Agreed — Rooftop Garden"]);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(!out.status.success(), "a refused reply must not exit 0: {out:?}");
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert_eq!(
        stderr.trim(),
        "fleet: `fleet reply` does not take a recipient — use `fleet send worker-3 \"<text>\"`, \
         or drop the name to answer whoever messaged you last"
    );
    assert!(out.stdout.is_empty(), "nothing is claimed on stdout: {:?}", out.stdout);
}

#[test]
fn a_message_that_begins_with_a_peers_name_is_not_refused_here() {
    let out = fleet(&["reply", "worker-3 said the annex is out"]);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(
        !stderr.contains("does not take a recipient"),
        "prose that opens with a pane name is a message, not a mistake: {stderr}"
    );
    // It got past every local check and failed where any `fleet` verb fails
    // without a hub — which is exactly as far as this test can push it.
    assert!(stderr.contains("FLEET_SOCKET is not set"), "{stderr}");
}
