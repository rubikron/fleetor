//! What the rail offers, and where the app opens.
//!
//! Source-reading tripwires: the frontend has no test runner, and this suite
//! deliberately does not add one. They read declarations, not behaviour.

use std::path::{Path, PathBuf};

const RAIL: &str = "ui/src/components/Sidebar.tsx";
const STAGE: &str = "ui/src/App.tsx";

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

/// **The app opens on Home**, whatever view it was closed on: no fleet is running
/// at launch, so a restored Terminals or Feed would open on nothing.
#[test]
fn the_app_opens_on_home() {
    assert!(
        read(STAGE).contains(r#"useState<View>("home")"#),
        "{STAGE} no longer starts on Home",
    );
    assert!(
        !repo_root().join("ui/src/ui/usePersistedNav.ts").exists(),
        "the view is being restored from storage again",
    );
}

/// The consolidated views (home, feed) and History are in the rail.
#[test]
fn the_rail_names_the_consolidated_views() {
    let rail = rail_views();
    assert!(rail.len() >= 5, "the rail's View union parsed as {rail:?} — that cannot be right");
    for view in ["home", "feed", "history"] {
        assert!(rail.iter().any(|v| v == view), "{view} must be in the rail: {rail:?}");
    }
}

/// The old views are gone from the rail (D-092).
#[test]
fn legacy_views_are_not_in_the_rail() {
    let rail = rail_views();
    for view in ["messages", "activity", "critic", "evaluator", "review"] {
        assert!(!rail.iter().any(|v| v == view), "{view} should not be in the rail: {rail:?}");
    }
}
