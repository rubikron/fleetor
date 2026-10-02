//! The interface declares its views twice, and the two lists must agree (D-073).
//!
//! The rail in `ui/src/components/Sidebar.tsx` says which views exist. The
//! restore list in `ui/src/ui/usePersistedNav.ts` says which of them may come
//! back on relaunch. A view in the first and not the second is selectable and
//! then silently unreachable: the persisted id fails `isValidView`, falls back
//! to Terminals, and says nothing — the *same* fallback a corrupt value gets,
//! which is precisely why it is invisible. `history` sat in that state from the
//! day it shipped until this test was written.
//!
//! **Why a Rust test for a TypeScript property.** The frontend has no test
//! runner, and this arc deliberately does not add one — introducing one as a
//! side effect of moving a terminal into a tab is how a small package becomes
//! the polluted one. So this is a source-reading tripwire: what is worth
//! pinning is that a future session cannot add a
//! view to one list and forget the other without a test noticing.
//!
//! It reads the *lists*, not the behaviour. It cannot tell you that restoring a
//! view works — only that the two lists name the same things, which is the one
//! way this has actually gone wrong.

use std::path::{Path, PathBuf};

const RAIL: &str = "ui/src/components/Sidebar.tsx";
const RESTORE: &str = "ui/src/ui/usePersistedNav.ts";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("src-tauri has a parent").to_path_buf()
}

fn read(rel: &str) -> String {
    let file = repo_root().join(rel);
    std::fs::read_to_string(&file)
        .unwrap_or_else(|e| panic!("{} must exist to be checked: {e}", file.display()))
}

/// Every double-quoted literal in `text`, in order. The two declarations this
/// file reads contain nothing but view names between their delimiters, so a
/// quote scan is the whole parse — and a declaration that grew something else
/// inside it would fail loudly here rather than silently drop a name.
fn quoted(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('"') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find('"') else { break };
        out.push(rest[..close].to_string());
        rest = &rest[close + 1..];
    }
    out
}

/// The section of `text` between `after` and the next `until`, panicking with
/// the declaration's name if the shape it depends on is gone — a renamed
/// declaration must fail this test rather than quietly match nothing and pass.
fn between(text: &str, after: &str, until: char, what: &str) -> String {
    let tail = text
        .split(after)
        .nth(1)
        .unwrap_or_else(|| panic!("{what} no longer starts with `{after}` — this test reads it"));
    tail.split(until)
        .next()
        .unwrap_or_else(|| panic!("{what} has no closing `{until}`"))
        .to_string()
}

/// The views the rail declares: the `View` union in `Sidebar.tsx`.
fn rail_views() -> Vec<String> {
    quoted(&between(&read(RAIL), "export type View =", ';', "the rail's View union"))
}

/// The views the app will restore: the `VIEWS` array in `usePersistedNav.ts`.
fn restorable_views() -> Vec<String> {
    quoted(&between(&read(RESTORE), "const VIEWS: readonly View[] = [", ']', "the restore list"))
}

/// **The tripwire.** Both lists, compared in both directions, with the missing
/// names in the message — a failure that only said "these differ" would send the
/// next session back to diffing two files by eye.
#[test]
fn the_rail_and_the_restore_list_name_the_same_views() {
    let rail = rail_views();
    let restorable = restorable_views();

    assert!(rail.len() >= 5, "the rail's View union parsed as {rail:?} — that cannot be right");

    let unrestorable: Vec<&String> = rail.iter().filter(|v| !restorable.contains(v)).collect();
    assert!(
        unrestorable.is_empty(),
        "{unrestorable:?} {} in the rail ({RAIL}) and missing from the restore list \
         ({RESTORE}). The operator can select {} and, on the next launch, land on Terminals \
         with nothing said — the same silent fallback a corrupt stored value gets. Add the \
         name to `VIEWS`.\n  rail:     {rail:?}\n  restores: {restorable:?}",
        if unrestorable.len() == 1 { "is" } else { "are" },
        if unrestorable.len() == 1 { "it" } else { "them" },
    );

    let phantom: Vec<&String> = restorable.iter().filter(|v| !rail.contains(v)).collect();
    assert!(
        phantom.is_empty(),
        "{phantom:?} in the restore list ({RESTORE}) and not in the rail ({RAIL}). Restoring a \
         view the rail does not declare puts the app on a stage nothing can navigate away \
         from, which is worse than the omission this test was written for.\n  \
         rail:     {rail:?}\n  restores: {restorable:?}",
    );
}

/// History must be in both lists.
#[test]
fn history_is_restorable() {
    assert!(
        restorable_views().iter().any(|v| v == "history"),
        "`history` must be in the restore list",
    );
}

/// The consolidated views (home, feed) are in both lists.
#[test]
fn consolidated_views_are_restorable() {
    let rail = rail_views();
    let restorable = restorable_views();
    for view in ["home", "feed"] {
        assert!(rail.iter().any(|v| v == view), "{view} must be in the rail: {rail:?}");
        assert!(restorable.iter().any(|v| v == view), "{view} must be restorable: {restorable:?}");
    }
}

/// The old views are gone from the rail and the restore list (D-092).
#[test]
fn legacy_views_are_not_in_the_rail() {
    let rail = rail_views();
    let restorable = restorable_views();
    for view in ["messages", "activity", "critic", "evaluator", "review"] {
        assert!(!rail.iter().any(|v| v == view), "{view} should not be in the rail: {rail:?}");
        assert!(!restorable.iter().any(|v| v == view), "{view} should not be restorable: {restorable:?}");
    }
}

/// A persisted `review`, `critic` or `evaluator` from an older build lands on
/// Home rather than wedging the app (D-092).
#[test]
fn removed_views_restore_to_home() {
    let map = between(&read(RESTORE), "const LEGACY_VIEW_MAP: Record<string, View> = {", '}', "the legacy map");
    for view in ["review", "critic", "evaluator"] {
        assert!(
            map.contains(&format!("{view}: \"home\"")),
            "a stored `{view}` must restore to home ({RESTORE}):\n{map}",
        );
    }
}
